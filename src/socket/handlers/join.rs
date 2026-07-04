use std::collections::HashMap;
use axum::extract::ws::Message;
use serde_json::json;
use sqlx::Row;
use base64::{ engine::general_purpose::STANDARD, Engine };
use hmac::{ Hmac, Mac, KeyInit };
use sha1::Sha1;
use chrono::Utc;

use crate::{
    services::attendance_service::AttendanceService,
    socket::{ events::log_join, room_manager::{ ClientSender, ParticipantState, Room } },
    state::AppState,
    utils::error::error_msg,
};

pub async fn handle_join(
    state: &AppState,
    room_id: &str,
    user_id: &str,
    name: &str,
    tx: ClientSender,
    incoming_session_id: &str,
    host_id: Option<String>,
    camera_stream_id: Option<String>
) {
    println!("👤 JOIN STARTED: {} ({}) -> room {}", user_id, name, room_id);

    // GENERATE TURN CREDENTIALS
    let expiration = Utc::now().timestamp() + 24 * 3600;
    let username = format!("{}:{}", expiration, user_id);

    let mut mac = Hmac::<Sha1>
        ::new_from_slice(state.turn_config.auth_secret.as_bytes())
        .expect("HMAC can take key of any size");
    mac.update(username.as_bytes());
    let result = mac.finalize().into_bytes();

    let credential = STANDARD.encode(result);
    println!("🔐 Generated TURN credentials for user {}", name);

    // CHECK ROOM EXISTS & FETCH NAME
    let room_name: String = match
        sqlx
            ::query_scalar("SELECT name FROM rooms WHERE id = $1")
            .bind(room_id)
            .fetch_optional(&state.db).await
    {
        Ok(Some(name)) => name,
        Ok(None) => {
            eprintln!("Room {} does not exist", room_id);
            let _ = tx.send(error_msg("Room does not exist"));
            return;
        }
        Err(e) => {
            eprintln!("Database error while checking room: {:?}", e);
            let _ = tx.send(error_msg("Database error while joining room"));
            return;
        }
    };

    // START TRANSACTION
    let mut tx_db = match state.db.begin().await {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to start database transaction: {:?}", e);
            let _ = tx.send(error_msg("Failed to start database transaction"));
            return;
        }
    };

    let rsid = uuid::Uuid::new_v4().to_string();

    // INSERT ROOM SESSION
    if
        let Err(e) = sqlx
            ::query(
                r#"
        INSERT INTO room_sessions (id, room_id, started_at)
        VALUES ($1, $2, NOW())
        ON CONFLICT (id) DO NOTHING
        "#
            )
            .bind(&rsid)
            .bind(room_id)
            .execute(&mut *tx_db).await
    {
        eprintln!("Failed to create room session: {:?}", e);
        let _ = tx.send(error_msg("Failed to create room session"));
        let _ = tx_db.rollback().await;
        return;
    }

    // RECORD ATTENDANCE
    if let Err(e) = AttendanceService::mark_join(&state.db, room_id, user_id, name).await {
        eprintln!("Failed to record attendance: {:?}", e);
        let _ = tx.send(error_msg("Failed to record attendance"));
        let _ = tx_db.rollback().await;
        return;
    }

    // INSERT PARTICIPANT SESSION
    if
        let Err(e) = sqlx
            ::query(
                r#"
        INSERT INTO participant_sessions
        (id, user_id, room_id, name, room_session_id, joined_at, last_seen)
        VALUES ($1, $2, $3, $4, $5, NOW(), NOW())
        ON CONFLICT (id)
        DO UPDATE SET last_seen = NOW()
        "#
            )
            .bind(incoming_session_id)
            .bind(user_id)
            .bind(room_id)
            .bind(name)
            .bind(&rsid)
            .execute(&mut *tx_db).await
    {
        eprintln!("Failed to create participant session: {:?}", e);
        let _ = tx.send(error_msg("Failed to create participant session"));
        let _ = tx_db.rollback().await;
        return;
    }

    // UPSERT PARTICIPANT
    if
        let Err(e) = sqlx
            ::query(
                r#"
        INSERT INTO participants (id, room_id, name, first_joined_at, last_seen)
        VALUES ($1, $2, $3, NOW(), NOW())
        ON CONFLICT (id, room_id)
        DO UPDATE SET
            last_seen = NOW(),
            name = EXCLUDED.name
        "#
            )
            .bind(user_id)
            .bind(room_id)
            .bind(name)
            .execute(&mut *tx_db).await
    {
        eprintln!("Failed to register participant: {:?}", e);
        let _ = tx.send(error_msg("Failed to register participant"));
        let _ = tx_db.rollback().await;
        return;
    }

    // LOG JOIN EVENT - INSIDE TRANSACTION
    if let Err(e) = log_join(&mut tx_db, room_id, user_id, incoming_session_id, name).await {
        eprintln!("Failed to log join event: {:?}", e);
        let _ = tx.send(error_msg("Failed to log join event"));
        let _ = tx_db.rollback().await;
        return;
    }

    // COMMIT TRANSACTION
    if let Err(e) = tx_db.commit().await {
        eprintln!("Failed to commit join transaction: {:?}", e);
        let _ = tx.send(error_msg("Failed to commit join transaction"));
        return;
    }

    println!("Database transaction committed for user {}", user_id);

    // FIX: Collect data from write lock, then release it
    // This prevents deadlock by not holding write lock while acquiring read lock
    let (existing_participants, presenter_id, _, pending_reqs, is_host, session_id_to_use) = {
        let mut rooms = state.rooms.write().await;

        let room = rooms.entry(room_id.to_string()).or_insert(Room {
            participants: HashMap::new(),
            sessions: HashMap::new(),
            senders: HashMap::new(),
            presenter_id: None,
            presenter_stream_id: None,
            host_id: host_id.clone(),
            is_open: Some(false),
            pending_requests: HashMap::new(),
            approved_users: std::collections::HashSet::new(),
        });

        // Ensure host_id is set from authoritative source
        room.host_id = host_id.clone();

        // Clean up old sessions for this user (handle reconnects)
        let old_sessions: Vec<String> = room.sessions
            .iter()
            .filter(|(_, uid)| *uid == user_id)
            .map(|(sid, _)| sid.clone())
            .collect();

        for sid in old_sessions {
            room.sessions.remove(&sid);
            room.senders.remove(&sid);
            println!("Cleaned up old session {} for user {}", sid, user_id);
        }

        // Register new session
        let session_id = incoming_session_id.to_string();

        // overwrite session mapping
        room.sessions.insert(session_id.clone(), user_id.to_string());

        // ALWAYS ensure participant exists (idempotent upsert)
        room.participants.insert(user_id.to_string(), ParticipantState {
            id: user_id.to_string(),
            name: name.to_string(),
            session_id: session_id.clone(),
            last_seen: chrono::Utc::now().timestamp() as u64,
            is_presenter: false,
            is_host: user_id == host_id.clone().unwrap_or_default(),
            camera_stream_id: camera_stream_id,
            screen_share_stream_id: None,
        });

        room.senders.insert(session_id.clone(), tx.clone());

        // Build existing participants list
        let existing: Vec<_> = room.participants
            .values()
            .filter(|p| p.id != user_id)
            .map(|p| {
                let mut participant_json =
                    json!({
                    "id": p.id,
                    "name": p.name,
                    "session_id": p.session_id,
                    "isHost": p.is_host,
                    "cameraId": p.camera_stream_id,
                    "isPresenter": p.is_presenter,
                });

                if let Some(pid) = &room.presenter_id {
                    if pid == &p.id {
                        if let Some(stream_id) = room.presenter_stream_id.as_ref() {
                            participant_json["isScreenSharing"] = json!(true);
                            participant_json["remoteScreenStreamId"] = json!(stream_id);
                        }
                    }
                }

                participant_json
            })
            .collect();

        let is_host_flag = host_id.as_deref() == Some(user_id);
        let pending_requests_list = if is_host_flag {
            room.pending_requests.values().cloned().collect()
        } else {
            vec![]
        };

        // Collect everything we need while holding the lock
        (
            existing,
            room.presenter_id.clone(),
            room.presenter_stream_id.clone(),
            pending_requests_list,
            is_host_flag,
            session_id,
        )
    }; // ← WRITE LOCK RELEASED HERE

    println!("🔓 Released write lock after updating room state");

    // BROADCAST JOIN TO OTHERS
    {
        let rooms = state.rooms.read().await; // ← Safe to acquire read lock now

        if let Some(room) = rooms.get(room_id) {
            let join_msg = Message::Text(
                json!({
                    "type": "USER_JOINED",
                    "participant": {
                        "id": user_id,
                        "name": name,
                        "session_id": &session_id_to_use
                    }
                })
                    .to_string()
                    .into()
            );

            let mut broadcast_count = 0;
            for (sid, sender) in room.senders.iter() {
                // Skip sending to the joining user's own session
                if let Some(owner) = room.sessions.get(sid) {
                    if owner == user_id {
                        continue;
                    }
                } else {
                    eprintln!("WARNING: Sender {} has no corresponding session owner", sid);
                    continue;
                }

                if let Err(e) = sender.send(join_msg.clone()) {
                    eprintln!("Failed to broadcast USER_JOINED to session {}: {:?}", sid, e);
                } else {
                    broadcast_count += 1;
                }
            }
            println!("Broadcasted join to {} other participants", broadcast_count);
        }
    } // ← READ LOCK RELEASED HERE

    // SEND PENDING REQUESTS TO HOST
    if is_host {
        println!("HOST JOINED - Sending pending join requests");

        if pending_reqs.is_empty() {
            println!("No pending join requests");
        } else {
            println!("Found {} pending requests", pending_reqs.len());

            for req in &pending_reqs {
                let pending_msg = Message::Text(
                    json!({
                        "type": "JOIN_REQUEST",
                        "request": {
                            "id": &req.id,
                            "user_id": &req.user_id,
                            "name": &req.name
                        }
                    })
                        .to_string()
                        .into()
                );

                if let Err(e) = tx.send(pending_msg) {
                    eprintln!("Failed to send pending request to host: {:?}", e);
                } else {
                    println!("Sent pending request: {} ({})", req.name, req.id);
                }
            }
        }
    }

    // SEND JOIN CONFIRMATION WITH ICE SERVERS
    let joined_msg =
        json!({
        "type": "JOINED",
        "room_id": room_id,
        "room_name": room_name,
        "user_id": user_id,
        "session_id": &session_id_to_use,
        "iceServers": [
            {
                "urls": [format!("stun:{}:3478", state.turn_config.server)]
            },
            {
                "urls": [format!("turn:{}:3478", state.turn_config.server)],
                "username": username,
                "credential": credential
            }
        ]
    });

    match tx.send(Message::Text(joined_msg.to_string().into())) {
        Ok(_) => println!("✔ JOINED delivered"),
        Err(e) => eprintln!("JOINED send failed: {:?}", e),
    }

    // Send EXISTING_USERS TO NEW USER
    if
        let Err(e) = tx.send(
            Message::Text(
                json!({
                    "type": "EXISTING_USERS",
                    "participants": existing_participants,
                    "presenterId": presenter_id
                })
                    .to_string()
                    .into()
            )
        )
    {
        eprintln!("Failed to send existing users list: {:?}", e);
    }

    println!("JOIN COMPLETE for user {} in room {}", user_id, room_id);
}

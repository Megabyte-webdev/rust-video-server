use std::collections::HashMap;
use axum::extract::ws::Message;
use serde_json::json;
use sqlx::Row;
use tokio::sync::mpsc::UnboundedSender;
use base64::{ engine::general_purpose::STANDARD, Engine };
use hmac::{ Hmac, Mac, KeyInit };
use sha1::Sha1;
use chrono::Utc;

use crate::{
    services::attendance_service::AttendanceService,
    socket::{ events::log_join, room_manager::{ ParticipantState, Room } },
    state::AppState,
    utils::error::error_msg,
};

pub async fn handle_join(
    state: &AppState,
    room_id: &str,
    user_id: &str,
    name: &str,
    tx: UnboundedSender<Message>,
    incoming_session_id: &str,
    host_id: Option<String>
) {
    let auth_secret = std::env::var("TURN_AUTH_SECRET").unwrap_or_else(|_| "".to_string());
    let turn_server = std::env
        ::var("TURN_SERVER")
        .map_err(|_| "TURN SERVER must be set")
        .expect("Failed to fetch TURN SERVER");

    let expiration = Utc::now().timestamp() + 24 * 3600;
    let username = format!("{}:{}", expiration, user_id);

    let mut mac = Hmac::<Sha1>
        ::new_from_slice(auth_secret.as_bytes())
        .expect("HMAC can take key of any size");
    mac.update(username.as_bytes());
    let result = mac.finalize().into_bytes();

    // Base64 encode
    let credential = STANDARD.encode(result);
    println!(" Generated credentials:");
    println!("  username: {}: {}", name, username);
    println!("  credential: {}", credential);

    // CHECK ROOM EXISTS & FETCH NAME
    let room_name: String = match
        sqlx
            ::query_scalar("SELECT name FROM rooms WHERE id = $1")
            .bind(room_id)
            .fetch_optional(&state.db).await
    {
        Ok(Some(name)) => name,
        Ok(None) => {
            let _ = tx.send(error_msg("Room does not exist"));
            return;
        }
        Err(_) => {
            let _ = tx.send(error_msg("Database error while joining room"));
            return;
        }
    };

    println!("JOIN STARTED: {} -> {}", user_id, room_id);

    // START TRANSACTION
    let mut tx_db = match state.db.begin().await {
        Ok(t) => t,
        Err(_) => {
            let _ = tx.send(error_msg("Failed to start database transaction"));
            return;
        }
    };

    let rsid = uuid::Uuid::new_v4().to_string();

    // ROOM SESSION
    if
        let Err(_) = sqlx
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
        let _ = tx.send(error_msg("Failed to create room session"));
        return;
    }

    // ATTENDANCE
    if let Err(_) = AttendanceService::mark_join(&state.db, room_id, user_id, name).await {
        let _ = tx.send(error_msg("Failed to record attendance"));
        return;
    }

    // PARTICIPANT SESSION
    if
        let Err(_) = sqlx
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
        let _ = tx.send(error_msg("Failed to create participant session"));
        return;
    }

    // PARTICIPANT UPSERT
    if
        let Err(_) = sqlx
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
        let _ = tx.send(error_msg("Failed to register participant"));
        return;
    }

    // COMMIT TRANSACTION
    if let Err(_) = tx_db.commit().await {
        let _ = tx.send(error_msg("Failed to commit join transaction"));
        return;
    }

    let mut rooms = state.rooms.write().await;

    let room = rooms.entry(room_id.to_string()).or_insert(Room {
        participants: HashMap::new(),
        sessions: HashMap::new(),
        senders: HashMap::new(),
        presenter_id: None,
        host_id: host_id.clone(),
        is_open: Some(false),
        pending_users: HashMap::new(),
    });

    // Ensure host_id is always set from the source of truth
    room.host_id = host_id.clone();

    // Clean old sessions
    let old_sessions: Vec<String> = room.sessions
        .iter()
        .filter(|(_, uid)| *uid == user_id)
        .map(|(sid, _)| sid.clone())
        .collect();

    for sid in old_sessions {
        room.sessions.remove(&sid);
        room.senders.remove(&sid);
    }

    // Register new session
    let session_id = incoming_session_id.to_string();

    room.sessions.insert(session_id.clone(), user_id.to_string());

    room.participants.insert(user_id.to_string(), ParticipantState {
        id: user_id.to_string(),
        name: name.to_string(),
        session_id: session_id.clone(),
        last_seen: chrono::Utc::now().timestamp() as u64,
    });

    room.senders.insert(session_id.clone(), tx.clone());

    // Check if this user is the host
    let is_host = host_id.as_deref() == Some(user_id);

    // EXISTING USERS
    let existing: Vec<_> = room.participants
        .values()
        .filter(|p| p.id != user_id)
        .map(|p| {
            json!({
                "id": p.id,
                "name": p.name,
                "session_id": p.session_id
            })
        })
        .collect();

    let _ = tx.send(
        Message::Text(
            json!({
            "type": "EXISTING_USERS",
            "participants": existing,
            "presenterId": room.presenter_id
        })
                .to_string()
                .into()
        )
    );

    // BROADCAST JOIN
    let join_msg = Message::Text(
        json!({
            "type": "USER_JOINED",
            "participant": {
                "id": user_id,
                "name": name,
                "session_id": session_id
            }
        })
            .to_string()
            .into()
    );

    for (sid, sender) in room.senders.iter() {
        if let Some(owner) = room.sessions.get(sid) {
            if owner == user_id {
                continue;
            }
        }

        let _ = sender.send(join_msg.clone());
    }

    // SEND PENDING JOIN REQUESTS TO HOST (ONLY IF RECENT)
    if is_host {
        println!("👑 HOST JOINED - Fetching recent pending join requests...");

        // Only get requests created in last 30 minutes
        match
            sqlx
                ::query(
                    r#"
            SELECT id, user_id, name 
            FROM join_requests 
            WHERE room_id = $1 
            AND status = 'pending'
            AND created_at > datetime('now', '-30 minutes')
            ORDER BY created_at ASC
            "#
                )
                .bind(room_id)
                .fetch_all(&state.db).await
        {
            Ok(pending_reqs) => {
                if pending_reqs.is_empty() {
                    println!(" No recent pending join requests");
                } else {
                    println!(" Found {} recent pending requests", pending_reqs.len());
                    for req in &pending_reqs {
                        let req_id: String = req.get("id");
                        let req_user_id: String = req.get("user_id");
                        let req_name: String = req.get("name");

                        println!("Sending pending request: {} ({})", req_name, req_id);

                        // Send to host
                        let pending_msg = Message::Text(
                            json!({
                                "type": "JOIN_REQUEST",
                                "request": {
                                    "id": req_id,
                                    "user_id": req_user_id,
                                    "name": req_name
                                }
                            })
                                .to_string()
                                .into()
                        );

                        if let Err(e) = tx.send(pending_msg) {
                            println!("❌ Failed to send pending request to host: {:?}", e);
                        } else {
                            println!("✅ Sent pending request to host");
                        }
                    }

                    println!(" Sent {} recent pending join requests to host", &pending_reqs.len());
                }

                // Clean up very old expired requests (older than 30 mins)
                let _ = sqlx
                    ::query(
                        r#"
                DELETE FROM join_requests 
                WHERE room_id = $1 
                AND status = 'pending'
                AND created_at < datetime('now', '-30 minutes')
                "#
                    )
                    .bind(room_id)
                    .execute(&state.db).await;
            }
            Err(e) => {
                println!("❌ Failed to fetch pending requests: {:?}", e);
            }
        }
    }

    // LOG + FINAL ACK
    log_join(state, room_id, user_id, name, &session_id).await;

    let _ = tx.send(
        Message::Text(
            json!({
            "type": "JOINED",
            "room_id": room_id,
            "room_name": room_name,
            "user_id": user_id,
            "session_id": session_id,
             "iceServers": [
                    {
                        "urls": [format!("stun:{}:3478", turn_server)]
                    },
                    {
                        "urls": [format!("turn:{}:3478", turn_server)],
                        "username": username,
                        "credential": credential
                    }
                ]
        })
                .to_string()
                .into()
        )
    );

    println!("JOIN COMPLETE");
}

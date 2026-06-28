use std::collections::HashMap;

use axum::extract::ws::{ Message, WebSocket };
use futures_util::{ SinkExt, StreamExt };
use sqlx::Row;
use tokio::sync::mpsc::unbounded_channel;

use crate::{
    socket::handlers::{
        join::handle_join,
        leave::handle_leave,
        media_state::handle_media_state,
        message::handle_message,
        screen_share::handle_screen_share,
        signaling::handle_signaling,
    },
    state::AppState,
};

pub async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = unbounded_channel::<Message>();

    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender.send(msg).await.is_err() {
                println!("❌ WS SEND FAILED (client likely disconnected)");
                break;
            }
        }
    });

    let mut room_id: Option<String> = None;
    let mut user_id: Option<String> = None;
    let mut session_id: Option<String> = None;
    let mut name: String = "Anonymous".to_string();

    while let Some(Ok(msg)) = receiver.next().await {
        let Message::Text(txt) = msg else {
            continue;
        };

        let Ok(value) = serde_json::from_str::<serde_json::Value>(&txt) else {
            continue;
        };

        let msg_type = value
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match msg_type {
            "JOIN" => {
                let rid = value["room_id"].as_str().unwrap_or("").to_string();
                let uid = value["user_id"].as_str().unwrap_or("").to_string();
                name = value["sender_name"].as_str().unwrap_or("Anonymous").to_string();

                println!("JOIN RECEIVED");
                println!("   room_id = {:?}", rid);
                println!("   user_id = {:?}", uid);
                println!("   sender = {:?}", name);

                room_id = Some(rid.clone());
                user_id = Some(uid.clone());
                session_id = Some(uuid::Uuid::new_v4().to_string());

                println!("QUERYING ROOM TABLE...");

                let room = sqlx
                    ::query(r#"SELECT is_open, created_by FROM rooms WHERE id = $1"#)
                    .bind(&rid)
                    .fetch_optional(&state.db).await;

                match &room {
                    Ok(Some(_)) => println!(" ROOM FOUND in DB"),
                    Ok(None) => println!("❌ ROOM NOT FOUND in DB"),
                    Err(e) => println!("💥 SQL ERROR: {:?}", e),
                }

                let Ok(Some(room)) = room else {
                    let _ = tx.send(Message::Text(r#"{"type":"ROOM_NOT_FOUND"}"#.into()));
                    return;
                };

                let is_open: bool = room.get("is_open");
                let host_id: Option<String> = room.get("created_by");

                println!("📦 ROOM DATA: is_open={:?}, created_by={:?}", is_open, host_id);

                let is_host = host_id.as_deref() == Some(&uid);

                println!("👤 is_host = {}", is_host);

                if is_open || is_host {
                    println!("🚪 ALLOWING JOIN");
                    handle_join(
                        &state,
                        &rid,
                        &uid,
                        &name,
                        tx.clone(),
                        &session_id.clone().unwrap(),
                        host_id.clone()
                    ).await;
                } else {
                    let request_id = uuid::Uuid::new_v4().to_string();

                    println!("🔒 ROOM CLOSED - Creating join request: {}", request_id);

                    let _ = sqlx
                        ::query(
                            r#"
            INSERT INTO join_requests (id, room_id, user_id, name, status, created_at)
            VALUES ($1, $2, $3, $4, 'pending', NOW())
            "#
                        )
                        .bind(&request_id)
                        .bind(&rid)
                        .bind(&uid)
                        .bind(&name)
                        .execute(&state.db).await;

                    println!("✅ Join request stored in DB: {}", request_id);

                    // notify user they're pending
                    let pending_msg =
                        serde_json::json!({
                        "type": "JOIN_PENDING",
                        "request_id": &request_id
                    })
                            .to_string()
                            .into();

                    if let Err(e) = tx.send(Message::Text(pending_msg)) {
                        println!("❌ Failed to send JOIN_PENDING to user: {:?}", e);
                    } else {
                        println!("✅ Sent JOIN_PENDING to user {}", uid);
                    }

                    // Store pending user's tx for later approval/rejection
                    {
                        let mut rooms = state.rooms.write().await;
                        let room_entry = rooms
                            .entry(rid.clone())
                            .or_insert_with(|| crate::socket::room_manager::Room {
                                participants: HashMap::new(),
                                sessions: HashMap::new(),
                                senders: HashMap::new(),
                                presenter_id: None,
                                host_id: host_id.clone(),
                                is_open: Some(false),
                                pending_users: HashMap::new(),
                            });

                        room_entry.pending_users.insert(uid.clone(), tx.clone());
                        room_entry.host_id = host_id.clone();
                        println!("✅ Stored pending user {} in room state", uid);
                    }

                    // Notify host if they're already in the room
                    {
                        let rooms = state.rooms.read().await;
                        if let Some(room_state) = rooms.get(&rid) {
                            println!(
                                "📢 Notifying {} senders about new join request",
                                room_state.senders.len()
                            );
                            for sender in room_state.senders.values() {
                                let req_msg = Message::Text(
                                    serde_json::json!({
                        "type": "JOIN_REQUEST",
                        "request": {
                            "id": &request_id,
                            "user_id": &uid,
                            "name": &name
                        }
                    })
                                        .to_string()
                                        .into()
                                );

                                if let Err(e) = sender.send(req_msg) {
                                    println!("❌ Failed to notify host: {:?}", e);
                                } else {
                                    println!("✅ Notified host about pending request");
                                }
                            }
                        } else {
                            println!("⏳ Host not in room yet, will send when they join");
                        }
                    }
                }
            }
            "JOIN_APPROVE" => {
                let request_id = value["request_id"].as_str().unwrap_or("");

                println!("🎯 JOIN_APPROVE received for request: {}", request_id);

                let req = sqlx
                    ::query(r#"SELECT room_id, user_id, name FROM join_requests WHERE id = $1"#)
                    .bind(request_id)
                    .fetch_optional(&state.db).await;

                if let Ok(Some(r)) = req {
                    let r_room_id: String = r.get("room_id");
                    let r_user_id: String = r.get("user_id");
                    let r_name: String = r.get("name");

                    println!(
                        "📋 Found request - approving user {} to join room {}",
                        r_user_id,
                        r_room_id
                    );

                    // Get the APPROVED USER's tx channel (dropped before write lock)
                    let user_tx = {
                        let rooms = state.rooms.read().await;
                        rooms
                            .get(&r_room_id)
                            .and_then(|room| room.pending_users.get(&r_user_id))
                            .cloned()
                    };

                    if let Some(user_tx) = user_tx {
                        // Send approval message
                        let approval_msg = Message::Text(
                            serde_json::json!({
                "type": "JOIN_APPROVED",
                "request_id": request_id,
                "user_id": &r_user_id, 
                "message": "Your join request was approved!"
            })
                                .to_string()
                                .into()
                        );

                        match user_tx.send(approval_msg) {
                            Ok(_) => println!("✅ Sent JOIN_APPROVED to user {}", r_user_id),
                            Err(e) =>
                                println!(
                                    "❌ Failed to send approval to user {}: {:?}",
                                    r_user_id,
                                    e
                                ),
                        }

                        // Update DB
                        let update_res = sqlx
                            ::query(r#"UPDATE join_requests SET status = 'approved' WHERE id = $1"#)
                            .bind(request_id)
                            .execute(&state.db).await;

                        match update_res {
                            Ok(_) => println!("✅ Updated request status in DB"),
                            Err(e) => println!("❌ Failed to update request in DB: {:?}", e),
                        }

                        // Get host_id for handle_join
                        let host_id = {
                            let rooms = state.rooms.read().await;
                            rooms.get(&r_room_id).and_then(|r| r.host_id.clone())
                        };

                        println!("🔌 Calling handle_join for approved user");

                        handle_join(
                            &state,
                            &r_room_id,
                            &r_user_id,
                            &r_name,
                            user_tx,
                            &uuid::Uuid::new_v4().to_string(),
                            host_id
                        ).await;

                        // Clean up pending user (after dropping read lock)
                        {
                            let mut rooms = state.rooms.write().await;
                            if let Some(room) = rooms.get_mut(&r_room_id) {
                                room.pending_users.remove(&r_user_id);
                                println!("✅ Removed user {} from pending_users", r_user_id);
                            }
                        }
                    } else {
                        println!("❌ User {} not in pending_users! (disconnected?)", r_user_id);
                    }
                } else {
                    println!("❌ Request {} not found in DB", request_id);
                }
            }
            "JOIN_REJECT" => {
                let request_id = value["request_id"].as_str().unwrap_or("");

                println!("❌ JOIN_REJECT received for request: {}", request_id);

                let req = sqlx
                    ::query(r#"SELECT room_id, user_id FROM join_requests WHERE id = $1"#)
                    .bind(request_id)
                    .fetch_optional(&state.db).await;

                let Ok(Some(req)) = req else {
                    println!("❌ Request {} not found in DB", request_id);
                    let _ = tx.send(
                        Message::Text(
                            r#"{"type":"JOIN_REJECT_FAILED","reason":"request_not_found"}"#.into()
                        )
                    );
                    continue;
                };

                let room_id_reject: String = req.get("room_id");
                let user_id_reject: String = req.get("user_id");

                println!(
                    "📋 Found request - rejecting user {} from room {}",
                    user_id_reject,
                    room_id_reject
                );

                // mark rejected in DB
                let update_res = sqlx
                    ::query(r#"UPDATE join_requests SET status = 'rejected' WHERE id = $1"#)
                    .bind(request_id)
                    .execute(&state.db).await;

                match update_res {
                    Ok(_) => println!("✅ Updated request status to rejected in DB"),
                    Err(e) => println!("❌ Failed to update request in DB: {:?}", e),
                }

                // Get user's tx and send rejection (DROP read lock before write)
                let user_tx = {
                    let rooms = state.rooms.read().await;
                    rooms
                        .get(&room_id_reject)
                        .and_then(|room| room.pending_users.get(&user_id_reject))
                        .cloned()
                };

                if let Some(user_tx) = user_tx {
                    let rejection_msg = Message::Text(
                        serde_json::json!({
                    "type": "JOIN_REJECTED",
                    "request_id": request_id,
                    "user_id": &user_id_reject, 
                    "reason": "Your join request was rejected"
                })
                            .to_string()
                            .into()
                    );

                    match user_tx.send(rejection_msg) {
                        Ok(_) => println!("✅ Sent JOIN_REJECTED to user {}", user_id_reject),
                        Err(e) =>
                            println!(
                                "❌ Failed to send rejection to user {}: {:?}",
                                user_id_reject,
                                e
                            ),
                    }
                } else {
                    println!("❌ User {} not in pending_users! (disconnected?)", user_id_reject);
                }

                // NOW safe to acquire write lock
                {
                    let mut rooms = state.rooms.write().await;
                    if let Some(room) = rooms.get_mut(&room_id_reject) {
                        room.pending_users.remove(&user_id_reject);
                        println!("✅ Removed user {} from pending_users", user_id_reject);
                    }
                }
            }

            "PING" => {
                if let (Some(rid), Some(uid), Some(rsid)) = (&room_id, &user_id, &session_id) {
                    let _ = sqlx
                        ::query(
                            r#"
            UPDATE participant_sessions
            SET last_seen = NOW()
            WHERE user_id = $1
            AND room_id = $2
            AND room_session_id = $3
            "#
                        )
                        .bind(uid)
                        .bind(rid)
                        .bind(rsid)
                        .execute(&state.db).await;
                }
                let _ = tx.send(Message::Text(r#"{"type":"PONG"}"#.to_string().into()));
            }

            "SCREEN_SHARE_START" => {
                if let (Some(rid), Some(uid)) = (&room_id, &user_id) {
                    let stream_id = value.get("stream_id").and_then(|v| v.as_str());
                    handle_screen_share(&state, rid, uid, true, stream_id).await;
                }
            }

            "SCREEN_SHARE_STOP" => {
                if let (Some(rid), Some(uid)) = (&room_id, &user_id) {
                    handle_screen_share(&state, rid, uid, false, None).await;
                }
            }
            "MEDIA_STATE" => {
                if let (Some(rid), Some(uid)) = (&room_id, &user_id) {
                    let kind = value
                        .get("kind")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let enabled = value
                        .get("enabled")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    handle_media_state(&state, rid, uid, kind, enabled).await;
                }
            }
            "CHAT_MESSAGE" => {
                if let (Some(rid), Some(uid)) = (&room_id, &user_id) {
                    handle_message(&state, rid, uid, &name, value).await;
                }
            }
            "OFFER" | "ANSWER" | "ICE" => {
                if let (Some(rid), Some(uid)) = (&room_id, &user_id) {
                    handle_signaling(&state, rid, uid, &txt).await;
                }
            }
            _ => (),
        }
    }

    println!("SOCKET CLOSED - Cleaning up user");

    if let Some(rid) = &room_id {
        if let Some(uid) = &user_id {
            // Clean up pending join request if this user had one
            println!("🗑️ Cleaning up join request for user {}", uid);
            let _ = sqlx
                ::query(
                    r#"DELETE FROM join_requests WHERE user_id = $1 AND room_id = $2 AND status = 'pending'"#
                )
                .bind(uid)
                .bind(rid)
                .execute(&state.db).await;

            // Remove from pending_users in memory
            {
                let mut rooms = state.rooms.write().await;
                if let Some(room) = rooms.get_mut(rid) {
                    if room.pending_users.remove(uid).is_some() {
                        println!("✅ Removed pending user {} from room state", uid);
                    }
                }
            }
        }
    }

    if
        let (Some(rid), Some(uid), Some(sid)) = (
            room_id.clone(),
            user_id.clone(),
            session_id.clone(),
        )
    {
        println!("CLEANING UP USER SESSION");

        handle_leave(&state, &rid, &uid, name.clone(), &sid).await;

        println!("CLEANUP COMPLETE");
    }
}

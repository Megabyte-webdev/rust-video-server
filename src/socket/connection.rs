use std::collections::HashMap;

use axum::extract::ws::{ Message, WebSocket };
use futures_util::{ SinkExt, StreamExt };
use sqlx::Row;
use tokio::sync::mpsc::unbounded_channel;

use crate::{
    socket::{
        handlers::{
            join::handle_join,
            leave::handle_leave,
            media_state::handle_media_state,
            message::handle_message,
            screen_share::handle_screen_share,
            signaling::handle_signaling,
        },
        room_manager::ClientSender,
    },
    state::AppState,
};

pub async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = unbounded_channel::<Message>();
    let client = ClientSender::new(tx);

    let write_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender.send(msg).await.is_err() {
                println!("WS SEND FAILED (client likely disconnected)");
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
                let camera_stream_id = value["camera_stream_id"].as_str().map(|s| s.to_string());
                let audio_muted = value["audio_muted"].as_bool();
                let video_muted = value["video_muted"].as_bool();

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
                    Ok(Some(_)) => println!("ROOM FOUND in DB"),
                    Ok(None) => println!("ROOM NOT FOUND in DB"),
                    Err(e) => println!("💥 SQL ERROR: {:?}", e),
                }

                let Ok(Some(room)) = room else {
                    let _ = client.send(Message::Text(r#"{"type":"ROOM_NOT_FOUND"}"#.into()));
                    return;
                };

                let is_open: bool = room.get("is_open");
                let host_id: Option<String> = room.get("created_by");

                println!("ROOM DATA: is_open={:?}, created_by={:?}", is_open, host_id);

                let is_host = host_id.as_deref() == Some(&uid);

                println!("👤 is_host = {}", is_host);

                // Check if user is approved to join
                let is_approved = if is_host {
                    false
                } else {
                    let rooms = state.rooms.read().await;
                    rooms
                        .get(&rid)
                        .map(|r| r.approved_users.contains(&uid))
                        .unwrap_or(false)
                };
                println!("DEBUG: is_approved = {}", is_approved);

                if is_open || is_host || is_approved {
                    println!(
                        "ALLOWING JOIN (open={}, host={}, approved={})",
                        is_open,
                        is_host,
                        is_approved
                    );

                    handle_join(
                        &state,
                        &rid,
                        &uid,
                        &name,
                        client.clone(),
                        session_id.as_ref().unwrap(),
                        host_id.clone(),
                        camera_stream_id,
                        audio_muted, // NEW
                        video_muted
                    ).await;
                } else {
                    let request_id = uuid::Uuid::new_v4().to_string();

                    println!("ROOM CLOSED - Creating join request: {}", request_id);

                    // Store in memory only - simplified with tx in JoinRequest
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
                                pending_requests: HashMap::new(),
                                approved_users: std::collections::HashSet::new(),
                                presenter_stream_id: None,
                            });

                        // Add to pending requests with tx included
                        room_entry.pending_requests.insert(
                            request_id.clone(),
                            crate::socket::room_manager::JoinRequest {
                                id: request_id.clone(),
                                user_id: uid.clone(),
                                name: name.clone(),
                                tx: client.clone(),
                            }
                        );

                        room_entry.host_id = host_id.clone();

                        println!(
                            "Stored pending request {} for user {} in room state",
                            request_id,
                            uid
                        );
                    }

                    // Notify user they're pending
                    let pending_msg =
                        serde_json::json!({
                        "type": "JOIN_PENDING",
                        "request_id": &request_id
                    })
                            .to_string()
                            .into();

                    if let Err(e) = client.send(Message::Text(pending_msg)) {
                        println!("Failed to send JOIN_PENDING to user: {:?}", e);
                    } else {
                        println!("Sent JOIN_PENDING to user {}", uid);
                    }

                    // Notify ONLY host if they're already in the room
                    {
                        let rooms = state.rooms.read().await;
                        if let Some(room_state) = rooms.get(&rid) {
                            if let Some(host_id_val) = &room_state.host_id {
                                // Find all senders belonging to the host only
                                let host_senders: Vec<_> = room_state.sessions
                                    .iter()
                                    .filter(|(_, owner_uid)| *owner_uid == host_id_val)
                                    .filter_map(|(sid, _)| room_state.senders.get(sid).clone())
                                    .collect();

                                println!(
                                    "Notifying {} host senders about new join request",
                                    host_senders.len()
                                );

                                for sender in host_senders {
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
                                        println!("Failed to notify host: {:?}", e);
                                    } else {
                                        println!("Notified host about pending request");
                                    }
                                }
                            } else {
                                println!("Host not in room yet, will send when they join");
                            }
                        } else {
                            println!("Host not in room yet, will send when they join");
                        }
                    }
                }
            }
            "JOIN_APPROVE" => {
                let request_id = value["request_id"].as_str().unwrap_or("");

                println!("JOIN_APPROVE received for request: {}", request_id);

                // VERIFY REQUESTER IS HOST
                {
                    let rooms = state.rooms.read().await;

                    // Find which room contains this request
                    let request_room = rooms
                        .iter()
                        .find(|(_, r)| r.pending_requests.contains_key(request_id));

                    match request_room {
                        Some((_, room_data)) => {
                            let is_requester_host =
                                room_data.host_id.as_deref() == user_id.as_deref();

                            if !is_requester_host {
                                println!(
                                    "UNAUTHORIZED: User {} attempted to approve but is not host (host: {:?})",
                                    user_id.as_deref().unwrap_or("unknown"),
                                    room_data.host_id
                                );
                                let _ = client.send(
                                    Message::Text(
                                        serde_json::json!({
                                            "type": "APPROVE_FAILED",
                                            "reason": "unauthorized",
                                            "message": "Only the host can approve join requests"
                                        })
                                            .to_string()
                                            .into()
                                    )
                                );
                                continue;
                            }
                        }
                        None => {
                            println!("Request {} not found in memory", request_id);
                            let _ = client.send(
                                Message::Text(
                                    serde_json::json!({
                                        "type": "APPROVE_FAILED",
                                        "reason": "request_not_found",
                                        "message": "Join request not found"
                                    })
                                        .to_string()
                                        .into()
                                )
                            );
                            continue;
                        }
                    }
                }

                // Get request from memory
                let (r_room_id, r_user_id, _, user_tx) = {
                    let mut rooms = state.rooms.write().await;

                    // Find the request
                    let mut found = None;
                    let mut room_id_found = None;

                    for (rid, room) in rooms.iter_mut() {
                        if let Some(req) = room.pending_requests.get(request_id) {
                            found = Some((req.user_id.clone(), req.name.clone(), req.tx.clone()));
                            room_id_found = Some(rid.clone());
                            break;
                        }
                    }

                    if
                        let (Some((req_user_id, req_name, req_tx)), Some(found_room_id)) = (
                            found,
                            room_id_found,
                        )
                    {
                        // Remove from pending requests
                        if let Some(room) = rooms.get_mut(&found_room_id) {
                            room.pending_requests.remove(request_id);
                            (found_room_id, req_user_id, req_name, Some(req_tx))
                        } else {
                            println!("Room {} disappeared", found_room_id);
                            let _ = client.send(
                                Message::Text(
                                    r#"{"type":"APPROVE_FAILED","reason":"room_not_found"}"#.into()
                                )
                            );
                            continue;
                        }
                    } else {
                        println!("Request {} not found in memory", request_id);
                        let _ = client.send(
                            Message::Text(
                                r#"{"type":"APPROVE_FAILED","reason":"request_not_found"}"#.into()
                            )
                        );
                        continue;
                    }
                };

                println!(
                    "Host {} approved user {} to join room {}",
                    user_id.as_deref().unwrap_or("unknown"),
                    r_user_id,
                    r_room_id
                );

                if let Some(user_tx) = user_tx {
                    // Send approval message
                    let approval_msg = Message::Text(
                        serde_json::json!({
                "type": "JOIN_APPROVED",
                "request_id": request_id,
                "user_id": &r_user_id,
                "message": "Your join request was approved! Please reconnect to join the room."
            })
                            .to_string()
                            .into()
                    );

                    match user_tx.send(approval_msg) {
                        Ok(_) => println!("Sent JOIN_APPROVED to user {}", r_user_id),
                        Err(e) =>
                            println!("Failed to send approval to user {}: {:?}", r_user_id, e),
                    }

                    // Mark user as approved for their reconnection
                    {
                        let mut rooms = state.rooms.write().await;
                        if let Some(room) = rooms.get_mut(&r_room_id) {
                            room.approved_users.insert(r_user_id.clone());
                            println!("Marked user {} as approved, waiting for reconnect", r_user_id);
                        }
                    }
                } else {
                    println!("Failed to get user tx!");
                }
            }
            "JOIN_REJECT" => {
                let request_id = value["request_id"].as_str().unwrap_or("");

                println!("JOIN_REJECT received for request: {}", request_id);

                // VERIFY REQUESTER IS HOST
                {
                    let rooms = state.rooms.read().await;

                    // Find which room contains this request
                    let request_room = rooms
                        .iter()
                        .find(|(_, r)| r.pending_requests.contains_key(request_id));

                    match request_room {
                        Some((_, room_data)) => {
                            let is_requester_host =
                                room_data.host_id.as_deref() == user_id.as_deref();

                            if !is_requester_host {
                                println!(
                                    "UNAUTHORIZED: User {} attempted to reject but is not host (host: {:?})",
                                    user_id.as_deref().unwrap_or("unknown"),
                                    room_data.host_id
                                );
                                let _ = client.send(
                                    Message::Text(
                                        serde_json::json!({
                                            "type": "REJECT_FAILED",
                                            "reason": "unauthorized",
                                            "message": "Only the host can reject join requests"
                                        })
                                            .to_string()
                                            .into()
                                    )
                                );
                                continue;
                            }
                        }
                        None => {
                            println!("Request {} not found in memory", request_id);
                            let _ = client.send(
                                Message::Text(
                                    serde_json::json!({
                                        "type": "REJECT_FAILED",
                                        "reason": "request_not_found",
                                        "message": "Join request not found"
                                    })
                                        .to_string()
                                        .into()
                                )
                            );
                            continue;
                        }
                    }
                }

                // Get request from memory
                let (room_id_reject, user_id_reject, user_tx) = {
                    let mut rooms = state.rooms.write().await;

                    let mut found = None;
                    let mut room_id_found = None;

                    for (rid, room) in rooms.iter_mut() {
                        if let Some(req) = room.pending_requests.get(request_id) {
                            found = Some((req.user_id.clone(), req.tx.clone()));
                            room_id_found = Some(rid.clone());
                            break;
                        }
                    }

                    if
                        let (Some((req_user_id, req_tx)), Some(found_room_id)) = (
                            found,
                            room_id_found,
                        )
                    {
                        if let Some(room) = rooms.get_mut(&found_room_id) {
                            // Remove from pending requests
                            room.pending_requests.remove(request_id);
                            (found_room_id, req_user_id, Some(req_tx))
                        } else {
                            println!("Room disappeared");
                            let _ = client.send(
                                Message::Text(
                                    r#"{"type":"REJECT_FAILED","reason":"room_disappeared"}"#.into()
                                )
                            );
                            continue;
                        }
                    } else {
                        println!("Request {} not found in memory", request_id);
                        let _ = client.send(
                            Message::Text(
                                r#"{"type":"REJECT_FAILED","reason":"request_not_found"}"#.into()
                            )
                        );
                        continue;
                    }
                };

                println!(
                    "Host {} rejected user {} from room {}",
                    user_id.as_deref().unwrap_or("unknown"),
                    user_id_reject,
                    room_id_reject
                );

                if let Some(user_tx) = user_tx {
                    let rejection_msg = Message::Text(
                        serde_json::json!({
                    "type": "JOIN_REJECTED",
                    "request_id": request_id,
                    "user_id": &user_id_reject,
                    "reason": "Your join request was rejected by the host"
                })
                            .to_string()
                            .into()
                    );

                    match user_tx.send(rejection_msg) {
                        Ok(_) => println!("Sent JOIN_REJECTED to user {}", user_id_reject),
                        Err(e) =>
                            println!(
                                "Failed to send rejection to user {}: {:?}",
                                user_id_reject,
                                e
                            ),
                    }
                } else {
                    println!("Failed to get user tx!");
                }
            }

            "PING" => {
                if let (Some(rid), Some(uid)) = (&room_id, &user_id) {
                    let mut rooms = state.rooms.write().await;

                    if let Some(room) = rooms.get_mut(rid) {
                        if let Some(participant) = room.participants.get_mut(uid) {
                            participant.last_seen = chrono::Utc::now().timestamp() as u64;
                        }
                    }
                }
                let _ = client.send(Message::Text(r#"{"type":"PONG"}"#.to_string().into()));
            }

            "SCREEN_SHARE_START" => {
                if let (Some(rid), Some(uid)) = (&room_id, &user_id) {
                    let stream_id = value.get("stream_id").and_then(|v| v.as_str());
                    let camera_id = value.get("camera_id").and_then(|v| v.as_str());
                    handle_screen_share(&state, rid, uid, true, stream_id, camera_id).await;
                }
            }

            "SCREEN_SHARE_STOP" => {
                if let (Some(rid), Some(uid)) = (&room_id, &user_id) {
                    handle_screen_share(&state, rid, uid, false, None, None).await;
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

            "LEAVE" => {
                // Manual leave call
                if let (Some(rid), Some(uid), Some(session_id)) = (&room_id, &user_id, &session_id) {
                    println!("👤 Manual Leave requested for user {}", uid);
                    handle_leave(&state, rid, uid, name.clone(), &session_id).await;
                    // Clear tracking variables so cleanup at bottom won't double execute
                    room_id = None;
                    user_id = None;
                }
                break; // Break the socket loop
            }

            _ => (),
        }
    }

    println!("🔌 SOCKET CLOSED - Cleaning up user");

    if let Some(rid) = &room_id {
        if let Some(uid) = &user_id {
            // Remove from pending requests in memory
            {
                let mut rooms = state.rooms.write().await;
                if let Some(room) = rooms.get_mut(rid) {
                    // Remove from pending_requests
                    let to_remove: Vec<_> = room.pending_requests
                        .iter()
                        .filter(|(_, req)| req.user_id == *uid)
                        .map(|(id, _)| id.clone())
                        .collect();

                    for req_id in to_remove {
                        room.pending_requests.remove(&req_id);
                        println!("Removed pending request {} from memory", req_id);
                    }
                }
            }
        }
    }

    if let (Some(rid), Some(uid), Some(sid)) = (room_id, user_id, session_id) {
        println!("🔌 SOCKET DISCONNECTED - Cleaning up user {} from room {}", uid, rid);
        handle_leave(&state, &rid, &uid, name.clone(), &sid).await;
    }
    write_task.abort();
}

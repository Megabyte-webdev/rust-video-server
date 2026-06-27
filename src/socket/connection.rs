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
                        &session_id.clone().unwrap()
                    ).await;
                } else {
                    let request_id = uuid::Uuid::new_v4().to_string();

                    let _ = sqlx
                        ::query(
                            r#"
            INSERT INTO join_requests (id, room_id, user_id, name)
            VALUES ($1, $2, $3, $4)
            "#
                        )
                        .bind(&request_id)
                        .bind(room_id.as_deref().unwrap_or("")) // Pass &str here
                        .bind(user_id.as_deref().unwrap_or("")) // Pass &str here
                        .bind(&name)
                        .execute(&state.db).await;

                    // notify user
                    let _ = tx.send(
                        Message::Text(
                            serde_json::json!({
                "type": "JOIN_PENDING",
                "request_id": &request_id
            })
                                .to_string()
                                .into()
                        )
                    );

                    // notify host safely (via room state)
                    if let Some(rid) = &room_id {
                        let rooms = state.rooms.read().await;

                        if let Some(room_state) = rooms.get(rid) {
                            for sender in room_state.senders.values() {
                                // Your existing logic
                                let _ = sender.send(
                                    Message::Text(
                                        serde_json::json!({
                        "type": "JOIN_REQUEST",
                        "request": {
                            "id": &request_id,
                            "user_id": user_id.as_deref().unwrap_or(""),
                            "name": &name
                        }
                    })
                                            .to_string()
                                            .into()
                                    )
                                );
                            }
                        }
                    }

                    state.rooms
                        .write().await
                        .get_mut(&rid)
                        .map(|room| room.pending_users.insert(uid.clone(), tx.clone()));
                }
            }
            "JOIN_APPROVE" => {
                let request_id = value["request_id"].as_str().unwrap_or("");

                let req = sqlx
                    ::query(r#"SELECT room_id, user_id, name FROM join_requests WHERE id = $1"#)
                    .fetch_optional(&state.db).await;

                if let Ok(Some(r)) = req {
                    let r_room_id: String = r.get("room_id");
                    let r_user_id: String = r.get("user_id");
                    let r_name: String = r.get("name");

                    // Get the APPROVED USER's tx channel
                    let user_tx = state.rooms
                        .read().await
                        .get(&r_room_id)
                        .and_then(|room| room.pending_users.get(&r_user_id))
                        .cloned();

                    if let Some(user_tx) = user_tx {
                        // Send approval message
                        let _ = user_tx.send(
                            Message::Text(
                                serde_json::json!({
                "type": "JOIN_APPROVED",
                "request_id": request_id,
                "user_id": &r_user_id, 
                "message": "Your join request was approved!"
            })
                                    .to_string()
                                    .into()
                            )
                        );

                        // Update DB
                        let _ = sqlx
                            ::query(r#"UPDATE join_requests SET status = 'approved' WHERE id = $1"#)
                            .bind(request_id)
                            .execute(&state.db).await;

                        handle_join(
                            &state,
                            &r_room_id,
                            &r_user_id,
                            &r_name,
                            user_tx,
                            &uuid::Uuid::new_v4().to_string()
                        ).await;

                        // Clean up pending user
                        state.rooms
                            .write().await
                            .get_mut(&r_room_id)
                            .map(|room| room.pending_users.remove(&r_user_id));
                    }
                }
            }
            "JOIN_REJECT" => {
                let request_id = value["request_id"].as_str().unwrap_or("");

                let req = sqlx
                    ::query(r#"SELECT room_id, user_id FROM join_requests WHERE id = $1"#)
                    .bind(request_id)
                    .fetch_optional(&state.db).await;

                let Ok(Some(req)) = req else {
                    let _ = tx.send(
                        Message::Text(
                            r#"{"type":"JOIN_REJECT_FAILED","reason":"request_not_found"}"#.into()
                        )
                    );
                    return;
                };

                let room_id: String = req.get("room_id");
                let user_id: String = req.get("user_id");

                // mark rejected in DB
                let _ = sqlx
                    ::query(r#"UPDATE join_requests SET status = 'rejected' WHERE id = $1"#)
                    .bind(request_id)
                    .execute(&state.db).await;

                //  Notify target user
                let rooms = state.rooms.read().await;

                if let Some(room) = rooms.get(&room_id) {
                    //  FIX 1: Check pending_users first (where pending users actually are)
                    if let Some(user_tx) = room.pending_users.get(&user_id) {
                        let _ = user_tx.send(
                            Message::Text(
                                serde_json::json!({
                    "type": "JOIN_REJECTED",
                    "request_id": request_id,
                    "user_id": &user_id, 
                    "reason": "Your join request was rejected"
                })
                                    .to_string()
                                    .into()
                            )
                        );
                    }
                }

                state.rooms
                    .write().await
                    .get_mut(&room_id)
                    .map(|room| room.pending_users.remove(&user_id));
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

    println!("SOCKET CLOSED");

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

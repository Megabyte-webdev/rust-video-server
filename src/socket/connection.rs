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

    println!("NEW SOCKET CONNECTION");

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

                room_id = Some(rid.clone());
                user_id = Some(uid.clone());
                session_id = Some(uuid::Uuid::new_v4().to_string());

                let session = session_id.clone().unwrap();

                // fetch room
                let room = sqlx
                    ::query(r#"SELECT is_open, host_id FROM rooms WHERE id = $1"#)
                    .bind(&rid)
                    .fetch_optional(&state.db).await;

                let Ok(Some(room)) = room else {
                    let _ = tx.send(Message::Text(r#"{"type":"ROOM_NOT_FOUND"}"#.into()));
                    return;
                };

                let is_open: bool = room.get("is_open");
                let host_id: Option<String> = room.get("host_id");

                let is_host = host_id.as_deref() == Some(&uid);

                if is_open || is_host {
                    handle_join(&state, &rid, &uid, &name, tx.clone(), &session).await;
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
                    let rooms = state.rooms.read().await;

                    // Safely unwrap the Option<String> before using it as a map key
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
                }
            }
            "JOIN_APPROVE" => {
                let request_id = value["request_id"].as_str().unwrap_or("");

                let req = sqlx
                    ::query(r#"SELECT room_id, user_id, name FROM join_requests WHERE id = $1"#)
                    .bind(request_id)
                    .fetch_optional(&state.db).await;

                if let Ok(Some(r)) = req {
                    // mark approved
                    let _ = sqlx
                        ::query(r#"UPDATE join_requests SET status = 'approved' WHERE id = $1"#)
                        .bind(request_id)
                        .execute(&state.db).await;
                    let r_room_id: String = r.get("room_id");
                    let r_user_id: String = r.get("user_id");
                    let r_name: String = r.get("name");

                    // NOW allow full join
                    handle_join(
                        &state,
                        &r_room_id,
                        &r_user_id,
                        &r_name,
                        tx.clone(),
                        &uuid::Uuid::new_v4().to_string()
                    ).await;
                }
            }
            "JOIN_REJECT" => {
                let request_id = value["request_id"].as_str().unwrap_or("");

                let _ = sqlx
                    ::query(r#"UPDATE join_requests SET status = 'rejected' WHERE id = $1"#)
                    .bind(request_id)
                    .execute(&state.db).await;

                let _ = tx.send(Message::Text(r#"{"type":"JOIN_REJECTED"}"#.into()));
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

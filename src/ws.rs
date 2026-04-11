use axum::{
    extract::{ State, ws::{ Message, WebSocket, WebSocketUpgrade } },
    response::IntoResponse,
};
use futures_util::SinkExt;
use futures_util::StreamExt;
use serde_json::json;
use std::collections::HashMap;

use crate::state::{ AppState, Room, RoomParticipant };

pub async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}
pub async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender_ws, mut receiver_ws) = socket.split();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Message>();

    let mut room_id: Option<String> = None;
    let mut user_id: Option<String> = None;

    // WRITE TASK
    tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let _ = sender_ws.send(msg).await;
        }
    });

    println!("🟢 NEW SOCKET CONNECTION");

    while let Some(msg) = receiver_ws.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                println!("❌ WS ERROR: {e}");
                break;
            }
        };

        let Message::Text(txt) = msg else {
            continue;
        };

        let Ok(value) = serde_json::from_str::<serde_json::Value>(&txt) else {
            continue;
        };

        let msg_type = value["type"].as_str().unwrap_or("");

        match msg_type {
            "JOIN" => {
                let rid = value["room_id"].as_str().unwrap_or("").to_string();
                let uid = value["user_id"].as_str().unwrap_or("").to_string();
                let name = value["sender_name"].as_str().unwrap_or("Anonymous").to_string();

                room_id = Some(rid.clone());
                user_id = Some(uid.clone());

                println!("➡️ JOIN: {uid} -> {rid}");

                let _ = sqlx
                    ::query(
                        "INSERT INTO participants (id, room_id, name)
         VALUES ($1,$2,$3)
         ON CONFLICT (id, room_id)
         DO UPDATE SET left_at = NULL"
                    )
                    .bind(&uid)
                    .bind(&rid)
                    .bind(&name)
                    .execute(&state.db).await;

                let mut rooms = state.rooms.write().await;

                let room = rooms.entry(rid.clone()).or_insert(Room {
                    participants: HashMap::new(),
                    senders: HashMap::new(),
                });

                // existing users
                let existing: Vec<_> = room.participants
                    .values()
                    .map(|p| { json!({
            "id": p.id,
            "name": p.name
        }) })
                    .collect();

                let _ = tx.send(
                    Message::Text(
                        json!({
            "type": "EXISTING_USERS",
            "participants": existing
        })
                            .to_string()
                            .into()
                    )
                );

                // register user
                room.participants.insert(uid.clone(), RoomParticipant {
                    id: uid.clone(),
                    name: name.clone(),
                });

                room.senders.insert(uid.clone(), tx.clone());

                println!("✅ ROOM SIZE: {}", room.participants.len());

                // broadcast to others ONLY
                let join_msg = Message::Text(
                    json!({
            "type": "USER_JOINED",
            "participant": {
                "id": uid,
                "name": name
            }
        })
                        .to_string()
                        .into()
                );

                for (pid, sender) in room.senders.iter() {
                    // Correct check: Skip the person who just joined
                    if pid != &uid {
                        let _ = sender.send(join_msg.clone());
                        println!("📡 Notified {} about {}", pid, uid);
                    }
                }
            }

            "SCREEN_SHARE_START" | "SCREEN_SHARE_STOP" => {
                if let (Some(rid), Some(uid)) = (&room_id, &user_id) {
                    let rooms = state.rooms.read().await;

                    if let Some(room) = rooms.get(rid) {
                        let outbound = Message::Text(txt.to_string().into());

                        for sender in room.senders.values() {
                            let _ = sender.send(outbound.clone());
                        }
                    }
                }
            }
            _ => {
                if let (Some(rid), Some(uid)) = (&room_id, &user_id) {
                    let rooms = state.rooms.read().await;

                    if let Some(room) = rooms.get(rid) {
                        let target = value.get("target").and_then(|v| v.as_str());

                        let outbound = Message::Text(txt.to_string().into());

                        match target {
                            Some(tid) => {
                                if let Some(sender) = room.senders.get(tid) {
                                    let _ = sender.send(outbound);
                                }
                            }
                            None => {
                                for sender in room.senders.values() {
                                    let _ = sender.send(outbound.clone());
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    println!("🔴 SOCKET CLOSED");

    if let (Some(rid), Some(uid)) = (room_id, user_id) {
        let mut rooms = state.rooms.write().await;

        if let Some(room) = rooms.get_mut(&rid) {
            room.participants.remove(&uid);
            room.senders.remove(&uid);

            let left_msg = Message::Text(
                json!({
                "type": "USER_LEFT",
                "peerId": uid
            })
                    .to_string()
                    .into()
            );

            for sender in room.senders.values() {
                let _ = sender.send(left_msg.clone());
            }
        }

        let _ = sqlx
            ::query(
                "UPDATE participants SET left_at = NOW()
         WHERE id = $1 AND room_id = $2"
            )
            .bind(&uid)
            .bind(&rid)
            .execute(&state.db).await;
    }
}

use axum::extract::ws::{ Message, WebSocket };
use futures_util::{ SinkExt, StreamExt };
use tokio::sync::mpsc::unbounded_channel;

use crate::{
    socket::handlers::{
        join::handle_join,
        leave::handle_leave,
        message::handle_message,
        screen_share::handle_screen_share,
    },
    state::AppState,
    utils::error::log_error,
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

        let msg_type = value["type"].as_str().unwrap_or("");

        match msg_type {
            "JOIN" => {
                let rid = value["room_id"].as_str().unwrap_or("").to_string();
                let uid = value["user_id"].as_str().unwrap_or("").to_string();
                name = value["sender_name"].as_str().unwrap_or("Anonymous").to_string();

                room_id = Some(rid.clone());
                user_id = Some(uid.clone());
                session_id = Some(uuid::Uuid::new_v4().to_string());

                handle_join(
                    &state,
                    &rid,
                    &uid,
                    &name,
                    tx.clone(),
                    session_id.as_ref().unwrap()
                ).await;
            }

            "PING" => {
                if let (Some(rid), Some(uid), Some(rsid)) = (&room_id, &user_id, &session_id) {
                    log_error(
                        sqlx
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
                            .execute(&state.db).await,
                        "PARTICIPANT_SESSION_UPDATE_FAILED"
                    );
                }
            }
            "SCREEN_SHARE_START" => {
                if let (Some(rid), Some(uid)) = (&room_id, &user_id) {
                    handle_screen_share(&state, rid, uid, true).await;
                }
            }

            "SCREEN_SHARE_STOP" => {
                if let (Some(rid), Some(uid)) = (&room_id, &user_id) {
                    handle_screen_share(&state, rid, uid, false).await;
                }
            }
            "Secure_Chat" => {
                if let (Some(rid), Some(uid)) = (&room_id, &user_id) {
                    handle_message(&state, rid, uid, &name, value).await;
                }
            }
            _ => (),
        }
    }

    println!("🔴 SOCKET CLOSED");

    if
        let (Some(rid), Some(uid), Some(sid)) = (
            room_id.clone(),
            user_id.clone(),
            session_id.clone(),
        )
    {
        println!("🧹 CLEANING UP USER SESSION");

        handle_leave(&state, &rid, &uid, name.clone(), &sid).await;

        println!("✅ CLEANUP COMPLETE");
    }
}

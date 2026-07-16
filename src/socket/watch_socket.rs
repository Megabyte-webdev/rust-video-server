use axum::extract::ws::{ Message, WebSocket };
use futures_util::{ SinkExt, StreamExt };
use tokio::sync::mpsc;

use crate::{
    socket::{
        handlers::room_feed::{
            build_room_presence,
            register_room_watcher,
            unregister_room_watcher,
        },
        room_manager::ClientSender,
    },
    state::AppState,
};

pub async fn watch_socket(state: AppState, room_id: String, user_id: String, socket: WebSocket) {
    let (mut ws_sender, mut ws_receiver) = socket.split();

    let (tx, mut rx) = mpsc::unbounded_channel::<Message>();

    let client = ClientSender::new(tx);

    register_room_watcher(&state, &room_id, &user_id, client.clone()).await;

    if let Some(payload) = build_room_presence(&state, &room_id, &user_id).await {
        let _ = client.send(payload);
    }

    let writer = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if ws_sender.send(msg).await.is_err() {
                break;
            }
        }
    });

    while let Some(result) = ws_receiver.next().await {
        match result {
            Ok(Message::Ping(data)) => {
                let _ = client.send(Message::Pong(data));
            }

            Ok(Message::Close(_)) => {
                break;
            }

            Err(_) => {
                break;
            }

            _ => {}
        }
    }

    unregister_room_watcher(&state, &room_id, &user_id).await;

    writer.abort();
}

use axum::extract::ws::Message;
use serde_json::json;
use uuid::Uuid;

use crate::state::AppState;

pub async fn handle_message(
    state: &AppState,
    room_id: &str,
    sender_id: &str,
    sender_name: &str,
    payload: serde_json::Value
) {
    let text = payload["message"].as_str().unwrap_or("").trim();
    if text.is_empty() {
        return;
    }

    let target_user = payload
        .get("target")
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let reply_to = payload.get("reply_to").cloned();

    let (user_senders, sender_tx) = {
        let rooms = state.rooms.read().await;
        let room = match rooms.get(room_id) {
            Some(r) => r,
            None => {
                return;
            }
        };

        (room.user_senders.clone(), room.user_senders.get(sender_id).cloned())
    };

    let message = Message::Text(
        json!({
            "type": "CHAT_MESSAGE",
            "data": {
                "id": uuid::Uuid::new_v4().to_string(),
                "sender_id": sender_id,
                "sender_name": sender_name,
                "message": text,
                "target": target_user,
                "reply_to": reply_to,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }
        })
            .to_string()
            .into()
    );

    // PRIVATE
    if let Some(target) = target_user {
        if let Some(tx) = user_senders.get(&target) {
            let _ = tx.send(message.clone());
        }

        if let Some(tx) = sender_tx {
            let _ = tx.send(message);
        }

        return;
    }

    // BROADCAST
    for tx in user_senders.values() {
        let _ = tx.send(message.clone());
    }
}

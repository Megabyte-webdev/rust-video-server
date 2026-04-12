use axum::extract::ws::Message;
use serde_json::json;

use crate::state::AppState;

/// Handle in-call chat messaging
pub async fn handle_message(
    state: &AppState,
    room_id: &str,
    sender_id: &str,
    sender_name: &str,
    payload: serde_json::Value
) {
    let text = payload["text"].as_str().unwrap_or("").trim().to_string();

    if text.is_empty() {
        return;
    }

    let rooms = state.rooms.read().await;

    let room = match rooms.get(room_id) {
        Some(r) => r,
        None => {
            return;
        }
    };

    let target = payload.get("target").and_then(|v| v.as_str());

    let message = Message::Text(
        json!({
            "type": "NEW_MESSAGE",
            "data": {
                "sender_id": sender_id,
                "sender_name": sender_name,
                "text": text,
                "target": target,
                "timestamp": chrono::Utc::now().to_rfc3339()
            }
        })
            .to_string()
            .into()
    );

    // -----------------------------
    // PRIVATE MESSAGE
    // -----------------------------
    if let Some(target_id) = target {
        if let Some(tx) = room.senders.get(target_id) {
            let _ = tx.send(message);
        }
        return;
    }

    // -----------------------------
    // BROADCAST MESSAGE
    // -----------------------------
    for tx in room.senders.values() {
        let _ = tx.send(message.clone());
    }
}

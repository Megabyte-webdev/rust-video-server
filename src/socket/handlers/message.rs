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
    let text = payload["message"].as_str().unwrap_or("").trim().to_string();

    if text.is_empty() {
        return;
    }

    let target = payload.get("target").and_then(|v| v.as_str());
    let reply_to = payload
        .get("reply_to")
        .and_then(|r| {
            Some(
                json!({
            "id": r.get("id")?.as_str()?,
            "name": r.get("name")?.as_str()?
        })
            )
        });

    // Clone senders OUTSIDE lock (important)
    let senders = {
        let rooms = state.rooms.read().await;

        match rooms.get(room_id) {
            Some(room) => room.senders.clone(),
            None => {
                return;
            }
        }
    };

    let message_payload =
        json!({
        "type": "CHAT_MESSAGE",
        "data": {
            "id": Uuid::new_v4().to_string(),
            "sender_id": sender_id,
            "sender_name": sender_name,
            "message": text,
            "target": target,
            "reply_to": reply_to,  
            "timestamp": chrono::Utc::now().to_rfc3339()
        }
    });

    let msg = Message::Text(message_payload.to_string().into());

    // ---------------- PRIVATE ----------------
    if let Some(target_id) = target {
        if let Some(tx) = senders.get(target_id) {
            let _ = tx.send(msg.clone());
        }

        // echo back to sender
        if let Some(tx) = senders.get(sender_id) {
            let _ = tx.send(msg);
        }

        return;
    }

    // ---------------- BROADCAST ----------------
    for tx in senders.values() {
        let _ = tx.send(msg.clone());
    }
}

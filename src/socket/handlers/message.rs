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

    // ---------------- EXTRACT TARGET (user_id) ----------------
    let target = payload
        .get("target")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    let reply_to = payload.get("reply_to").cloned();

    let (senders, target_session, sender_session) = {
        let rooms = state.rooms.read().await;

        let room = match rooms.get(room_id) {
            Some(r) => r,
            None => {
                return;
            }
        };

        let senders = room.senders.clone();

        let target_session = target.as_ref().and_then(|uid| room.sessions.get(uid).cloned());

        let sender_session = room.sessions.get(sender_id).cloned();

        (senders, target_session, sender_session)
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

    if let Some(target_session_id) = target_session {
        // send to target user session
        if let Some(tx) = senders.get(&target_session_id) {
            let _ = tx.send(msg.clone());
        }

        // echo back to sender
        if let Some(sender_session_id) = sender_session {
            if let Some(tx) = senders.get(&sender_session_id) {
                let _ = tx.send(msg);
            }
        }

        return;
    }

    for tx in senders.values() {
        let _ = tx.send(msg.clone());
    }
}

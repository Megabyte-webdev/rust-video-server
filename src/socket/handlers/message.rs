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
        .map(|s| s.trim().to_string());

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

        // 🔥 IMPORTANT FIX: resolve from PARTICIPANTS (not sessions)
        let target_session = target_user
            .as_ref()
            .and_then(|uid| room.participants.get(uid))
            .map(|p| p.session_id.clone());

        let sender_session = room.participants.get(sender_id).map(|p| p.session_id.clone());

        (senders, target_session, sender_session)
    };

    let msg = Message::Text(
        json!({
            "type": "CHAT_MESSAGE",
            "data": {
                "id": Uuid::new_v4().to_string(),
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
    if let Some(target_session_id) = target_session {
        if let Some(tx) = senders.get(&target_session_id) {
            let _ = tx.send(msg.clone());
        }

        if let Some(sender_session_id) = sender_session {
            if let Some(tx) = senders.get(&sender_session_id) {
                let _ = tx.send(msg);
            }
        }

        return;
    }

    // BROADCAST
    for tx in senders.values() {
        let _ = tx.send(msg.clone());
    }
}

use axum::extract::ws::Message;
use serde_json::Value;

use crate::state::AppState;

pub async fn handle_signaling(state: &AppState, room_id: &str, sender_id: &str, raw_msg: &str) {
    let Ok(value) = serde_json::from_str::<Value>(raw_msg) else {
        println!("❌ Invalid signaling JSON");
        return;
    };

    let msg_type = value
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let rooms = state.rooms.read().await;

    let Some(room) = rooms.get(room_id) else {
        println!("❌ Room not found");
        return;
    };

    let target_user_id = value.get("target").and_then(|v| v.as_str());

    match msg_type {
        "OFFER" | "ANSWER" | "ICE" => {
            let rooms = state.rooms.read().await;

            if let Some(room) = rooms.get(room_id) {
                let mut enriched = value.clone();
                enriched["sender"] = serde_json::json!(sender_id);

                let outbound = Message::Text(serde_json::to_string(&enriched).unwrap().into());

                if let Some(tid) = target_user_id {
                    let target_session = room.sessions
                        .iter()
                        .find(|(_, uid)| uid.as_str() == tid)
                        .map(|(sid, _)| sid.clone());

                    if let Some(session_id) = target_session {
                        if let Some(sender) = room.senders.get(&session_id) {
                            let _ = sender.send(outbound);
                        } else {
                            println!("❌ No sender for session: {}", session_id);
                        }
                    } else {
                        println!("❌ No session for user: {}", tid);
                    }
                } else {
                    for sender in room.senders.values() {
                        let _ = sender.send(outbound.clone());
                    }
                }
            }
        }
        _ => {
            println!("⚠️ Unknown signaling type: {}", msg_type);
        }
    }
}

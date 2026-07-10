use axum::extract::ws::Message;
use serde_json::json;
use crate::state::AppState;

/// Broadcast track state toggles (audio/video mute/unmute) to all room participants
pub async fn handle_media_state(
    state: &AppState,
    room_id: &str,
    user_id: &str,
    kind: &str,
    enabled: bool
) {
    if kind != "audio" && kind != "video" {
        return;
    }

    {
        let mut rooms = state.rooms.write().await;
        if let Some(room) = rooms.get_mut(room_id) {
            if let Some(participant) = room.participants.get_mut(user_id) {
                if kind == "audio" {
                    participant.mic_enabled = enabled;
                } else if kind == "video" {
                    participant.cam_enabled = enabled;
                }
            }
        }
    }

    let rooms = state.rooms.read().await;

    let Some(room) = rooms.get(room_id) else {
        println!("MEDIA_STATE: Room {} not found", room_id);
        return;
    };

    println!("MEDIA_STATE: Broadcasting to {} senders", room.senders.len());
    println!("   Senders in map: {:?}", room.senders.keys().collect::<Vec<_>>());

    let outbound = Message::Text(
        json!({
            "type": "MEDIA_STATE_CHANGE",
            "peerId": user_id,
            "sender": user_id,
            "kind": kind,
            "enabled": enabled
        })
            .to_string()
            .into()
    );

    for (session_id, sender) in room.senders.iter() {
        match sender.send(outbound.clone()) {
            Ok(_) => println!("Sent MEDIA_STATE to session {}", session_id),
            Err(e) => println!("Failed to send to {}: {:?}", session_id, e),
        }
    }
}

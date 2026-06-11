use axum::extract::ws::Message;
use serde_json::json;
use crate::state::AppState;

/// Broadcast track state toggles (audio/video mute/unmute) to all room participants
pub async fn handle_media_state(
    state: &AppState,
    room_id: &str,
    user_id: &str,
    kind: &str, // "audio" or "video"
    enabled: bool // true = unmuted/on, false = muted/off
) {
    // Prevent broadcasting empty or unvalidated variants
    if kind != "audio" && kind != "video" {
        return;
    }

    let rooms = state.rooms.read().await;

    let Some(room) = rooms.get(room_id) else {
        return;
    };

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

    for sender in room.senders.values() {
        let _ = sender.send(outbound.clone());
    }
}

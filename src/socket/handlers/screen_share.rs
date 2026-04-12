use axum::extract::ws::Message;
use serde_json::json;

use crate::state::AppState;

/// Broadcast screen share events to all participants in a room
pub async fn handle_screen_share(state: &AppState, room_id: &str, user_id: &str, is_start: bool) {
    let rooms = state.rooms.read().await;

    let Some(room) = rooms.get(room_id) else {
        return;
    };

    let msg_type = if is_start { "SCREEN_SHARE_START" } else { "SCREEN_SHARE_STOP" };

    let outbound = Message::Text(
        json!({
            "type": msg_type,
            "peerId": user_id
        })
            .to_string()
            .into()
    );

    for sender in room.senders.values() {
        let _ = sender.send(outbound.clone());
    }
}

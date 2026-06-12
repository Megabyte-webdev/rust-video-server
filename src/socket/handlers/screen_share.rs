use axum::extract::ws::Message;
use serde_json::json;

use crate::state::AppState;

/// Broadcast screen share events to all participants in a room
pub async fn handle_screen_share(
    state: &AppState,
    room_id: &str,
    user_id: &str,
    is_start: bool,
    stream_id: Option<&str>
) {
    let mut rooms = state.rooms.write().await;
    let Some(room) = rooms.get_mut(room_id) else {
        return;
    };

    // 1. Enforce Single Presenter
    if is_start {
        // If someone is already sharing, reject this attempt
        if room.presenter_id.is_some() && room.presenter_id != Some(user_id.to_string()) {
            return;
        }
        room.presenter_id = Some(user_id.to_string());
    } else {
        // Only allow the active presenter to stop their own share
        if room.presenter_id == Some(user_id.to_string()) {
            room.presenter_id = None;
        } else {
            return; // Ignore "stop" requests from non-presenters
        }
    }

    // 2. Broadcast the state change
    let msg_type = if is_start { "SCREEN_SHARE_START" } else { "SCREEN_SHARE_STOP" };
    let outbound = Message::Text(
        json!({
        "type": msg_type,
        "peerId": user_id,
        "stream_id": stream_id
    })
            .to_string()
            .into()
    );

    for sender in room.senders.values() {
        let _ = sender.send(outbound.clone());
    }
}

use crate::{ socket::handlers::room_feed::build_room_presence, state::AppState };

pub async fn broadcast_room_presence(state: &AppState, room_id: &str) {
    let watchers = {
        let rooms = state.rooms.read().await;

        let Some(room) = rooms.get(room_id) else {
            return;
        };

        room.watchers
            .iter()
            .map(|(_, sender)| sender.clone())
            .collect::<Vec<_>>()
    };

    let Some(payload) = build_room_presence(state, room_id, "").await else {
        return;
    };

    for watcher in watchers {
        let _ = watcher.send(payload.clone());
    }
}

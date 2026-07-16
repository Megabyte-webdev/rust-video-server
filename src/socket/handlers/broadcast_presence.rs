use crate::{ socket::handlers::room_feed::build_room_presence, state::AppState };

pub async fn broadcast_room_presence(state: &AppState, room_id: &str) {
    let watchers = {
        let watchers = state.watchers.read().await;

        let Some(room_watchers) = watchers.get(room_id) else {
            return;
        };

        room_watchers
            .iter()
            .map(|(user_id, sender)| (user_id.clone(), sender.clone()))
            .collect::<Vec<_>>()
    };

    for (user_id, watcher) in watchers {
        if let Some(payload) = build_room_presence(state, room_id, &user_id).await {
            let _ = watcher.send(payload);
        }
    }
}

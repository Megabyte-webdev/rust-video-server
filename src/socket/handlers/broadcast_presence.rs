use crate::{ socket::handlers::room_feed::build_room_presence, state::AppState };

pub async fn broadcast_room_presence(state: &AppState, room_id: &str) {
    let updates = {
        let rooms = state.rooms.read().await;

        let Some(room) = rooms.get(room_id) else {
            return;
        };

        room.watchers
            .iter()
            .map(|(user_id, sender)| {
                let payload = build_room_presence(room, room_id, user_id);

                (sender.clone(), payload)
            })
            .collect::<Vec<_>>()
    };

    for (sender, payload) in updates {
        if sender.send(payload).is_err() {
            // optional:
            // remove dead watcher later
        }
    }
}

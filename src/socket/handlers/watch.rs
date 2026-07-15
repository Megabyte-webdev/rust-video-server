use crate::{
    socket::{ handlers::room_feed::build_room_presence, room_manager::ClientSender },
    state::AppState,
};

pub async fn handle_watch_room(state: &AppState, room_id: &str, user_id: &str, tx: ClientSender) {
    let payload = {
        let mut rooms = state.rooms.write().await;

        let Some(room) = rooms.get_mut(room_id) else {
            return;
        };

        // subscribe
        room.watchers.insert(user_id.to_string(), tx.clone());

        build_room_presence(room, room_id, user_id)
    };

    // send current state immediately
    let _ = tx.send(payload);
}

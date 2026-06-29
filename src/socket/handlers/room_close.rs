use crate::state::AppState;

pub async fn handle_room_close(state: &AppState, room_id: &str) {
    println!("🔒 ROOM CLOSING: {}", room_id);

    // Clear memory
    let mut rooms = state.rooms.write().await;
    if let Some(room) = rooms.get_mut(room_id) {
        room.pending_requests.clear();
    }
    rooms.remove(room_id);

    println!("ROOM CLOSED - Cleanup complete");
}

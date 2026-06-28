use crate::state::AppState;

pub async fn handle_room_close(state: &AppState, room_id: &str) {
    println!("🔒 ROOM CLOSING: {}", room_id);

    // Delete ALL pending join requests for this room
    let _ = sqlx
        ::query(r#"DELETE FROM join_requests WHERE room_id = $1"#)
        .bind(room_id)
        .execute(&state.db).await;

    // Clear memory
    let mut rooms = state.rooms.write().await;
    if let Some(room) = rooms.get_mut(room_id) {
        room.pending_users.clear();
    }
    rooms.remove(room_id);

    println!("✅ ROOM CLOSED - Cleanup complete");
}

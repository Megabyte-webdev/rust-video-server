use crate::{ socket::handlers::leave::handle_leave, state::AppState };

pub async fn cleanup_stale_sessions(state: &AppState) {
    let mut to_cleanup: Vec<(String, String, String, String)> = vec![]; // (room_id, user_id, session_id, name)

    {
        let mut rooms = state.rooms.write().await;
        let now = chrono::Utc::now().timestamp() as u64;

        for (room_id, room) in rooms.iter_mut() {
            let mut stale_sessions = vec![];

            // Find stale sessions in memory
            for (session_id, user_id) in &room.sessions {
                if let Some(participant) = room.participants.get(user_id) {
                    // If last_seen is older than 45 seconds, mark as stale
                    if now.saturating_sub(participant.last_seen) > 45 {
                        stale_sessions.push((
                            session_id.clone(),
                            user_id.clone(),
                            participant.name.clone(),
                        ));
                    }
                }
            }

            // Collect for cleanup outside the lock
            for (session_id, user_id, name) in stale_sessions {
                to_cleanup.push((room_id.clone(), user_id, session_id, name));
            }

            // Remove empty rooms
            if room.sessions.is_empty() {
                println!("📭 Room {} empty, will be removed", room_id);
            }
        }

        // Remove empty rooms
        rooms.retain(|_, room| !room.sessions.is_empty());
    }

    // Execute cleanup outside lock
    for (room_id, user_id, session_id, name) in to_cleanup {
        println!("🧹 Cleaning stale session for user {} in room {}", user_id, room_id);
        handle_leave(state, &room_id, &user_id, name, &session_id).await;
    }
}

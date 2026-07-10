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
                    let elapsed = now.saturating_sub(participant.last_seen);

                    if elapsed > 60 {
                        stale_sessions.push((
                            session_id.clone(),
                            user_id.clone(),
                            participant.name.clone(),
                        ));
                    } else if elapsed > 40 {
                        // Warn before removal
                        println!("⚠️ Session {} at risk ({}s)", session_id, elapsed);
                    }
                }
            }

            // Collect for cleanup outside the lock
            for (session_id, user_id, name) in stale_sessions {
                to_cleanup.push((room_id.clone(), user_id, session_id, name));
            }
        }

        // Remove empty rooms
        rooms.retain(|_, room| !room.sessions.is_empty());
    }

    // Execute cleanup outside lock
    for (room_id, user_id, session_id, name) in to_cleanup {
        println!("Removing stale session for {}", user_id);
        handle_leave(state, &room_id, &user_id, name, &session_id).await;
    }
}

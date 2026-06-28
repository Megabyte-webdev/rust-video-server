use axum::extract::ws::Message;
use serde_json::json;

use crate::{ services::attendance_service::AttendanceService, state::AppState };

pub async fn handle_leave(
    state: &AppState,
    room_id: &str,
    user_id: &str,
    name: String,
    session_id: &str
) {
    println!("🔥 HANDLE_LEAVE CALLED for user {}", user_id);

    let mut recipients = vec![];

    // ---------------- MEMORY LOCK ----------------
    {
        let mut rooms = state.rooms.write().await;

        if let Some(room) = rooms.get_mut(room_id) {
            recipients = room.senders
                .iter()
                .filter_map(|(sid, tx)| {
                    let owner = room.sessions.get(sid)?;
                    if sid == session_id || owner == user_id {
                        return None;
                    }
                    Some(tx.clone())
                })
                .collect();

            room.sessions.remove(session_id);
            room.senders.remove(session_id);

            let still_connected = room.sessions.values().any(|uid| uid == user_id);

            if !still_connected {
                room.participants.remove(user_id);
            }

            // ============ NEW: CLEANUP PENDING JOIN REQUEST ============
            // If user had a pending join request, remove from memory
            if room.pending_users.remove(user_id).is_some() {
                println!("🗑️ Removed pending user {} from room state", user_id);
            }
            // ============ END: CLEANUP ============

            if room.sessions.is_empty() {
                println!("📭 Room {} is now empty, removing from state", room_id);
                rooms.remove(room_id);
            }
        } else {
            println!("⚠️ Room {} not found in state", room_id);
            return;
        }
    }

    // ---------------- DB TRANSACTION ----------------
    let mut tx_db = match state.db.begin().await {
        Ok(t) => t,
        Err(_) => {
            println!("❌ Failed to start DB transaction");
            return;
        }
    };

    let _ = sqlx
        ::query(
            r#"
        INSERT INTO room_events
        (room_id, session_id, user_id, event_type, payload)
        VALUES ($1, $2, $3, 'LEAVE', $4)
        "#
        )
        .bind(room_id)
        .bind(session_id)
        .bind(user_id)
        .bind(json!({ "name": name }))
        .execute(&mut *tx_db).await;

    let _ = sqlx
        ::query("UPDATE room_sessions SET ended_at = NOW() WHERE id = $1")
        .bind(session_id)
        .execute(&mut *tx_db).await;

    let _ = sqlx
        ::query(
            r#"
        UPDATE participant_sessions
        SET left_at = NOW(), last_seen = NOW()
        WHERE id = $1
        "#
        )
        .bind(session_id)
        .execute(&mut *tx_db).await;

    if let Err(_) = AttendanceService::mark_leave(&state.db, room_id, user_id).await {
        println!("❌ Failed to mark attendance");
        return;
    }

    // ============ NEW: CLEANUP PENDING JOIN REQUEST FROM DB ============
    // Delete any pending join requests for this user in this room
    match
        sqlx
            ::query(
                r#"
        DELETE FROM join_requests 
        WHERE room_id = $1 AND user_id = $2 AND status = 'pending'
        "#
            )
            .bind(room_id)
            .bind(user_id)
            .execute(&mut *tx_db).await
    {
        Ok(result) => {
            if result.rows_affected() > 0 {
                println!(
                    "✅ Deleted {} pending join request(s) for user {}",
                    result.rows_affected(),
                    user_id
                );
            }
        }
        Err(e) => {
            println!("⚠️ Failed to delete pending join requests: {:?}", e);
        }
    }
    // ============ END: CLEANUP ============

    let _ = tx_db.commit().await;

    // ============ NEW: CLEANUP EXPIRED REQUESTS FOR THIS ROOM ============
    // While we're here, also clean up very old pending requests (older than 30 mins)
    // This prevents the list from accumulating stale requests
    match
        sqlx
            ::query(
                r#"
        DELETE FROM join_requests 
        WHERE room_id = $1 
        AND status = 'pending'
        AND created_at < datetime('now', '-30 minutes')
        "#
            )
            .bind(room_id)
            .execute(&state.db).await
    {
        Ok(result) => {
            if result.rows_affected() > 0 {
                println!(
                    "🧹 Cleaned up {} expired pending requests from room {}",
                    result.rows_affected(),
                    room_id
                );
            }
        }
        Err(e) => {
            println!("⚠️ Failed to clean up expired requests: {:?}", e);
        }
    }
    // ============ END: CLEANUP EXPIRED ============

    // ---------------- BROADCAST ----------------
    let leave_msg = Message::Text(
        json!({
        "type": "USER_LEFT",
        "participant": {
            "id": user_id,
            "name": name,
            "session_id": session_id
        }
    })
            .to_string()
            .into()
    );

    for tx in recipients {
        let _ = tx.send(leave_msg.clone());
    }

    println!("✅ LEAVE COMPLETE for user {}", user_id);
}

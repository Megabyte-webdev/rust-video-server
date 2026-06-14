use sqlx::Row;
use crate::{ socket::handlers::leave::handle_leave, state::AppState };

pub async fn cleanup_stale_sessions(state: &AppState) {
    let stale_sessions = sqlx
        ::query(
            r#"
        SELECT room_id, user_id,name, room_session_id
        FROM participant_sessions
        WHERE last_seen < NOW() - INTERVAL '45 seconds'
        "#
        )
        .fetch_all(&state.db).await
        .unwrap_or_default();

    for row in stale_sessions {
        let room_id: String = row.get("room_id");
        let user_id: String = row.get("user_id");
        let room_session_id: String = row.get("room_session_id");
        let name: String = row
            .get::<Option<String>, _>("name")
            .unwrap_or_else(|| "Anonymous".to_string());

        handle_leave(state, &room_id, &user_id, name, &room_session_id).await;
    }
}

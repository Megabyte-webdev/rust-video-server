use serde_json::json;

use crate::state::AppState;

pub async fn log_join(
    state: &AppState,
    room_id: &str,
    user_id: &str,
    name: &str,
    session_id: &str
) {
    // DB event
    let _ = sqlx
        ::query(
            r#"
        INSERT INTO room_events
        (room_id, session_id, user_id, event_type, payload)
        VALUES ($1, $2, $3, 'JOIN', $4)
        "#
        )
        .bind(room_id)
        .bind(session_id)
        .bind(user_id)
        .bind(json!({ "name": name }))
        .execute(&state.db).await;
}

pub async fn log_leave(
    state: &AppState,
    room_id: &str,
    user_id: &str,
    name: String,
    session_id: &str
) {
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
        .execute(&state.db).await;
}

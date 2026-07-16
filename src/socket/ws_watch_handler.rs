use axum::{ extract::{ ws::WebSocketUpgrade, Path, Query, State }, response::IntoResponse };

use crate::{ socket::watch_socket::watch_socket, state::AppState };

#[derive(serde::Deserialize)]
pub struct WatchQuery {
    pub user_id: Option<String>,
}

pub async fn handle_watch_socket(
    ws: WebSocketUpgrade,
    Path(room_id): Path<String>,
    Query(query): Query<WatchQuery>,
    State(state): State<AppState>
) -> impl IntoResponse {
    let user_id = query.user_id.unwrap_or_default();

    ws.on_upgrade(move |socket| { watch_socket(state, room_id, user_id, socket) })
}

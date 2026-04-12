use axum::{ extract::{ State, ws::WebSocketUpgrade }, response::IntoResponse };

use crate::state::AppState;
use super::connection::handle_socket;

pub async fn socket_response(
    ws: WebSocketUpgrade,
    State(state): State<AppState>
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

use axum::{ extract::State, http::StatusCode, response::IntoResponse, Json };
use crate::state::AppState;
use serde::{ Deserialize, Serialize };

#[derive(Deserialize)]
pub struct CreateRoomRequest {
    pub title: Option<String>,
    pub created_by: String,
}

#[derive(Serialize)]
pub struct CreateRoomResponse {
    pub id: String,
    pub title: Option<String>,
}

pub async fn create_room(
    State(state): State<AppState>,
    Json(req): Json<CreateRoomRequest>
) -> impl IntoResponse {
    // Generate a unique room ID (8 chars)
    let room_id = nanoid::nanoid!(8);

    // Insert into Postgres
    let res = sqlx
        ::query("INSERT INTO rooms (id, name) VALUES ($1, $2)")
        .bind(&room_id)
        .bind(&req.title)
        .execute(&state.db).await;

    match res {
        Ok(_) =>
            (
                StatusCode::CREATED,
                Json(CreateRoomResponse {
                    id: room_id,
                    title: req.title,
                }),
            ),
        Err(err) => {
            eprintln!("DB error creating room: {:?}", err);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(CreateRoomResponse {
                    id: "".into(),
                    title: None,
                }),
            )
        }
    }
}

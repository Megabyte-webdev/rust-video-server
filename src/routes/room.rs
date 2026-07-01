use axum::{ extract::{ Path, State }, http::StatusCode, Json };

use serde::{ Deserialize, Serialize };

use crate::{
    socket::handlers::room_close::handle_room_close,
    state::AppState,
    utils::helper::generate_room_id,
};

#[derive(Deserialize)]
pub struct CreateRoomRequest {
    pub title: Option<String>,
    pub created_by: String,
    pub is_open: Option<bool>,
}

#[derive(Serialize)]
pub struct CreateRoomResponse {
    pub id: String,
    pub title: Option<String>,
}

#[derive(sqlx::FromRow, Serialize)]
pub struct RoomData {
    pub room_id: String,
    pub created_by: String,
    pub name: String,
    pub is_open: bool,
}

#[derive(Serialize)]
pub struct ValidateResponse {
    pub valid: bool,
    pub data: Option<RoomData>,
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

pub async fn create_room(
    State(state): State<AppState>,
    Json(req): Json<CreateRoomRequest>
) -> Result<Json<CreateRoomResponse>, (StatusCode, Json<ErrorResponse>)> {
    let title = req.title.clone();
    let created_by = req.created_by.clone();
    let is_open = req.is_open.unwrap_or(true);

    if created_by.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "created_by cannot be empty".to_string(),
            }),
        ));
    }

    if let Some(ref t) = title {
        if t.trim().is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Title cannot be empty".to_string(),
                }),
            ));
        }
        if t.trim().len() > 50 || t.trim().len() < 3 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Title must be between 3 and 50 characters".to_string(),
                }),
            ));
        }
    }

    for _ in 0..5 {
        let room_id = generate_room_id();

        let result = sqlx
            ::query("INSERT INTO rooms (id, name, created_by, is_open) VALUES ($1, $2, $3, $4)")
            .bind(&room_id)
            .bind(&title)
            .bind(&req.created_by)
            .bind(&is_open)
            .execute(&state.db).await;

        match result {
            Ok(_) => {
                return Ok(
                    Json(CreateRoomResponse {
                        id: room_id,
                        title,
                    })
                );
            }

            Err(err) => {
                eprintln!("DB error creating room: {:?}", err);

                // retry only on duplicate key (23505)
                if let Some(db_err) = err.as_database_error() {
                    if db_err.code().as_deref() != Some("23505") {
                        return Err((
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(ErrorResponse {
                                error: "Database error while creating room".to_string(),
                            }),
                        ));
                    }
                }
            }
        }
    }

    Err((
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ErrorResponse {
            error: "Failed to generate unique room ID".to_string(),
        }),
    ))
}

pub async fn get_meeting(
    State(state): State<AppState>,
    Path(id): Path<String>
) -> (StatusCode, Json<ValidateResponse>) {
    let result = sqlx
        ::query_as::<_, RoomData>(
            r#"
        SELECT id as room_id, created_by, name, is_open
        FROM rooms
        WHERE id = $1
        "#
        )
        .bind(&id)
        .fetch_optional(&state.db).await;

    match result {
        Ok(Some(room)) =>
            (
                StatusCode::OK,
                Json(ValidateResponse {
                    valid: true,
                    data: Some(room),
                }),
            ),

        Ok(None) =>
            (
                StatusCode::OK,
                Json(ValidateResponse {
                    valid: false,
                    data: None,
                }),
            ),

        Err(err) => {
            eprintln!("get_meeting error: {:?}", err);

            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ValidateResponse {
                    valid: false,
                    data: None,
                }),
            )
        }
    }
}

pub async fn delete_room(state: AppState, Path(room_id): Path<String>) -> Result<String, String> {
    // Delete room from DB
    let _ = sqlx::query("DELETE FROM rooms WHERE id = $1").bind(&room_id).execute(&state.db).await;

    // Cleanup pending requests & memory
    handle_room_close(&state, &room_id).await; // ← HERE

    Ok("Room deleted".to_string())
}

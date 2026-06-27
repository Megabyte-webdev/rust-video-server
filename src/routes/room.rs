use axum::{ extract::{ Path, State }, http::StatusCode, Json };

use serde::{ Deserialize, Serialize };

use crate::{ state::AppState, utils::helper::generate_room_id };

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

#[derive(Serialize)]
pub struct RoomData {
    pub roomId: String,
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

    for _ in 0..5 {
        let room_id = generate_room_id();

        let result = sqlx
            ::query("INSERT INTO rooms (id, name, created_by) VALUES ($1, $2, $3)")
            .bind(&room_id)
            .bind(&title)
            .bind(&req.created_by)
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

pub async fn validate_meeting(
    State(state): State<AppState>,
    Path(id): Path<String>
) -> (StatusCode, Json<ValidateResponse>) {
    let result = sqlx
        ::query_scalar::<_, String>("SELECT id FROM rooms WHERE id = $1")
        .bind(&id)
        .fetch_optional(&state.db).await;

    match result {
        Ok(Some(room_id)) =>
            (
                StatusCode::OK,
                Json(ValidateResponse {
                    valid: true,
                    data: Some(RoomData { roomId: room_id }),
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
            eprintln!("validate_meeting error: {:?}", err);

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

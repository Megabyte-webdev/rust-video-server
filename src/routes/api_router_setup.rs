use axum::{ Router, routing::{ delete, get, post } };

use crate::{
    routes::{
        attendance_api::{
            get_attendance,
            get_live_participants,
            get_participant_detail,
            get_participants,
            get_room_sessions,
        },
        room::{ create_room, delete_room, get_meeting },
    },
    state::AppState,
};

/// Setup all room & attendance API routes
pub fn create_api_router() -> Router<AppState> {
    Router::new()
        .route("/rooms", post(create_room))
        .route("/rooms/{id}", get(get_meeting))
        .route("/rooms/{id}", delete(delete_room))

        // Attendance & participant data
        .route("/rooms/{id}/attendance", get(get_attendance))
        .route("/rooms/{id}/participants", get(get_participants))
        .route("/rooms/{id}/sessions", get(get_room_sessions))
        .route("/rooms/{id}/participants/{user_id}", get(get_participant_detail))

        //live meeting info
        .route("/rooms/{id}/live", get(get_live_participants))
}

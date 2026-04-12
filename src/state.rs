use sqlx::PgPool;
use crate::socket::room_manager::Rooms;

#[derive(Clone)]
pub struct AppState {
    pub rooms: Rooms,
    pub db: PgPool,
}

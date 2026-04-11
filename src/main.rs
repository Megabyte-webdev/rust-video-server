use axum::{ routing::{ get, post }, Router };
use tower_http::cors::{ CorsLayer, Any };
use sqlx::postgres::PgPoolOptions;
mod routes;
use crate::{ state::AppState, ws::ws_handler, routes::room::create_room };
//use redis::Commands;

mod state;
mod ws;
mod auth;
mod models;
mod signaling;

#[tokio::main]
async fn main() {
    let db_url =
        "postgresql://neondb_owner:npg_9AMVw6gyseCu@ep-mute-haze-ampf6jkv-pooler.c-5.us-east-1.aws.neon.tech/neondb?channel_binding=require&sslmode=require";
    let db_pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(db_url).await
        .expect("Failed to connect to Postgres");

    println!("Postgres connected");

    // let redis_url = "redis://127.0.0.1/";
    // let redis_client = redis::Client::open(redis_url).expect("Failed to connect to Redis");

    // Test connection
    //let mut conn = redis_client.get_connection().expect("Redis connection failed");
    //let pong: String = conn.ping().expect("Redis ping failed");
    // println!("Redis PING response: {}", pong);

    // println!(" Redis connected");

    let rooms = std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new()));

    let state = AppState {
        rooms,
        db: db_pool,
        //redis: redis_client,
    };

    let cors = CorsLayer::new()
        .allow_origin(Any) // allow all origins for testing
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/rooms", post(create_room))
        .with_state(state)
        .layer(cors);

    let addr = tokio::net::TcpListener
        ::bind("0.0.0.0:8080").await
        .expect("Failed to bind port 8080");
    println!("🚀 WebSocket server running on ws://{:?}", addr);
    axum::serve(addr, app).await.unwrap();
}

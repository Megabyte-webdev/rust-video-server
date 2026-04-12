use axum::{ routing::{ get, post }, Router };
use tower_http::cors::{ CorsLayer, Any };
use sqlx::postgres::PgPoolOptions;
mod routes;
use crate::{ state::AppState, ws::ws_handler, routes::room::create_room };
use std::time::Duration;
use tokio::time;
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

    if let Ok(url) = std::env::var("APP_URL") {
        tokio::spawn(start_keep_alive(url));
    }

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
        .allow_origin("https://videosdk.vercel.app".parse::<axum::http::HeaderValue>().unwrap())
        .allow_methods([axum::http::Method::GET, axum::http::Method::POST])
        .allow_headers([axum::http::HeaderName::from_static("content-type")])
        .allow_credentials(true);

    let app = Router::new()
        .route("/ws", get(ws_handler))
        .route("/rooms", post(create_room))
        .with_state(state)
        .layer(cors);

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());

    let addr_str = format!("0.0.0.0:{}", port);

    let listener = tokio::net::TcpListener
        ::bind(&addr_str).await
        .expect(&format!("Failed to bind to {}", addr_str));
    println!("🚀 WebSocket server running on ws://{:?}", listener);
    axum::serve(listener, app).await.unwrap();
}

async fn start_keep_alive(url: String) {
    let client = reqwest::Client::new();
    // Ping every 10 minutes (Render sleeps after 15)
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(600));

    loop {
        interval.tick().await;
        match client.get(&url).send().await {
            Ok(_) => println!("Status: Server is awake."),
            Err(e) => eprintln!("Ping failed: {}", e),
        }
    }
}

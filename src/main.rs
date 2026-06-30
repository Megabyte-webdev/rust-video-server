use axum::Router;
use axum::http::HeaderValue;
use axum::routing::{ get, post };
use dotenvy::dotenv;
use tower_http::cors::{ CorsLayer, Any };
use sqlx::postgres::PgPoolOptions;

mod routes;
mod socket;
mod state;
mod auth;
mod models;
mod utils;
mod services;

use crate::{
    routes::room::{ create_room, get_meeting },
    socket::handlers::cleanup::cleanup_stale_sessions,
    socket::ws_handler::socket_response,
    state::{ AppState, TurnConfig },
};

#[tokio::main]
async fn main() {
    dotenv().ok();

    // ============ LOAD DATABASE ============
    let db_url = std::env
        ::var("DATABASE_URL")
        .map_err(|_| "DATABASE_URL must be set")
        .expect("Failed to fetch DB URL");

    let db_pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&db_url).await
        .expect("Failed to connect to Postgres");

    println!("Postgres connected");

    // ============ LOAD TURN CONFIGURATION AT STARTUP ============
    let turn_config = TurnConfig::from_env().expect(
        "Failed to load TURN configuration. Please set TURN_SERVER and TURN_AUTH_SECRET environment variables."
    );

    println!("TURN Server configured: {}", turn_config.server);

    // ============ KEEP-ALIVE PING ============
    if let Ok(url) = std::env::var("APP_URL") {
        tokio::spawn(start_keep_alive(url));
    }

    // ============ INITIALIZE ROOMS STATE ============
    let rooms = std::sync::Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new()));

    // ============ CREATE APP STATE ============
    let state = AppState {
        rooms,
        db: db_pool,
        turn_config,
    };

    {
        let cleanup_state = state.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));

            loop {
                interval.tick().await;
                cleanup_stale_sessions(&cleanup_state).await;
            }
        });
    }

    // ============ CORS CONFIGURATION ============
    let cors = CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any);

    // ============ BUILD ROUTER ============
    let app = Router::new()
        .route("/ws", get(socket_response))
        .route("/rooms", post(create_room))
        .route("/room/{id}", get(get_meeting))
        .with_state(state)
        .layer(cors);

    // ============ START SERVER ============
    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let addr_str = format!("0.0.0.0:{}", port);

    let listener = tokio::net::TcpListener
        ::bind(&addr_str).await
        .expect(&format!("Failed to bind to {}", addr_str));

    println!("🚀 WebSocket server running on ws://{}", addr_str);

    axum::serve(listener, app).await.expect("Server error");
}

/// Periodically ping the application to prevent it from sleeping on free-tier hosting
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

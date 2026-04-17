use axum::{ Router, http::HeaderValue, routing::{ get, post } };
use dotenvy::dotenv;
use tower_http::cors::{ CorsLayer, Any };
use sqlx::postgres::PgPoolOptions;
mod routes;
mod socket;
mod state;
mod auth;
mod models;
mod utils;
use crate::{ routes::room::create_room, socket::ws_handler::socket_response, state::AppState };

//use redis::Commands;

#[tokio::main]
async fn main() {
    dotenv().ok();
    let db_url = std::env
        ::var("DATABASE_URL")
        .map_err(|_| "DATABASE_URL must be set")
        .expect("Failed to fetch DB URL");

    let db_pool = PgPoolOptions::new()
        .max_connections(10)
        .connect(&db_url).await
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

    // let cors = CorsLayer::new()
    //     .allow_origin(Any) // allow all origins for testing
    //     .allow_methods(Any)
    //     .allow_headers(Any);

    let cors = CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any);
    // .allow_credentials(true);

    let app = Router::new()
        .route("/ws", get(socket_response))
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

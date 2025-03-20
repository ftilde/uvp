use std::path::PathBuf;

use axum::{routing::get, Json, Router};
use uvp_state::data::{Active, Available, DateTime, Feed, Store};

use clap::Parser;

#[derive(Parser)]
struct CliArgs {
    /// Path to sqlite database which stores all state
    #[arg()]
    db: PathBuf,
}

#[tokio::main]
async fn main() {
    let args = CliArgs::parse();

    let db = uvp_state::data::Database::new(&args.db).unwrap();
    let db = &*Box::leak(Box::new(tokio::sync::Mutex::new(db)));

    // build our application with a single route
    let app = Router::new()
        .route(
            "/all_feeds",
            get(move || async move {
                let db = db.lock().await;
                let res = db.all_feeds().unwrap();
                Json(res)
            }),
        )
        .route(
            "/add_to_feed",
            get(move |feed: Json<Feed>| async move {
                let db = db.lock().await;
                let res = db.add_to_feed(&feed.0).unwrap();
                Json(res)
            }),
        )
        .route(
            "/remove_feed",
            get(move |url: Json<String>| async move {
                let db = db.lock().await;
                let res = db.remove_feed(&url.0).unwrap();
                Json(res)
            }),
        )
        .route(
            "/set_last_update",
            get(
                move |Json((url, update)): Json<(String, DateTime)>| async move {
                    let db = db.lock().await;
                    let res = db.set_last_update(&url, update).unwrap();
                    Json(res)
                },
            ),
        )
        .route(
            "/all_available",
            get(move || async move {
                let db = db.lock().await;
                let res = db.all_available().unwrap();
                Json(res)
            }),
        )
        .route(
            "/find_in_available",
            get(move |Json(url): Json<String>| async move {
                println!("jo");
                let db = db.lock().await;
                let res = db.find_in_available(&url).unwrap();
                Json(res)
            }),
        )
        .route(
            "/remove_from_available",
            get(move |Json(url): Json<String>| async move {
                let db = db.lock().await;
                let res = db.remove_from_available(&url).unwrap();
                Json(res)
            }),
        )
        .route(
            "/add_to_available",
            get(move |v: Json<Available>| async move {
                let db = db.lock().await;
                db.add_to_available(&v.0).unwrap();
            }),
        )
        .route(
            "/all_active",
            get(move || async move {
                let db = db.lock().await;
                let active = db.all_active().unwrap();
                Json(active)
            }),
        )
        .route(
            "/find_in_active",
            get(move |Json(url): Json<String>| async move {
                let db = db.lock().await;
                let res = db.find_in_active(&url).unwrap();
                Json(res)
            }),
        )
        .route(
            "/add_to_active",
            get(move |Json(active): Json<Active>| async move {
                let db = db.lock().await;
                let res = db.add_to_active(&active).unwrap();
                Json(res)
            }),
        )
        .route(
            "/make_active",
            get(move |Json(url): Json<String>| async move {
                let db = db.lock().await;
                let res = db.make_active(&url).unwrap();
                Json(res)
            }),
        )
        .route(
            "/set_position",
            get(move |Json((url, pos)): Json<(String, f64)>| async move {
                let db = db.lock().await;
                let res = db.set_position(&url, pos).unwrap();
                Json(res)
            }),
        )
        .route(
            "/set_duration",
            get(
                move |Json((url, duration)): Json<(String, f64)>| async move {
                    let db = db.lock().await;
                    let res = db.set_duration(&url, duration).unwrap();
                    Json(res)
                },
            ),
        )
        .route(
            "/set_title",
            get(
                move |Json((url, title)): Json<(String, String)>| async move {
                    let db = db.lock().await;
                    let res = db.set_title(&url, &title).unwrap();
                    Json(res)
                },
            ),
        )
        .route(
            "/remove_from_active",
            get(move |Json(url): Json<String>| async move {
                let db = db.lock().await;
                let res = db.remove_from_active(&url).unwrap();
                Json(res)
            }),
        );

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("localhost:3000")
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}

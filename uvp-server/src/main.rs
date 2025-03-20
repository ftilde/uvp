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

macro_rules! build_fn {
    ($db:ident, fn $fn_name:ident (&self $(, $arg:ident : &$type:ty)+) -> $ret:ty;) => {
            get(move |Json(($($arg,)*)): Json::<($($type,)*)>| async move {
                let db = $db.lock().await;
                let res = db.$fn_name($(&$arg,)*).unwrap();
                Json(res)
            })
    };
    ($db:ident, fn $fn_name:ident (&self) -> $ret:ty;) => {
            get(move || async move {
                let db = $db.lock().await;
                let res = db.$fn_name().unwrap();
                Json(res)
            })
    };
}

macro_rules! build_router {
    ($db:ident; $(fn $fn_name:ident (&self $(,$arg:ident : &$type:ty)*) -> $ret:ty;)*) => {
        Router::new()

            $(
            .route(
                std::stringify!($fn_name),
                build_fn!{$db, fn $fn_name(&self $(, $arg : &$type)*) -> $ret;}
            )
            )*
    }
}

#[tokio::main]
async fn main() {
    let args = CliArgs::parse();

    let db = uvp_state::data::Database::new(&args.db).unwrap();
    let db = &*Box::leak(Box::new(tokio::sync::Mutex::new(db)));

    // Methods copied from Store trait:
    let app = build_router!(
        db;
        fn all_feeds(&self) -> Result<Vec<Feed>, crate::Error>;
        fn add_to_feed(&self, feed: &Feed) -> Result<(), crate::Error>;
        fn remove_feed(&self, url: &String) -> Result<(), crate::Error>;
        fn set_last_update(&self, url: &String, update: &DateTime) -> Result<(), crate::Error>;

        fn all_available(&self) -> Result<Vec<Available>, crate::Error>;
        fn find_in_available(&self, url: &String) -> Result<Option<Available>, crate::Error>;
        fn remove_from_available(&self, url: &String) -> Result<(), crate::Error>;
        fn add_to_available(&self, available: &Available) -> Result<(), crate::Error>;

        fn all_active(&self) -> Result<Vec<Active>, crate::Error>;
        fn find_in_active(&self, url: &String) -> Result<Option<Active>, crate::Error>;
        fn add_to_active(&self, active: &Active) -> Result<(), crate::Error>;
        fn make_active(&self, url: &String) -> Result<(), crate::Error>;
        fn set_position(&self, url: &String, position_secs: &f64) -> Result<(), crate::Error>;
        fn set_duration(&self, url: &String, duration_secs: &f64) -> Result<(), crate::Error>;
        fn set_title(&self, url: &String, title: &String) -> Result<(), crate::Error>;
        fn remove_from_active(&self, url: &String) -> Result<(), crate::Error>;
    );

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("localhost:3000")
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}

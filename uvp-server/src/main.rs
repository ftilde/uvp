use std::path::PathBuf;

use axum::{routing::post, Json, Router};
use uvp_state::data::{Active, Available, DateTime, Feed, Store};

use clap::Parser;

#[derive(Parser)]
struct CliArgs {
    /// Path to sqlite database which stores all state
    #[arg()]
    db: PathBuf,
}

macro_rules! owned (
    ($typ:ty) => {<$typ as std::borrow::ToOwned>::Owned};
);

macro_rules! build_fn {
    ($db:ident, fn $fn_name:ident (&self $(, $arg:ident : &$type:ty)+) -> $ret:ty;) => {
            post(move |Json(($($arg,)*)): Json::<($(owned!($type),)*)>| async move {
                let db = $db.lock().await;
                let res = db.$fn_name($(&$arg,)*).unwrap();
                Json(res)
            })
    };
    ($db:ident, fn $fn_name:ident (&self) -> $ret:ty;) => {
            post(move || async move {
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
        fn remove_feed(&self, url: &str) -> Result<(), crate::Error>;
        fn set_last_update(&self, url: &str, update: &DateTime) -> Result<(), crate::Error>;

        fn all_available(&self) -> Result<Vec<Available>, crate::Error>;
        fn find_in_available(&self, url: &str) -> Result<Option<Available>, crate::Error>;
        fn remove_from_available(&self, url: &str) -> Result<(), crate::Error>;
        fn add_to_available(&self, available: &Available) -> Result<(), crate::Error>;

        fn all_active(&self) -> Result<Vec<Active>, crate::Error>;
        fn find_in_active(&self, url: &str) -> Result<Option<Active>, crate::Error>;
        fn add_to_active(&self, active: &Active) -> Result<(), crate::Error>;
        fn make_active(&self, url: &str) -> Result<(), crate::Error>;
        fn set_position(&self, url: &str, position_secs: &f64) -> Result<(), crate::Error>;
        fn set_duration(&self, url: &str, duration_secs: &f64) -> Result<(), crate::Error>;
        fn set_title(&self, url: &str, title: &str) -> Result<(), crate::Error>;
        fn remove_from_active(&self, url: &str) -> Result<(), crate::Error>;
    );

    // run our app with hyper, listening globally on port 3000
    let listener = tokio::net::TcpListener::bind("localhost:3000")
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}

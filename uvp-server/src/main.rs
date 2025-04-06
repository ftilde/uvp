use std::{path::PathBuf, time::Duration};

use axum::{routing::post, Json, Router};
use tokio::sync::Mutex;
use uvp_state::data::{Active, Available, Database, DateTime, Feed, Store};

use clap::Parser;

const REFRESH_PERIOD: Duration = Duration::from_secs(60 * 60);

#[derive(Parser)]
struct CliArgs {
    #[arg(long = "auto_refresh", short = 'r')]
    auto_refresh: bool,

    #[arg(long = "bind_address", short = 'L', default_value = "localhost:3000")]
    bind_address: String,

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
                println!(">>>> Call {}, res {:?}", stringify!($fn_name), res);
                Json(res)
            })
    };
    ($db:ident, fn $fn_name:ident (&self) -> $ret:ty;) => {
            post(move || async move {
                let db = $db.lock().await;
                let res = db.$fn_name().unwrap();
                println!(">>>> Call {}, res {:?}", stringify!($fn_name), res);
                Json(res)
            })
    };
}

macro_rules! build_router {
    ($db:ident; $(fn $fn_name:ident (&self $(,$arg:ident : &$type:ty)*) -> $ret:ty;)*) => {
        Router::new()

            $(
            .route(
                std::concat!("/", std::stringify!($fn_name)),
                build_fn!{$db, fn $fn_name(&self $(, $arg : &$type)*) -> $ret;}
            )
            )*
    }
}

fn refresh_job(db_path: PathBuf) {
    let db = Database::new(&db_path).unwrap();
    loop {
        if let Err(e) = db.refresh() {
            eprintln!("Refresh err: {:?}", e);
        }

        std::thread::sleep(REFRESH_PERIOD);
    }
}

#[tokio::main]
async fn main() {
    let args = CliArgs::parse();

    if args.auto_refresh {
        let db_path = args.db.clone();
        std::thread::spawn(|| refresh_job(db_path));
    }

    let db = Database::new(&args.db).unwrap();
    let db = &*Box::leak(Box::new(Mutex::new(db)));

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

    let listener = tokio::net::TcpListener::bind(args.bind_address)
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}

use std::{path::PathBuf, time::Duration};

use axum::{routing::post, Json, Router};
use tokio::sync::Mutex;
use uvp_state::{
    data::{Active, Available, Database, DateTime, Feed, Store},
    Error,
};

use clap::Parser;

const REFRESH_WAIT_SECS_MIN: u64 = 10;
const REFRESH_WAIT_SECS_MAX: u64 = 60;

#[derive(Parser)]
struct CliArgs {
    #[arg(long = "auto_refresh", short = 'r')]
    auto_refresh: Option<chrono::NaiveTime>,

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

fn refresh_job(db_path: PathBuf, refresh_time: chrono::NaiveTime) {
    let db = Database::new(&db_path).unwrap();

    let client = reqwest::blocking::ClientBuilder::new()
        .timeout(uvp_state::data::FETCH_TIMEOUT)
        .build()
        .unwrap();

    loop {
        let now = chrono::Local::now();
        let today = now.date_naive();
        let update_today = today.and_time(refresh_time);
        let next_update = if update_today > now.naive_local() {
            update_today
        } else {
            today.succ_opt().unwrap().and_time(refresh_time)
        };

        let sleep_time = next_update
            .signed_duration_since(now.naive_local())
            .to_std()
            .unwrap();

        eprintln!("Sleeping {} secs until next refresh", sleep_time.as_secs());

        std::thread::sleep(sleep_time);

        let mut feeds = db.all_feeds().unwrap();

        use rand::seq::SliceRandom;
        let mut r = rand::rng();
        feeds.shuffle(&mut r);

        for feed in feeds {
            let wait_duration = Duration::from_secs(rand::random_range(
                REFRESH_WAIT_SECS_MIN..REFRESH_WAIT_SECS_MAX,
            ));
            eprintln!("Waiting {} secs before next fetch", wait_duration.as_secs());

            std::thread::sleep(wait_duration);
            let fetch_result = uvp_state::feeds::fetch(&client, &feed.url);

            let fetched_feed = match fetch_result.map_err(|e| e.into()) {
                Ok(feed) => feed,
                Err(Error::Reqwest(e)) => {
                    eprintln!("Failed to fetch feed {}: {}", feed.title, e);
                    continue;
                }
                Err(Error::RSS(e)) => {
                    eprintln!("Failed to parse feed {}: {}", feed.title, e);
                    continue;
                }
                Err(Error::Atom(e)) => {
                    eprintln!("Failed to parse feed {}: {}", feed.title, e);
                    continue;
                }
                Err(e) => {
                    eprintln!("Unexpected error during fetch {}: {:?}", feed.title, e);
                    continue;
                }
            };
            if let Err(e) = uvp_state::data::update_feed(&db, feed, fetched_feed) {
                eprintln!("Failed to update feed: {:?}", e);
            }
        }
    }
}

#[tokio::main]
async fn main() {
    let args = CliArgs::parse();

    if let Some(time) = args.auto_refresh {
        let db_path = args.db.clone();
        std::thread::spawn(move || refresh_job(db_path, time));
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

use atom_syndication;
use reqwest;
use rss;
use rusqlite::{params, Connection};
use std::path::{Path, PathBuf};
use structopt::StructOpt;

mod data;
mod feeds;
mod mpv;
mod tui;

use data::*;
use feeds::fetch;

const DB_NAME: &'static str = "uvp.db";
const CONFIG_FILE_NAME: &'static str = "uvp.toml";
const DB_FILE_CONFIG_KEY: &'static str = "database_file";
const MPV_BINARY_CONFIG_KEY: &'static str = "mpv_binary";
const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

#[derive(StructOpt)]
enum Add {
    #[structopt(about = "Add a feed")]
    Feed(AddFeed),
    #[structopt(about = "Add video to the list of active videos")]
    Video(AddVideo),
}

#[derive(StructOpt)]
struct AddVideo {
    #[structopt(help = "Url")]
    url: String,
}

#[derive(StructOpt)]
enum AddFeed {
    #[structopt(about = "Add a youtube channel feed")]
    Youtube {
        #[structopt(short = "i", long = "id", help = "Fetch using the channel id")]
        channel_id: Option<String>,
        channel_name: String,
    },
    #[structopt(about = "Add a query of the German public broadcast multimedia library")]
    Mediathek {
        #[structopt(
            short = "t",
            long = "title",
            help = "Assign a title separate from the query"
        )]
        title: Option<String>,
        query: String,
    },
    #[structopt(about = "Add a custom feed via URL")]
    Other {
        #[structopt(
            short = "t",
            long = "title",
            help = "Assign a title other than the URL"
        )]
        title: Option<String>,
        url: String,
    },
}

#[derive(StructOpt)]
struct Play {
    #[structopt(help = "url")]
    url: String,
}

#[derive(StructOpt)]
enum Remove {
    #[structopt(about = "Remove a feed via its url")]
    Feed { url: String },
    #[structopt(about = "Remove a video via its url")]
    Video { url: String },
}

#[derive(StructOpt)]
enum List {
    #[structopt(about = "List feeds")]
    Feeds,
    #[structopt(about = "List available videos")]
    Available,
    #[structopt(about = "List active videos")]
    Active,
}

#[derive(StructOpt)]
#[structopt(author, about)]
enum Options {
    #[structopt(about = "Add a feed or video")]
    Add(Add),
    #[structopt(about = "Refresh the list of available videos")]
    Refresh,
    #[structopt(about = "List feeds, available or active videos")]
    List(List),
    #[structopt(about = "Play an (external) video")]
    Play(Play),
    #[structopt(about = "Remove an item from the list of available/active videos")]
    Remove(Remove),
    #[structopt(about = "Start an interactive tui for video selection")]
    Tui,
}

fn youtube_url_user(channel: &str) -> String {
    format!("https://www.youtube.com/feeds/videos.xml?user={}", channel)
}
fn youtube_url_channelid(channel: &str) -> String {
    format!(
        "https://www.youtube.com/feeds/videos.xml?channel_id={}",
        channel
    )
}

fn mediathek_url(channel: &str) -> String {
    format!("https://mediathekviewweb.de/feed?query={}", channel)
}

fn ignore_constraint_errors(res: Result<(), rusqlite::Error>) -> Result<(), rusqlite::Error> {
    match res {
        Err(rusqlite::Error::SqliteFailure(error, _))
            if error.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            Ok(())
        }
        o => o,
    }
}

#[derive(Debug)]
pub enum Error {
    Reqwest(reqwest::Error),
    RSS(rss::Error),
    Atom(atom_syndication::Error),
    DB(rusqlite::Error),
}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        Error::Reqwest(error)
    }
}
impl From<rss::Error> for Error {
    fn from(error: rss::Error) -> Self {
        Error::RSS(error)
    }
}
impl From<atom_syndication::Error> for Error {
    fn from(error: atom_syndication::Error) -> Self {
        Error::Atom(error)
    }
}
impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Error::DB(error)
    }
}

fn refresh(conn: &Connection) -> Result<(), rusqlite::Error> {
    let client = reqwest::ClientBuilder::new()
        .timeout(FETCH_TIMEOUT)
        .build()
        .unwrap();
    let fetches =
        futures_util::future::join_all(iter_feeds(&conn)?.into_iter().map(|feed| async {
            let fetch_result = fetch(&client, &feed.url).await;
            (fetch_result, feed)
        }));
    let mut rt = tokio::runtime::Builder::new()
        .basic_scheduler()
        .enable_io()
        .enable_time()
        .build()
        .unwrap();
    let fetched_feeds = rt.block_on(fetches);
    for (fetch_result, feed) in fetched_feeds {
        let mut lastpublication = feed.lastupdate;

        let fetched_feed = match fetch_result {
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
                panic!("Unexpected error during fetch: {:?}", e);
            }
        };
        for entry in fetched_feed.entries() {
            if feed.lastupdate.is_none() || feed.lastupdate.unwrap() < entry.publication {
                ignore_constraint_errors(add_entry_to_available(&conn, feed.url.clone(), &entry))?;
            }
            lastpublication = if let Some(lastpublication) = lastpublication {
                Some(entry.publication.max(lastpublication))
            } else {
                Some(entry.publication)
            }
        }
        if let Some(lastpublication) = lastpublication {
            conn.execute(
                r#"
                UPDATE feed SET lastupdate = ?1 WHERE feedurl = ?2
                "#,
                params!(lastpublication.to_rfc3339(), feed.url),
            )?;
        }
    }
    Ok(())
}
fn main() -> Result<(), Error> {
    let mut settings = config::Config::default();
    let default_db_path = dirs::data_dir()
        .unwrap_or(Path::new("./").to_owned())
        .join(DB_NAME);

    settings
        .set_default(
            DB_FILE_CONFIG_KEY,
            default_db_path.to_string_lossy().as_ref(),
        )
        .unwrap();
    settings.set_default(MPV_BINARY_CONFIG_KEY, "mpv").unwrap();

    for config_location in vec![
        Some(PathBuf::from("/etc")),
        Some(PathBuf::from("/usr/etc")),
        dirs::config_dir(),
    ] {
        if let Some(config_location) = config_location {
            let config_file = config_location.join(CONFIG_FILE_NAME);
            if config_file.is_file() {
                let config_file_name = config_file.to_string_lossy();
                if let Err(e) = settings.merge(config::File::with_name(&config_file_name)) {
                    panic!("Failed to load config file {}: {:?}", config_file_name, e);
                }
            }
        }
    }

    let db_path = settings.get_str(DB_FILE_CONFIG_KEY).unwrap();
    let mpv_binary = settings.get_str(MPV_BINARY_CONFIG_KEY).unwrap();

    //let flags = OpenFlags::SQLITE_OPEN_FULL_MUTEX;
    //let conn = Connection::open_with_flags(db_path, flags).unwrap();
    let conn = Connection::open(Path::new(&db_path))?;
    for def in TABLE_DEFINITIONS {
        conn.execute(def, params![])?;
    }
    match Options::from_args() {
        Options::Add(Add::Video(vid)) => {
            make_active(&conn, &vid.url)?;
        }
        Options::Play(p) => {
            mpv::play(&conn, &p.url, &mpv_binary)?;
        }
        Options::Add(Add::Feed(add)) => {
            let feed = match add {
                AddFeed::Youtube {
                    channel_name,
                    channel_id,
                } => {
                    let url = if let Some(channel_id) = channel_id {
                        youtube_url_channelid(&channel_id)
                    } else {
                        youtube_url_user(&channel_name)
                    };
                    Feed {
                        title: channel_name,
                        url,
                        lastupdate: None,
                    }
                }
                AddFeed::Mediathek { title, query } => {
                    let url = mediathek_url(&query);
                    Feed {
                        title: if let Some(title) = title {
                            title
                        } else {
                            query
                        },
                        url,
                        lastupdate: None,
                    }
                }
                AddFeed::Other { title, url } => Feed {
                    title: if let Some(title) = title {
                        title
                    } else {
                        url.clone()
                    },
                    url,
                    lastupdate: None,
                },
            };
            add_to_feed(&conn, &feed)?;
        }
        Options::List(what) => match what {
            List::Feeds => {
                println!("{} \t| {} \t| {}", "Title", "Last Update", "Url");
                for feed in iter_feeds(&conn)? {
                    println!(
                        "{} \t| {} \t| {}",
                        feed.title,
                        feed.lastupdate
                            .map(|lu| lu.to_rfc3339())
                            .unwrap_or("Never".to_owned()),
                        feed.url,
                    );
                }
            }
            List::Available => {
                println!(
                    "{} \t| {} \t| {} \t| {}",
                    "Title", "Duration", "Publication", "Url"
                );
                for entry in iter_available(&conn)? {
                    println!(
                        "{} \t| {:?} \t| {} \t| {}",
                        entry.title,
                        entry.duration_secs,
                        entry.publication.to_rfc3339(),
                        entry.url,
                    );
                }
            }
            List::Active => {
                println!("{} \t| {} \t| {}", "Title", "Url", "Playback");
                for entry in iter_active(&conn)? {
                    let title = entry.title.unwrap_or("Unknown".to_string());
                    println!("{} \t| {} \t {}", title, entry.url, entry.position_secs);
                }
            }
        },
        Options::Remove(Remove::Video { url }) => {
            remove_from_available(&conn, &url)?;
        }
        Options::Remove(Remove::Feed { url }) => {
            remove_feed(&conn, &url)?;
        }
        Options::Refresh => {
            refresh(&conn)?;
        }
        Options::Tui => {
            tui::run(&conn, &mpv_binary)?;
        }
    }
    Ok(())
}

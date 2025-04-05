use std::{
    convert::{TryFrom, TryInto},
    iter::FromIterator,
    path::{Path, PathBuf},
};
use structopt::StructOpt;
use unsegen::base::Color;
use uvp_state::data::{Feed, Store};

mod mpv;
mod tui;

const DB_NAME: &'static str = "uvp.db";
const CONFIG_FILE_NAME: &'static str = "uvp.toml";
const DB_FILE_CONFIG_KEY: &'static str = "database_file";
const MPV_BINARY_CONFIG_KEY: &'static str = "mpv_binary";
const THEME_CONFIG_KEY: &'static str = "theme";

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

#[derive(Debug)]
pub enum Error {
    State(uvp_state::Error),
    Config(config::ConfigError),
}

impl From<uvp_state::Error> for Error {
    fn from(value: uvp_state::Error) -> Self {
        Self::State(value)
    }
}
impl From<config::ConfigError> for Error {
    fn from(error: config::ConfigError) -> Self {
        Error::Config(error)
    }
}

impl From<std::num::ParseIntError> for Error {
    fn from(value: std::num::ParseIntError) -> Self {
        Error::Config(config::ConfigError::Foreign(Box::new(value)))
    }
}

struct Theme {
    primary_fg: Color,
    primary_bg: Color,
    alt_fg: Color,
    alt_bg: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            primary_fg: Color::Default,
            primary_bg: Color::Default,
            alt_fg: Color::Default,
            alt_bg: Color::Ansi(8),
        }
    }
}

impl Theme {
    const KEYS: &'static [&'static str] = &["primary_fg", "primary_bg", "alt_fg", "alt_bg"];
}

impl TryFrom<config::Map<String, config::Value>> for Theme {
    type Error = Error;

    fn try_from(value: config::Map<String, config::Value>) -> Result<Self, Self::Error> {
        let mut theme = Theme::default();

        for key in Self::KEYS {
            if let Ok(v) = value
                .get(*key)
                .ok_or(config::ConfigError::NotFound(key.to_string()))
                .and_then(|v| v.clone().into_string())
            {
                let value = match v.as_str() {
                    "default" => Color::Default,
                    _ => Color::Ansi(v.parse::<u8>()?),
                };

                match *key {
                    "primary_fg" => theme.primary_fg = value,
                    "primary_bg" => theme.primary_bg = value,
                    "alt_fg" => theme.alt_fg = value,
                    "alt_bg" => theme.alt_bg = value,
                    _ => continue,
                }
            }
        }

        Ok(theme)
    }
}

impl From<Theme> for config::Value {
    fn from(value: Theme) -> Self {
        let values = [
            value.primary_fg,
            value.primary_bg,
            value.alt_fg,
            value.alt_bg,
        ];

        let map = values.iter().zip(Theme::KEYS).map(|(v, k)| {
            let color_code = match v {
                Color::Ansi(n) => n.to_string(),
                Color::Default => "default".to_string(),
                _ => unreachable!(),
            };
            (
                (*k).to_owned(),
                config::Value::new(
                    Some(&(*k).to_owned()),
                    config::ValueKind::String(color_code),
                ),
            )
        });

        config::Value::new(
            Some(&THEME_CONFIG_KEY.to_owned()),
            config::ValueKind::Table(config::Map::from_iter(map.into_iter())),
        )
    }
}

fn main() -> Result<(), Error> {
    let default_db_path = dirs::data_dir()
        .unwrap_or(Path::new("./").to_owned())
        .join(DB_NAME);

    let mut settings_builder = config::Config::builder()
        .set_default(
            DB_FILE_CONFIG_KEY,
            default_db_path.to_string_lossy().as_ref(),
        )?
        .set_default(MPV_BINARY_CONFIG_KEY, "mpv")?
        .set_default(THEME_CONFIG_KEY, Theme::default())?;

    for config_location in vec![
        Some(PathBuf::from("/etc")),
        Some(PathBuf::from("/usr/etc")),
        dirs::config_dir(),
    ] {
        if let Some(config_location) = config_location {
            let config_file = config_location.join(CONFIG_FILE_NAME);
            if config_file.is_file() {
                settings_builder = settings_builder.add_source(config::File::new(
                    config_file.to_str().unwrap(),
                    config::FileFormat::Toml,
                ));
            }
        }
    }

    let settings = settings_builder.build()?;

    let db_path = settings.get_string(DB_FILE_CONFIG_KEY).unwrap();
    let mpv_binary = settings.get_string(MPV_BINARY_CONFIG_KEY).unwrap();

    let theme: Theme = settings.get_table(THEME_CONFIG_KEY)?.try_into()?;

    //let flags = OpenFlags::SQLITE_OPEN_FULL_MUTEX;
    //let conn = Connection::open_with_flags(db_path, flags).unwrap();
    let db = uvp_state::data::Database::new(Path::new(&db_path)).unwrap();
    //let db = uvp_state::data::HttpStore::new("http://localhost:3000");
    let store: Box<dyn Store> = Box::new(db);

    match Options::from_args() {
        Options::Add(Add::Video(vid)) => {
            store.make_active(&vid.url)?;
        }
        Options::Play(p) => {
            mpv::play(&*store, &p.url, &mpv_binary)?;
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
            store.add_to_feed(&feed)?;
        }
        Options::List(what) => match what {
            List::Feeds => {
                println!("{} \t| {} \t| {}", "Title", "Last Update", "Url");
                for feed in store.all_feeds()? {
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
                println!("{} \t| {} \t| {}", "Title", "Publication", "Url");
                for entry in store.all_available()? {
                    println!(
                        "{} \t| {} \t| {}",
                        entry.title,
                        entry.publication.to_rfc3339(),
                        entry.url,
                    );
                }
            }
            List::Active => {
                println!("{} \t| {} \t| {}", "Title", "Url", "Playback");
                for entry in store.all_active()? {
                    let title = entry.title.unwrap_or("Unknown".to_string());
                    println!("{} \t| {} \t {}", title, entry.url, entry.position_secs);
                }
            }
        },
        Options::Remove(Remove::Video { url }) => {
            store.remove_from_available(&url)?;
        }
        Options::Remove(Remove::Feed { url }) => {
            store.remove_feed(&url)?;
        }
        Options::Refresh => {
            store.refresh()?;
        }
        Options::Tui => {
            tui::run(&*store, &mpv_binary, &theme)?;
        }
    }
    Ok(())
}

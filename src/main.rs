use atom_syndication;
use derive_more::*;
use reqwest;
use rss;
use rusqlite::{params, Connection};
use std::path::Path;
use std::str::FromStr;
use structopt::StructOpt;

#[derive(StructOpt)]
#[structopt()]
enum Add {
    Youtube {
        channel_name: String,
    },
    Mediathek {
        query: String,
    },
    Other {
        #[structopt(flatten)]
        feed: Feed,
    },
}

#[derive(StructOpt)]
#[structopt()]
struct Feed {
    #[structopt(short = "t", long = "title", help = "Title")]
    title: String,
    #[structopt(short = "u", long = "url", help = "URL")]
    url: String,
}

#[derive(StructOpt)]
#[structopt()]
enum Options {
    Add(Add),
    Fetch,
    ListFeeds,
}

fn youtube_url(channel: &str) -> String {
    format!("https://www.youtube.com/feeds/videos.xml?user={}", channel)
}

fn mediathek_url(channel: &str) -> String {
    format!("https://mediathekviewweb.de/feed?query={}", channel)
}

#[derive(Debug)]
enum FeedEntries {
    Atom(Box<atom_syndication::Feed>),
    RSS(Box<rss::Channel>),
}

impl FeedEntries {
    fn entries(&self) -> Vec<Entry> {
        match self {
            FeedEntries::Atom(f) => f.entries().iter().filter_map(Entry::from_atom).collect(),
            FeedEntries::RSS(c) => c.items().iter().filter_map(Entry::from_rss).collect(),
        }
    }
}

#[derive(Debug)]
struct Entry {
    title: String,
    link: String,
    publication: String,
}

impl Entry {
    fn from_atom(entry: &atom_syndication::Entry) -> Option<Self> {
        Some(Entry {
            title: entry.title().to_owned(),
            link: entry.links().first()?.href().to_owned(),
            publication: entry.published()?.to_owned(),
        })
    }
    fn from_rss(entry: &rss::Item) -> Option<Self> {
        Some(Entry {
            title: entry.title()?.to_owned(),
            link: entry.link()?.to_owned(),
            publication: entry.pub_date()?.to_owned(),
        })
    }
}

fn parse(xml: &str) -> Result<FeedEntries, Error> {
    if let Ok(channel) = rss::Channel::from_str(&xml) {
        return Ok(FeedEntries::RSS(Box::new(channel)));
    }
    Ok(FeedEntries::Atom(Box::new(
        atom_syndication::Feed::from_str(&xml)?,
    )))
}

const DB_NAME: &'static str = "umc.db";
const TABLE_DEFINITION: &'static str = r#"
CREATE TABLE IF NOT EXISTS feed (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    title   TEXT NOT NULL,
    url     TEXT NOT NULL
);
"#;

#[derive(From, Debug)]
enum Error {
    Reqwest(reqwest::Error),
    RSS(rss::Error),
    Atom(atom_syndication::Error),
    DB(rusqlite::Error),
}

fn fetch(url: &str) -> Result<FeedEntries, Error> {
    println!("Fetching from url: {}", url);
    let xml_resp = reqwest::get(url)?.text()?;
    //println!("Response: {}", xml_resp);
    Ok(parse(&xml_resp)?)
}

fn iter_feeds(conn: &Connection) -> Result<Vec<Result<Feed, rusqlite::Error>>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        r#"
        SELECT title, url FROM feed
        "#,
    )?;
    let res = stmt
        .query_map(params!(), |row| {
            Ok(Feed {
                title: row.get(0)?,
                url: row.get(1)?,
            })
        })?
        .collect();
    Ok(res)
}

fn main() -> Result<(), Error> {
    let db_path = dirs::data_dir()
        .unwrap_or(Path::new("./").to_owned())
        .join(DB_NAME);

    //let flags = OpenFlags::SQLITE_OPEN_FULL_MUTEX;
    //let conn = Connection::open_with_flags(db_path, flags).unwrap();
    let conn = Connection::open(db_path).unwrap();
    conn.execute(TABLE_DEFINITION, params![])?;
    match Options::from_args() {
        Options::Add(add) => {
            let feed = match add {
                Add::Youtube { channel_name } => {
                    let url = youtube_url(&channel_name);
                    Feed {
                        title: channel_name,
                        url,
                    }
                }
                Add::Mediathek { query } => {
                    let url = mediathek_url(&query);
                    Feed { title: query, url }
                }
                Add::Other { feed } => feed,
            };
            conn.execute(
                r#"
                INSERT INTO feed (title, url) VALUES (?1, ?2)
                "#,
                params!(feed.title, feed.url),
            )?;
        }
        Options::Fetch => {
            for feed in iter_feeds(&conn)? {
                let feed = feed.unwrap();

                println!("Feed {}", feed.title);
                let feed = fetch(&feed.url).unwrap();
                for entry in feed.entries() {
                    println!("\t {:?}", entry);
                }
            }
        }
        Options::ListFeeds => {
            for feed in iter_feeds(&conn)? {
                let feed = feed.unwrap();
                println!("Feed {}, url {}", feed.title, feed.url);
            }
        }
    }
    Ok(())
}

use atom_syndication;
use derive_more::*;
use reqwest;
use rss;
use rusqlite::{params, Connection};
use std::path::Path;
use structopt::StructOpt;

mod data;
mod feeds;

use feeds::fetch;

use data::*;

#[derive(StructOpt)]
enum Add {
    Youtube {
        channel_name: String,
    },
    Mediathek {
        query: String,
    },
    Other {
        #[structopt(short = "t", long = "title", help = "Title")]
        title: String,
        #[structopt(short = "u", long = "url", help = "URL")]
        url: String,
    },
}

#[derive(StructOpt)]
struct Remove {
    #[structopt(help = "Url")]
    link: String,
}

#[derive(StructOpt)]
#[structopt(help = "List something")]
enum List {
    #[structopt(help = "List feeds")]
    Feeds,
    #[structopt(help = "List available videos")]
    Available,
}

#[derive(StructOpt)]
#[structopt()]
enum Options {
    #[structopt(about = "Add a feed")]
    Add(Add),
    Fetch,
    #[structopt(about = "Refresh the list of available videos")]
    Refresh,
    #[structopt(about = "List parts of database")]
    List(List),
    #[structopt(about = "Remove an item from the list of available videos")]
    Remove(Remove),
}

fn youtube_url(channel: &str) -> String {
    format!("https://www.youtube.com/feeds/videos.xml?user={}", channel)
}

fn mediathek_url(channel: &str) -> String {
    format!("https://mediathekviewweb.de/feed?query={}", channel)
}

const DB_NAME: &'static str = "umc.db";

#[derive(From, Debug)]
pub enum Error {
    Reqwest(reqwest::Error),
    RSS(rss::Error),
    Atom(atom_syndication::Error),
    DB(rusqlite::Error),
}

fn iter_feeds(
    conn: &Connection,
) -> Result<Vec<Result<(u32, Feed), rusqlite::Error>>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        r#"
        SELECT feedid, title, url, lastupdate FROM feed
        "#,
    )?;
    let res = stmt
        .query_map(params!(), |row| {
            Ok((
                row.get(0)?,
                Feed {
                    title: row.get(1)?,
                    url: row.get(2)?,
                    lastupdate: row.get(3).map(|lastupdate: Option<String>| {
                        lastupdate.map(|lastupdate| {
                            time::strptime(&lastupdate, TIME_FORMAT_RFC3339).unwrap()
                        })
                    })?,
                },
            ))
        })?
        .collect();
    Ok(res)
}

fn iter_available(
    conn: &Connection,
) -> Result<Vec<Result<Available, rusqlite::Error>>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        r#"
        SELECT title, link, publication FROM available
        "#,
    )?;
    let res = stmt
        .query_map(params!(), |row| {
            let publication: String = row.get(2)?;
            Ok(Available {
                title: row.get(0)?,
                link: row.get(1)?,
                publication: time::strptime(&publication, TIME_FORMAT_RFC3339).unwrap(),
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
    for def in TABLE_DEFINITIONS {
        conn.execute(def, params![])?;
    }
    match Options::from_args() {
        Options::Add(add) => {
            let feed = match add {
                Add::Youtube { channel_name } => {
                    let url = youtube_url(&channel_name);
                    Feed {
                        title: channel_name,
                        url,
                        lastupdate: None,
                    }
                }
                Add::Mediathek { query } => {
                    let url = mediathek_url(&query);
                    Feed {
                        title: query,
                        url,
                        lastupdate: None,
                    }
                }
                Add::Other { title, url } => Feed {
                    title,
                    url,
                    lastupdate: None,
                },
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
                let (_, feed) = feed.unwrap();

                println!("Feed {}", feed.title);
                let feed = fetch(&feed.url).unwrap();
                for entry in feed.entries() {
                    println!(
                        "Available {}, url {}, publication {}",
                        entry.title,
                        entry.link,
                        entry.publication.strftime(TIME_FORMAT_RFC3339).unwrap()
                    );
                }
            }
        }
        Options::List(what) => match what {
            List::Feeds => {
                for feed in iter_feeds(&conn)? {
                    let (_, feed) = feed.unwrap();
                    println!(
                        "Feed {}, url {}, last update: {}",
                        feed.title,
                        feed.url,
                        feed.lastupdate
                            .map(|lu| lu.strftime(TIME_FORMAT_RFC3339).unwrap().to_string())
                            .unwrap_or("Never".to_owned())
                    );
                }
            }
            List::Available => {
                for entry in iter_available(&conn)? {
                    let entry = entry.unwrap();
                    println!(
                        "Entry {}, url {}, publication {}",
                        entry.title,
                        entry.link,
                        entry.publication.strftime(TIME_FORMAT_RFC3339).unwrap()
                    );
                }
            }
        },
        Options::Remove(remove) => {
            conn.execute(
                r#"
                DELETE FROM entry WHERE link = ?1
                "#,
                params!(remove.link),
            )?;
        }
        Options::Refresh => {
            for feed in iter_feeds(&conn)? {
                let (fid, feed) = feed.unwrap();

                let mut lastpublication = feed.lastupdate;

                for entry in fetch(&feed.url).unwrap().entries() {
                    // FIXME: might swallow entries if entries have identical publication dates
                    // due to < and not <=. However, <= tries to insert the latest already present
                    // entry again.
                    if feed.lastupdate.is_none() || feed.lastupdate.unwrap() < entry.publication {
                        conn.execute(
                            r#"
                            INSERT INTO entry (title, link, feedid, publication) VALUES (?1, ?2, ?3, ?4)
                            "#,
                            params!(
                                entry.title,
                                entry.link,
                                fid,
                                entry
                                    .publication
                                    .strftime(TIME_FORMAT_RFC3339)
                                    .unwrap()
                                    .to_string()
                            ),
                        )?;
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
                        UPDATE feed SET lastupdate = ?1 WHERE feedid = ?2
                        "#,
                        params!(
                            lastpublication
                                .strftime(TIME_FORMAT_RFC3339)
                                .unwrap()
                                .to_string(),
                            fid
                        ),
                    )?;
                }
            }
        }
    }
    Ok(())
}

use atom_syndication;
use derive_more::*;
use reqwest;
use rss;
use rusqlite::{params, Connection};
use std::path::Path;
use structopt::StructOpt;

mod data;
mod feeds;

use data::*;
use feeds::fetch;

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
    link: String,
}

#[derive(StructOpt)]
enum AddFeed {
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
    #[structopt(about = "List feeds")]
    Feeds,
    #[structopt(about = "List available videos")]
    Available,
    #[structopt(about = "List active videos")]
    Active,
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
        Options::Add(Add::Video(vid)) => {
            if let Some(available) = find_in_available(&conn, &vid.link)? {
                add_to_active(&conn, &vid.link, &available.title)?;
                remove_from_available(&conn, &vid.link)?;
            } else {
                add_to_active(&conn, &vid.link, &vid.link)?;
            }
        }
        Options::Add(Add::Feed(add)) => {
            let feed = match add {
                AddFeed::Youtube { channel_name } => {
                    let url = youtube_url(&channel_name);
                    Feed {
                        title: channel_name,
                        url,
                        lastupdate: None,
                    }
                }
                AddFeed::Mediathek { query } => {
                    let url = mediathek_url(&query);
                    Feed {
                        title: query,
                        url,
                        lastupdate: None,
                    }
                }
                AddFeed::Other { title, url } => Feed {
                    title,
                    url,
                    lastupdate: None,
                },
            };
            add_to_feed(&conn, &feed)?;
        }
        Options::Fetch => {
            for feed in iter_feeds(&conn)? {
                let (_, feed) = feed.unwrap();

                println!("Feed {}", feed.title);
                println!("{} \t| {} \t| {}", "Title", "Publication", "Link");
                let feed = fetch(&feed.url).unwrap();
                for entry in feed.entries() {
                    println!(
                        "{} \t| {} \t| \"{}\"",
                        entry.title,
                        entry.publication.to_rfc3339(),
                        entry.link,
                    );
                }
            }
        }
        Options::List(what) => match what {
            List::Feeds => {
                println!("{} \t| {} \t| {}", "Title", "Last Update", "Url");
                for feed in iter_feeds(&conn)? {
                    let (_, feed) = feed.unwrap();
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
                for entry in iter_available(&conn)? {
                    let entry = entry.unwrap();
                    println!(
                        "{} \t| {} \t| {}",
                        entry.title,
                        entry.publication.to_rfc3339(),
                        entry.link,
                    );
                }
            }
            List::Active => {
                println!("{} \t| {}", "Title", "Url");
                for entry in iter_active(&conn)? {
                    let entry = entry.unwrap();
                    println!("{} \t| {}", entry.title, entry.link,);
                }
            }
        },
        Options::Remove(remove) => {
            remove_from_available(&conn, &remove.link)?;
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
                        add_to_available(&conn, fid, &entry)?;
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
                        params!(lastpublication.to_rfc3339(), fid),
                    )?;
                }
            }
        }
    }
    Ok(())
}

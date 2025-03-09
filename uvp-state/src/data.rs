use std::path::Path;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::feeds::fetch;

pub type DateTime = chrono::DateTime<chrono::FixedOffset>; //TODO use UTC, rusqlite has direct support for it

const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

fn parse(s: &str) -> chrono::ParseResult<DateTime> {
    DateTime::parse_from_rfc3339(s)
}
fn to_string(d: &DateTime) -> String {
    d.to_rfc3339()
}

pub struct Database {
    connection: Connection,
}

impl Database {
    pub fn new(db_path: &Path) -> crate::Result<Database> {
        let conn = Connection::open(db_path)?;
        for def in TABLE_DEFINITIONS {
            conn.execute(def, params![])?;
        }

        Ok(Self { connection: conn })
    }
}

const TABLE_DEFINITION_ACTIVE: &'static str = r#"
CREATE TABLE IF NOT EXISTS active (
    url            TEXT PRIMARY KEY,
    title          TEXT,
    position_secs  FLOAT NOT NULL,
    duration_secs  FLOAT,
    feed_title     TEXT
);
"#;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Active {
    pub title: Option<String>,
    pub url: String,
    pub position_secs: f64,
    pub duration_secs: Option<f64>,
    pub feed_title: Option<String>,
}

const TABLE_DEFINITION_AVAILABLE: &'static str = r#"
CREATE TABLE IF NOT EXISTS available (
    title          TEXT NOT NULL,
    url            TEXT PRIMARY KEY,
    publication    TEXT NOT NULL,
    feedurl        TEXT NOT NULL,
    FOREIGN KEY(feedurl) REFERENCES feed
);
"#;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Available {
    pub title: String,
    pub url: String,
    pub publication: DateTime,
    pub feed: Feed,
}

const TABLE_DEFINITION_FEED: &'static str = r#"
CREATE TABLE IF NOT EXISTS feed (
    feedurl         TEXT PRIMARY KEY,
    title           TEXT NOT NULL,
    lastupdate      Text
);
"#;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Feed {
    pub title: String,
    pub url: String,
    pub lastupdate: Option<DateTime>,
}

const TABLE_DEFINITIONS: &[&str] = &[
    TABLE_DEFINITION_FEED,
    TABLE_DEFINITION_AVAILABLE,
    TABLE_DEFINITION_ACTIVE,
];

/// Feed -----------------------------------------------------------------------

impl Database {
    pub fn iter_feeds(&self) -> Result<Vec<Feed>, crate::Error> {
        let mut stmt = self.connection.prepare(
            r#"
        SELECT feedurl, title, lastupdate FROM feed
        "#,
        )?;
        let res = stmt
            .query_map(params!(), |row| {
                Ok(Feed {
                    url: row.get(0)?,
                    title: row.get(1)?,
                    lastupdate: row.get(2).map(|lastupdate: Option<String>| {
                        lastupdate.map(|lastupdate| parse(&lastupdate).unwrap())
                    })?,
                })
            })?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        Ok(res)
    }
    pub fn add_to_feed(&self, feed: &Feed) -> Result<(), crate::Error> {
        self.connection.execute(
            r#"
        INSERT INTO feed (title, feedurl) VALUES (?1, ?2)
        "#,
            params!(feed.title, feed.url),
        )?;
        Ok(())
    }
    pub fn remove_feed(&self, url: &str) -> Result<(), crate::Error> {
        self.connection.execute(
            r#"
        DELETE FROM feed WHERE feedurl = ?1
        "#,
            params!(url),
        )?;
        Ok(())
    }
}

/// Available ------------------------------------------------------------------
impl Database {
    pub fn all_available(&self) -> Result<Vec<Available>, crate::Error> {
        let mut stmt = self.connection.prepare(
            r#"
        SELECT available.title, url, publication, feedurl, feed.title, lastupdate
        FROM available INNER JOIN feed USING(feedurl)
        ORDER BY publication DESC
        "#,
        )?;
        let res = stmt
            .query_map(params!(), |row| {
                let publication: String = row.get(2)?;
                Ok(Available {
                    title: row.get(0)?,
                    url: row.get(1)?,
                    publication: parse(&publication).unwrap(),
                    feed: Feed {
                        url: row.get(3)?,
                        title: row.get(4)?,
                        lastupdate: row.get(5).map(|lastupdate: Option<String>| {
                            lastupdate.map(|lastupdate| parse(&lastupdate).unwrap())
                        })?,
                    },
                })
            })?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        Ok(res)
    }

    pub fn find_in_available(&self, url: &str) -> Result<Option<Available>, crate::Error> {
        let mut stmt = self.connection.prepare(
            r#"
        SELECT available.title, url, publication, feedurl, feed.title, lastupdate
        FROM available INNER JOIN feed USING(feedurl)
        WHERE url = ?1
        "#,
        )?;
        let res = stmt.query_map(params!(url), |row| {
            let publication: String = row.get(2)?;
            Ok(Available {
                title: row.get(0)?,
                url: row.get(1)?,
                publication: parse(&publication).unwrap(),
                feed: Feed {
                    url: row.get(3)?,
                    title: row.get(4)?,
                    lastupdate: row.get(5).map(|lastupdate: Option<String>| {
                        lastupdate.map(|lastupdate| parse(&lastupdate).unwrap())
                    })?,
                },
            })
        })?;
        let mut iter = res.into_iter();
        Ok(iter.next().transpose()?)
    }

    pub fn remove_from_available(&self, url: &str) -> Result<(), crate::Error> {
        self.connection.execute(
            r#"
        DELETE FROM available WHERE url = ?1
        "#,
            params!(url),
        )?;
        Ok(())
    }

    pub fn add_to_available(&self, available: &Available) -> Result<(), crate::Error> {
        self.connection.execute(
            r#"
        INSERT INTO available (title, url, feedurl, publication) VALUES (?1, ?2, ?3, ?4)
        "#,
            params!(
                available.title,
                available.url,
                available.feed.url,
                to_string(&available.publication)
            ),
        )?;
        Ok(())
    }

    pub fn set_last_update(&self, url: &str, update: DateTime) -> Result<(), crate::Error> {
        self.connection.execute(
            r#"
                UPDATE feed SET lastupdate = ?1 WHERE feedurl = ?2
                "#,
            params!(update.to_rfc3339(), url),
        )?;
        Ok(())
    }
}

/// Active ---------------------------------------------------------------------
impl Database {
    pub fn iter_active(&self) -> Result<Vec<Active>, crate::Error> {
        let mut stmt = self.connection.prepare(
            r#"
        SELECT title, url, position_secs, duration_secs, feed_title
        FROM active
        "#,
        )?;
        let res = stmt
            .query_map(params!(), |row| {
                Ok(Active {
                    title: row.get(0)?,
                    url: row.get(1)?,
                    position_secs: row.get(2)?,
                    duration_secs: row.get(3)?,
                    feed_title: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        Ok(res)
    }

    pub fn find_in_active(&self, url: &str) -> Result<Option<Active>, crate::Error> {
        let mut stmt = self.connection.prepare(
            r#"
        SELECT title, url, position_secs, duration_secs, feed_title
        FROM active
        where url = ?1
        "#,
        )?;
        let res = stmt.query_map(params!(url), |row| {
            Ok(Active {
                title: row.get(0)?,
                url: row.get(1)?,
                position_secs: row.get(2)?,
                duration_secs: row.get(3)?,
                feed_title: row.get(4)?,
            })
        })?;
        let mut iter = res.into_iter();
        Ok(iter.next().transpose()?)
    }

    pub fn add_to_active(&self, active: &Active) -> Result<(), crate::Error> {
        self.connection.execute(
            r#"
        INSERT INTO active (url, title, position_secs, feed_title) VALUES (?1, ?2, ?3, ?4)
        "#,
            params!(
                active.url,
                active.title,
                active.position_secs,
                active.feed_title
            ),
        )?;
        Ok(())
    }

    pub fn make_active(&self, url: &str) -> Result<(), crate::Error> {
        if let Some(available) = self.find_in_available(url)? {
            self.add_to_active(&Active {
                url: url.to_owned(),
                title: Some(available.title),
                position_secs: 0.0,
                duration_secs: None,
                feed_title: Some(available.feed.title),
            })?;
            self.remove_from_available(url)
        } else {
            self.add_to_active(&Active {
                url: url.to_owned(),
                title: None,
                position_secs: 0.0,
                duration_secs: None,
                feed_title: None,
            })
        }
    }
    pub fn set_position_secs(&self, url: &str, position_secs: f64) -> Result<(), crate::Error> {
        self.connection.execute(
            r#"
        UPDATE active SET position_secs = ?1 WHERE url = ?2
        "#,
            params!(position_secs, url),
        )?;
        Ok(())
    }
    pub fn set_duration(&self, url: &str, duration_secs: f64) -> Result<(), crate::Error> {
        self.connection.execute(
            r#"
        UPDATE active SET duration_secs = ?1 WHERE url = ?2
        "#,
            params!(duration_secs, url),
        )?;
        Ok(())
    }
    pub fn set_title(&self, url: &str, title: &str) -> Result<(), crate::Error> {
        self.connection.execute(
            r#"
        UPDATE active SET title = ?1 WHERE url = ?2
        "#,
            params!(title, url),
        )?;
        Ok(())
    }
    pub fn remove_from_active(&self, url: &str) -> Result<(), crate::Error> {
        self.connection.execute(
            r#"
        DELETE FROM active WHERE url = ?1
        "#,
            params!(url),
        )?;
        Ok(())
    }

    pub fn refresh(&self) -> Result<(), crate::Error> {
        let client = reqwest::ClientBuilder::new()
            .timeout(FETCH_TIMEOUT)
            .build()
            .unwrap();
        let fetches =
            futures_util::future::join_all(self.iter_feeds()?.into_iter().map(|feed| async {
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

            let fetched_feed = match fetch_result.map_err(|e| e.into()) {
                Ok(feed) => feed,
                Err(crate::Error::Reqwest(e)) => {
                    eprintln!("Failed to fetch feed {}: {}", feed.title, e);
                    continue;
                }
                Err(crate::Error::RSS(e)) => {
                    eprintln!("Failed to parse feed {}: {}", feed.title, e);
                    continue;
                }
                Err(crate::Error::Atom(e)) => {
                    eprintln!("Failed to parse feed {}: {}", feed.title, e);
                    continue;
                }
                Err(e) => {
                    panic!("Unexpected error during fetch: {:?}", e);
                }
            };
            for entry in fetched_feed.entries() {
                if feed.lastupdate.is_none() || feed.lastupdate.unwrap() < entry.publication {
                    let available = Available {
                        title: entry.title,
                        url: entry.url,
                        publication: entry.publication,
                        feed: feed.clone(),
                    };
                    ignore_constraint_errors(self.add_to_available(&available))?;
                }
                lastpublication = if let Some(lastpublication) = lastpublication {
                    Some(entry.publication.max(lastpublication))
                } else {
                    Some(entry.publication)
                }
            }
            if let Some(lastpublication) = lastpublication {
                self.set_last_update(&feed.url, lastpublication)?;
            }
        }
        Ok(())
    }
}

pub fn ignore_constraint_errors(res: Result<(), crate::Error>) -> Result<(), crate::Error> {
    match res {
        Err(crate::Error::DB(rusqlite::Error::SqliteFailure(error, _)))
            if error.code == rusqlite::ErrorCode::ConstraintViolation =>
        {
            Ok(())
        }
        o => o,
    }
}

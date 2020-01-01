use rusqlite::{params, Connection};

pub type DateTime = chrono::DateTime<chrono::FixedOffset>; //TODO use UTC, rusqlite has direct support for it

fn parse(s: &str) -> chrono::ParseResult<DateTime> {
    DateTime::parse_from_rfc3339(s)
}
fn to_string(d: &DateTime) -> String {
    d.to_rfc3339()
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
#[derive(Debug, Clone)]
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
    duration_secs  FLOAT,
    FOREIGN KEY(feedurl) REFERENCES feed
);
"#;
#[derive(Debug, Clone)]
pub struct Available {
    pub title: String,
    pub url: String,
    pub publication: DateTime,
    pub duration_secs: Option<f64>,
    pub feed: Feed,
}

const TABLE_DEFINITION_FEED: &'static str = r#"
CREATE TABLE IF NOT EXISTS feed (
    feedurl         TEXT PRIMARY KEY,
    title           TEXT NOT NULL,
    lastupdate      Text
);
"#;

#[derive(Debug, Clone)]
pub struct Feed {
    pub title: String,
    pub url: String,
    pub lastupdate: Option<DateTime>,
}

pub const TABLE_DEFINITIONS: &[&str] = &[
    TABLE_DEFINITION_FEED,
    TABLE_DEFINITION_AVAILABLE,
    TABLE_DEFINITION_ACTIVE,
];

/// Feed -----------------------------------------------------------------------
pub fn iter_feeds(conn: &Connection) -> Result<Vec<Feed>, rusqlite::Error> {
    let mut stmt = conn.prepare(
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
        .collect::<Result<Vec<_>, rusqlite::Error>>();
    res
}
pub fn add_to_feed(conn: &Connection, feed: &Feed) -> Result<(), rusqlite::Error> {
    conn.execute(
        r#"
        INSERT INTO feed (title, feedurl) VALUES (?1, ?2)
        "#,
        params!(feed.title, feed.url),
    )?;
    Ok(())
}

/// Available ------------------------------------------------------------------
pub fn iter_available(conn: &Connection) -> Result<Vec<Available>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        r#"
        SELECT available.title, url, publication, duration_secs, feedurl, feed.title, lastupdate
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
                duration_secs: row.get(3)?,
                feed: Feed {
                    url: row.get(4)?,
                    title: row.get(5)?,
                    lastupdate: row.get(6).map(|lastupdate: Option<String>| {
                        lastupdate.map(|lastupdate| parse(&lastupdate).unwrap())
                    })?,
                },
            })
        })?
        .collect::<Result<Vec<_>, rusqlite::Error>>();
    res
}

pub fn find_in_available(
    conn: &Connection,
    url: &str,
) -> Result<Option<Available>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        r#"
        SELECT available.title, url, publication, duration_secs, feedurl, feed.title, lastupdate
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
            duration_secs: row.get(3)?,
            feed: Feed {
                url: row.get(4)?,
                title: row.get(5)?,
                lastupdate: row.get(6).map(|lastupdate: Option<String>| {
                    lastupdate.map(|lastupdate| parse(&lastupdate).unwrap())
                })?,
            },
        })
    })?;
    let mut iter = res.into_iter();
    Ok(iter.next().transpose()?)
}

pub fn remove_from_available(conn: &Connection, url: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        r#"
        DELETE FROM available WHERE url = ?1
        "#,
        params!(url),
    )?;
    Ok(())
}

pub fn add_entry_to_available(
    conn: &Connection,
    feed: String,
    available: &crate::feeds::Entry,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        r#"
        INSERT INTO available (title, url, feedurl, publication) VALUES (?1, ?2, ?3, ?4)
        "#,
        params!(
            available.title,
            available.url,
            feed,
            to_string(&available.publication)
        ),
    )?;
    Ok(())
}

pub fn add_to_available(conn: &Connection, available: &Available) -> Result<(), rusqlite::Error> {
    conn.execute(
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

/// Active ---------------------------------------------------------------------

pub fn iter_active(conn: &Connection) -> Result<Vec<Active>, rusqlite::Error> {
    let mut stmt = conn.prepare(
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
        .collect::<Result<Vec<_>, rusqlite::Error>>();
    res
}

pub fn find_in_active(conn: &Connection, url: &str) -> Result<Option<Active>, rusqlite::Error> {
    let mut stmt = conn.prepare(
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

pub fn add_to_active(conn: &Connection, active: &Active) -> Result<(), rusqlite::Error> {
    conn.execute(
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

pub fn make_active(conn: &Connection, url: &str) -> Result<(), rusqlite::Error> {
    if let Some(available) = find_in_available(&conn, url)? {
        add_to_active(
            &conn,
            &Active {
                url: url.to_owned(),
                title: Some(available.title),
                position_secs: 0.0,
                duration_secs: available.duration_secs,
                feed_title: Some(available.feed.title),
            },
        )?;
        remove_from_available(&conn, url)
    } else {
        add_to_active(
            &conn,
            &Active {
                url: url.to_owned(),
                title: None,
                position_secs: 0.0,
                duration_secs: None,
                feed_title: None,
            },
        )
    }
}
pub fn set_position_secs(
    conn: &Connection,
    url: &str,
    position_secs: f64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        r#"
        UPDATE active SET position_secs = ?1 WHERE url = ?2
        "#,
        params!(position_secs, url),
    )?;
    Ok(())
}
pub fn set_duration(
    conn: &Connection,
    url: &str,
    duration_secs: f64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        r#"
        UPDATE active SET duration_secs = ?1 WHERE url = ?2
        "#,
        params!(duration_secs, url),
    )?;
    Ok(())
}
pub fn set_title(conn: &Connection, url: &str, title: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        r#"
        UPDATE active SET title = ?1 WHERE url = ?2
        "#,
        params!(title, url),
    )?;
    Ok(())
}
pub fn remove_from_active(conn: &Connection, url: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        r#"
        DELETE FROM active WHERE url = ?1
        "#,
        params!(url),
    )?;
    Ok(())
}

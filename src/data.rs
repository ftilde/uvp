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
    url           TEXT PRIMARY KEY,
    title          TEXT NOT NULL,
    playbackpos    FLOAT NOT NULL
);
"#;
#[derive(Debug, Clone)]
pub struct Active {
    pub title: String,
    pub url: String,
    pub playbackpos: f64,
}

const TABLE_DEFINITION_AVAILABLE: &'static str = r#"
CREATE TABLE IF NOT EXISTS available (
    title          TEXT NOT NULL,
    url            TEXT PRIMARY KEY,
    publication    TEXT NOT NULL,
    feedid         INTEGER,
    duration       FLOAT,
    FOREIGN KEY(feedid) REFERENCES feed
);
"#;
#[derive(Debug, Clone)]
pub struct Available {
    pub title: String,
    pub url: String,
    pub publication: DateTime,
    pub duration_secs: Option<f64>,
}

const TABLE_DEFINITION_FEED: &'static str = r#"
CREATE TABLE IF NOT EXISTS feed (
    feedid          INTEGER PRIMARY KEY AUTOINCREMENT,
    title           TEXT NOT NULL,
    url             TEXT NOT NULL,
    lastupdate      Text
);
"#;
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
pub fn iter_feeds(
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
                        lastupdate.map(|lastupdate| parse(&lastupdate).unwrap())
                    })?,
                },
            ))
        })?
        .collect();
    Ok(res)
}
pub fn add_to_feed(conn: &Connection, feed: &Feed) -> Result<(), rusqlite::Error> {
    conn.execute(
        r#"
        INSERT INTO feed (title, url) VALUES (?1, ?2)
        "#,
        params!(feed.title, feed.url),
    )?;
    Ok(())
}

/// Available ------------------------------------------------------------------
pub fn iter_available(conn: &Connection) -> Result<Vec<Available>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        r#"
        SELECT title, url, publication, duration FROM available
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
        SELECT title, url, publication FROM available
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

pub fn add_to_available(
    conn: &Connection,
    feedid: Option<u32>,
    available: &Available,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        r#"
        INSERT INTO available (title, url, feedid, publication) VALUES (?1, ?2, ?3, ?4)
        "#,
        params!(
            available.title,
            available.url,
            feedid,
            to_string(&available.publication)
        ),
    )?;
    Ok(())
}

/// Active ---------------------------------------------------------------------

pub fn iter_active(conn: &Connection) -> Result<Vec<Active>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        r#"
        SELECT title, url, playbackpos FROM active
        "#,
    )?;
    let res = stmt
        .query_map(params!(), |row| {
            Ok(Active {
                title: row.get(0)?,
                url: row.get(1)?,
                playbackpos: row.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, rusqlite::Error>>();
    res
}

pub fn find_in_active(conn: &Connection, url: &str) -> Result<Option<Active>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        r#"
        SELECT title, url, playbackpos FROM active WHERE url = ?1
        "#,
    )?;
    let res = stmt.query_map(params!(url), |row| {
        Ok(Active {
            title: row.get(0)?,
            url: row.get(1)?,
            playbackpos: row.get(2)?,
        })
    })?;
    let mut iter = res.into_iter();
    Ok(iter.next().transpose()?)
}

pub fn add_to_active(conn: &Connection, active: &Active) -> Result<(), rusqlite::Error> {
    conn.execute(
        r#"
        INSERT INTO active (url, title, playbackpos) VALUES (?1, ?2, ?3)
        "#,
        params!(active.url, active.title, active.playbackpos),
    )?;
    Ok(())
}

pub fn make_active(conn: &Connection, url: &str) -> Result<(), rusqlite::Error> {
    if let Some(available) = find_in_available(&conn, url)? {
        add_to_active(
            &conn,
            &Active {
                url: url.to_owned(),
                title: available.title,
                playbackpos: 0.0,
            },
        )?;
        remove_from_available(&conn, url)
    } else {
        add_to_active(
            &conn,
            &Active {
                url: url.to_owned(),
                title: url.to_owned(),
                playbackpos: 0.0,
            },
        )
    }
}
pub fn set_playbackpos(
    conn: &Connection,
    url: &str,
    playbackpos: f64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        r#"
        UPDATE active SET playbackpos = ?1 WHERE url = ?2
        "#,
        params!(playbackpos, url),
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

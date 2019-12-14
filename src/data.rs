use time::Tm;
pub const TIME_FORMAT_RFC3339: &'static str = "%Y-%m-%dT%H:%M:%S";

const TABLE_DEFINITION_AVAILABLE: &'static str = r#"
CREATE TABLE IF NOT EXISTS available (
    title          TEXT PRIMARY KEY,
    link           TEXT NOT NULL,
    publication    TEXT NOT NULL,
    feedid         INTEGER,
    FOREIGN KEY(feedid) REFERENCES feed
);
"#;
#[derive(Debug)]
pub struct Available {
    pub title: String,
    pub link: String,
    pub publication: Tm,
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
    pub lastupdate: Option<Tm>,
}

pub const TABLE_DEFINITIONS: &[&str] = &[TABLE_DEFINITION_AVAILABLE, TABLE_DEFINITION_FEED];

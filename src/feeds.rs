use atom_syndication;
use chrono::{DateTime, FixedOffset};
use rss;

use std::str::FromStr;

use crate::Error;

pub enum FeedEntries {
    Atom(Box<atom_syndication::Feed>),
    RSS(Box<rss::Channel>),
}

fn parse_time(s: &str) -> chrono::ParseResult<DateTime<FixedOffset>> {
    if let Ok(r) = DateTime::parse_from_rfc2822(s) {
        return Ok(r);
    }
    DateTime::parse_from_rfc3339(s)
}
#[derive(Debug, Clone)]
pub struct Entry {
    pub title: String,
    pub url: String,
    pub publication: crate::data::DateTime,
    pub duration_secs: Option<f64>,
}

impl FeedEntries {
    pub fn entries(&self) -> Vec<Entry> {
        match self {
            FeedEntries::Atom(f) => f.entries().iter().filter_map(entry_from_atom).collect(),
            FeedEntries::RSS(c) => c.items().iter().filter_map(entry_from_rss).collect(),
        }
    }
}

fn entry_from_atom(entry: &atom_syndication::Entry) -> Option<Entry> {
    Some(Entry {
        title: entry.title().to_owned(),
        url: entry.links().first()?.href().to_owned(),
        publication: parse_time(entry.published()?).unwrap(),
        duration_secs: None, //TODO
    })
}
fn entry_from_rss(entry: &rss::Item) -> Option<Entry> {
    Some(Entry {
        title: entry.title()?.to_owned(),
        url: entry.link()?.to_owned(),
        publication: parse_time(entry.pub_date()?).unwrap(),
        duration_secs: entry
            .itunes_ext()
            .and_then(|ext| ext.duration())
            .and_then(|s| str::parse::<f64>(s).ok()),
    })
}

fn parse(xml: &str) -> Result<FeedEntries, Error> {
    if let Ok(channel) = rss::Channel::from_str(&xml) {
        return Ok(FeedEntries::RSS(Box::new(channel)));
    }
    Ok(FeedEntries::Atom(Box::new(
        atom_syndication::Feed::from_str(&xml)?,
    )))
}

pub async fn fetch(client: &reqwest::Client, url: &str) -> Result<FeedEntries, Error> {
    let xml_resp = client.get(url).send().await?.text().await?;
    println!("Fetched from url: {}", url);
    Ok(parse(&xml_resp)?)
}

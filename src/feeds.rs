use crate::data::Available;
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

impl FeedEntries {
    pub fn entries(&self) -> Vec<Available> {
        match self {
            FeedEntries::Atom(f) => f.entries().iter().filter_map(entry_from_atom).collect(),
            FeedEntries::RSS(c) => c.items().iter().filter_map(entry_from_rss).collect(),
        }
    }
}

fn entry_from_atom(entry: &atom_syndication::Entry) -> Option<Available> {
    Some(Available {
        title: entry.title().to_owned(),
        link: entry.links().first()?.href().to_owned(),
        publication: parse_time(entry.published()?).unwrap(),
    })
}
fn entry_from_rss(entry: &rss::Item) -> Option<Available> {
    Some(Available {
        title: entry.title()?.to_owned(),
        link: entry.link()?.to_owned(),
        publication: parse_time(entry.pub_date()?).unwrap(),
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

pub fn fetch(url: &str) -> Result<FeedEntries, Error> {
    println!("Fetching from url: {}", url);
    let xml_resp = reqwest::get(url)?.text()?;
    //println!("Response: {}", xml_resp);
    Ok(parse(&xml_resp)?)
}

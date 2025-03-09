pub mod data;
pub mod feeds;

#[derive(Debug)]
pub enum Error {
    Reqwest(reqwest::Error),
    RSS(rss::Error),
    Atom(atom_syndication::Error),
    DB(rusqlite::Error),
}

impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        Error::Reqwest(error)
    }
}
impl From<rss::Error> for Error {
    fn from(error: rss::Error) -> Self {
        Error::RSS(error)
    }
}
impl From<atom_syndication::Error> for Error {
    fn from(error: atom_syndication::Error) -> Self {
        Error::Atom(error)
    }
}
impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        Error::DB(error)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

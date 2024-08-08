//! All write methods are disabled, but let's protect the keyring anyway

use std::fmt::{self, Display, Formatter};

pub struct Entry;

impl Entry {
    pub fn new(_: &str, _: &str) -> Result<Self, Error> {
        unimplemented!()
    }

    pub fn get_password(&self) -> Result<String, Error> {
        unimplemented!()
    }

    pub fn set_password(&self, _: &str) -> Result<(), Error> {
        unimplemented!()
    }
}

#[derive(Debug)]
pub enum Error {
    NoEntry,
}

impl Display for Error {
    fn fmt(&self, _: &mut Formatter) -> fmt::Result {
        unimplemented!()
    }
}

impl std::error::Error for Error {}

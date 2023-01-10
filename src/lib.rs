use std::{
    fmt::{Display, Write},
    ops::{Deref, DerefMut},
};

use once_cell::sync::Lazy;
use serde::Serialize;
use smallstr::SmallString;
use time::{format_description::FormatItem, macros::format_description, Date};

pub mod auth;
pub mod config;
pub mod course;
pub mod hist;
pub mod inter;
pub mod pace;
pub mod report;
pub mod store;
pub mod user;

#[allow(clippy::upper_case_acronyms)]
/// 16-byte backing store for `SmallString`s or `MiniString`s.
type SMALLSTORE = [u8; 16];
#[allow(clippy::upper_case_acronyms)]
/// 32-byte backing store for `SmallString`s or `MiniString`s.
type MEDSTORE = [u8; 32];
//type BIGSTORE = [u8; 64];
//type HUGESTORE = [u8; 128];
//type QUARTERKILO = [u8; 256];

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Format for `time::Date`s used for server-client interchange and also as a
/// display format in the Admin and Teacher views.
pub const DATE_FMT: &[FormatItem] = format_description!("[year]-[month]-[day]");

/**
The [`time`] crate offers no way to conveniently summon up a current [`Date`],
so we have a hack involving adding the time since the Epoch to the Epoch
in order to get the current date.
*/
static EPOCH: Lazy<Date> =
    Lazy::new(|| Date::from_calendar_date(1970, time::Month::January, 1).unwrap());

/// This error type implements [`From<E>`] for several types of other errors,
/// thus simplifing error propagation with `?`.
#[derive(Debug)]
pub enum UnifiedError {
    Postgres(tokio_postgres::error::Error),
    Auth(crate::auth::DbError),
    Data(crate::store::DbError),
    String(String),
}

impl From<tokio_postgres::error::Error> for UnifiedError {
    fn from(e: tokio_postgres::error::Error) -> Self {
        Self::Postgres(e)
    }
}
impl From<crate::auth::DbError> for UnifiedError {
    fn from(e: crate::auth::DbError) -> Self {
        Self::Auth(e)
    }
}
impl From<crate::store::DbError> for UnifiedError {
    fn from(e: crate::store::DbError) -> Self {
        Self::Data(e)
    }
}
impl From<String> for UnifiedError {
    fn from(e: String) -> Self {
        Self::String(e)
    }
}

impl Display for UnifiedError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Postgres(e) => write!(f, "Underlying database error: {}", e),
            Self::Auth(e) => write!(f, "Auth DB error: {}", e),
            Self::Data(e) => write!(f, "Data DB error: {}", e),
            Self::String(e) => write!(f, "Error: {}", e),
        }
    }
}

impl std::error::Error for UnifiedError {}

/**
This function is used for reading data from CSV files (and sometimes SQL
query results) where a blank value is better represented internally as
an `Option::None`.
*/
pub fn blank_string_means_none<S: AsRef<str>>(s: Option<S>) -> Option<S> {
    match s {
        None => None,
        Some(x) => match x.as_ref().trim() {
            "" => None,
            _ => Some(x),
        },
    }
}

/**
Return a [`Date`] representing the current day.

As the [`time`] crate doesn't offer a convenient way to do this directly, this
is kind of a hack, but it should work as long as the system's (or container's)
clock is set properly.
*/
pub fn now() -> time::Date {
    let since_epoch = std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .unwrap();
    let secs_since_epoch = since_epoch.as_secs() as i64;
    let duration_since_epoch = time::Duration::seconds(secs_since_epoch);
    EPOCH.saturating_add(duration_since_epoch)
}

/**
Return a value [`simplelog`] can use to set its log level by reading from
the `LOG_LEVEL` environment variable. From greatest to least volume of
logging messages, the possible values of `LOG_LEVEL` are

```text
max
trace
debug
info
warn
error
off
```

Any other value will be interpreted as `LevelFilter::Warn`.

*/
#[cfg(unix)]
pub fn log_level_from_env() -> simplelog::LevelFilter {
    use simplelog::LevelFilter;

    let mut level_string = match std::env::var("LOG_LEVEL") {
        Err(_) => {
            return LevelFilter::Warn;
        }
        Ok(s) => s,
    };

    level_string.make_ascii_lowercase();
    match level_string.as_str() {
        "max" => LevelFilter::max(),
        "trace" => LevelFilter::Trace,
        "debug" => LevelFilter::Debug,
        "info" => LevelFilter::Info,
        "warn" => LevelFilter::Warn,
        "error" => LevelFilter::Error,
        "off" => LevelFilter::Off,
        _ => LevelFilter::Warn,
    }
}

/**
Passing environment variables sucks in Windows, and this will never be
running in production (only in development) on a Windows platform, so
this just defaults to returning the maximum amount of logging when
called on Windows.
*/
#[cfg(windows)]
pub fn log_level_from_env() -> simplelog::LevelFilter {
    simplelog::LevelFilter::max()
}

/**
This just wraps a [`SmallString`] so we can implement [`std::io::Write`]
on it. This is necessary because the [`Date::format_into`] method requires
the target to be `std::io::Write`, and not just `std::fmt::Write`.
*/
#[derive(Debug, Serialize)]
pub struct MiniString<A: smallvec::Array<Item = u8>>(SmallString<A>);

impl<A: smallvec::Array<Item = u8>> MiniString<A> {
    /// Instantiate an empty `MiniString`.
    pub fn new() -> MiniString<A> {
        let inner: SmallString<A> = SmallString::new();
        MiniString(inner)
    }
}

impl<A: smallvec::Array<Item = u8>> Default for MiniString<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: smallvec::Array<Item = u8>> Deref for MiniString<A> {
    type Target = SmallString<A>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<A: smallvec::Array<Item = u8>> DerefMut for MiniString<A> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<A: smallvec::Array<Item = u8>> std::io::Write for MiniString<A> {
    fn write(&mut self, buff: &[u8]) -> std::io::Result<usize> {
        use std::io::{Error, ErrorKind};

        let str_buff = match std::str::from_utf8(buff) {
            Ok(s) => s,
            Err(_) => {
                return Err(Error::new(ErrorKind::InvalidData, "not valid UTF-8"));
            }
        };

        match self.0.write_str(str_buff) {
            Ok(()) => Ok(buff.len()),
            Err(_) => Err(Error::new(ErrorKind::Other, "formatting failed")),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<A: smallvec::Array<Item = u8>> Display for MiniString<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        self.deref().fmt(f)
    }
}

impl<A: smallvec::Array<Item = u8>> From<&str> for MiniString<A> {
    fn from(s: &str) -> MiniString<A> {
        let ss: SmallString<A> = SmallString::from_str(s);
        MiniString(ss)
    }
}

pub fn format_date(format: &[FormatItem], date: &Date) -> Result<MiniString<SMALLSTORE>, String> {
    let mut s: MiniString<SMALLSTORE> = MiniString::new();
    date.format_into(&mut s, format)
        .map_err(|e| format!("Failed to format date {:?}: {}", date, &e))?;
    Ok(s)
}

pub fn format_maybe_date(
    format: &[FormatItem],
    maybe_date: &Option<Date>,
) -> Result<MiniString<SMALLSTORE>, String> {
    match maybe_date {
        Some(d) => format_date(format, d),
        None => Ok(MiniString::new()),
    }
}

pub fn academic_year_from_start_year(year: i32) -> MiniString<SMALLSTORE> {
    let mut years: MiniString<SMALLSTORE> = MiniString::new();
    match year {
        0 => { write!(&mut years, "0000--0000").unwrap(); },
        n => { write!(&mut years, "{}--{}", n, n + 1).unwrap(); },
    }
    years
}

pub fn academic_year_from_start_date(d: &Date) -> MiniString<SMALLSTORE> {
    academic_year_from_start_year(d.year())
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn ensure_logging() {
        use simplelog::{ColorChoice, TermLogger, TerminalMode};
        let log_cfg = simplelog::ConfigBuilder::new()
            .add_filter_allow_str("camp")
            .build();
        let res = TermLogger::init(
            log_level_from_env(),
            log_cfg,
            TerminalMode::Stdout,
            ColorChoice::Auto,
        );

        match res {
            Ok(_) => {
                log::info!("Test logging started.");
            }
            Err(_) => {
                log::info!("Test logging already started.");
            }
        }
    }
}

/*!
Student course completion history.
*/
use std::cmp::{Ord, Ordering, PartialOrd};

use serde::{Deserialize, Serialize};

use crate::pace::Term;

#[derive(Debug, Eq, Deserialize, PartialEq, Serialize)]
pub struct HistEntry {
    pub sym: String,
    pub year: i32,
    pub term: Term,
}

impl Ord for HistEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.year.cmp(&other.year) {
            Ordering::Equal => self.term.cmp(&other.term),
            x => x,
        }
    }
}

impl PartialOrd for HistEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct CompletionHistory {
    pub uname: String,
    pub hist: Vec<HistEntry>
}
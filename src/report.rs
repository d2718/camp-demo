/*!
Internal representations and containers for data only necessary for
report-writing season.
*/
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{blank_string_means_none, pace::Term};

/// Represents the "mastery" status of a Goal in a report.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub enum MasteryStatus {
    Not,
    Mastered,
    Retained,
}

impl MasteryStatus {
    pub fn as_sql(&self) -> Option<&'static str> {
        match self {
            MasteryStatus::Not => None,
            MasteryStatus::Mastered => Some("M"),
            MasteryStatus::Retained => Some("R"),
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            MasteryStatus::Not => "Not Mastered",
            MasteryStatus::Mastered => "Mastered",
            MasteryStatus::Retained => "Mastered & Retained",
        }
    }
}

impl std::convert::TryFrom<Option<&str>> for MasteryStatus {
    type Error = String;

    fn try_from(opt: Option<&str>) -> Result<Self, Self::Error> {
        match blank_string_means_none(opt) {
            None => Ok(Self::Not),
            Some("M") => Ok(Self::Mastered),
            Some("R") => Ok(Self::Retained),
            Some(s) => Err(format!("{:?} is not a valid MasteryStatus.", s)),
        }
    }
}

/// For transfer of [`Goal`] mastery status information to/from the frontend.
#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct Mastery {
    // `id` of `Goal`
    pub id: i64,
    pub status: MasteryStatus,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(from = "&str", into = "&str")]
pub enum FactStatus {
    Not,
    Mastered,
    Excused,
}

impl std::convert::Into<&str> for FactStatus {
    fn into(self) -> &'static str {
        use FactStatus::*;
        match self {
            Not => "Not",
            Mastered => "Mastered",
            Excused => "Ex",
        }
    }
}

impl std::convert::From<&str> for FactStatus {
    fn from(s: &str) -> Self {
        use FactStatus::*;
        match s {
            "Not" | "not" | "NOT" => Not,
            "Mastered" | "mastered" | "MASTERED" => Mastered,
            "Ex" | "ex" | "EX" | "Excused" | "excused" | "EXCUSED" => Excused,
            _ => Not,
        }
    }
}

impl FactStatus {
    pub fn as_str(&self) -> &'static str {
        let s: &str = (*self).into();
        s
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
pub struct FactSet {
    pub add: FactStatus,
    pub sub: FactStatus,
    pub mul: FactStatus,
    pub div: FactStatus,
}

impl std::default::Default for FactSet {
    fn default() -> Self {
        use FactStatus::*;
        FactSet {
            add: Not,
            sub: Not,
            mul: Not,
            div: Not,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ReportSidecar {
    pub uname: String,
    pub facts: Option<FactSet>,
    pub fall_social: HashMap<String, String>,
    pub spring_social: HashMap<String, String>,
    pub fall_complete: String,
    pub spring_complete: String,
    pub summer_complete: Option<String>,
    pub mastery: Vec<Mastery>,
}

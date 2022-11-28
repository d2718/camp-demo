/*!
The `Goal` struct and `Pace` calendars, for internally represending pace
calendar information.
*/
use std::{
    cmp::{Ord, Ordering, PartialOrd},
    collections::HashMap,
    io::{Read, Write},
};

use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use time::{Date, Month};

use crate::{
    config::Glob,
    user::{Student, Teacher, User},
    MiniString, MEDSTORE,
};

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub enum Term {
    Fall,
    Spring,
    Summer,
}

impl Term {
    pub fn as_str(&self) -> &'static str {
        match self {
            Term::Fall => "Fall",
            Term::Spring => "Spring",
            Term::Summer => "Summer",
        }
    }
}

impl std::fmt::Display for Term {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for Term {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Fall" | "fall" => Ok(Term::Fall),
            "Spring" | "spring" => Ok(Term::Spring),
            "Summer" | "summer" => Ok(Term::Summer),
            _ => Err(format!("{:?} is not a valid Term.", s)),
        }
    }
}

/**
Attempt to interpret a [`&str`] that might represent a grade or a score
as a value in the range [0.0, 1.0] (or possibly greater than 1.0 if the
student has earned extra credit).

"Scores" are saved (in the database) verbatim as they are entered by the
teacher. This attempts to turn a string of characters into a
floating-point "out of one" fractional score. It will attempt to interpret
a score `&str` in one of three different ways, according to the following
criteria:

  * If the string contains a `/` character, it attempts to interpret the
    value as having a numerator (to the left of the `/`) and a denominator
    (to the right of the `/`).

Without a `/`, it just attempts to read a single (floating-point) number.

  * If that number is less than 2.0, it just interprets it directly as a
    "fraction out of one".
  * If that number is greater than 2.0, it interprets it as a percentage
    (and so divides it by 100.0).

```
# use camp::pace::parse_score_str;
// fractional interpretation
assert_eq!(parse_score_str("9/10"), Ok(0.9));
// fractional interpretation
assert_eq!(parse_score_str("18.5 / 20"), Ok(0.925));
// direct value interpretation
assert_eq!(parse_score_str("0.82"), Ok(0.82));
// percentage interpretation
assert_eq!(parse_score_str("95"), Ok(0.95));

```
*/
pub fn parse_score_str(score_str: &str) -> Result<f32, String> {
    let chunks: SmallVec<[f32; 2]> = score_str
        .split('/')
        .take(2)
        .map(|s| s.trim().parse::<f32>())
        .filter(|res| res.is_ok())
        .map(|ok| ok.unwrap())
        .collect();

    match chunks[..] {
        [n, d] => {
            if d.abs() < 0.001 {
                Err("Danger of division by zero.".to_string())
            } else {
                Ok(n / d)
            }
        }
        [x] => {
            if x.abs() > 2.0 {
                Ok(x / 100.0)
            } else {
                Ok(x)
            }
        }
        _ => Err(format!("Unable to parse: {:?} as score.", score_str)),
    }
}

/// Similar to [`parse_score_str`], but operates on (and returns) an `Option`.
pub fn maybe_parse_score_str(score_str: Option<&str>) -> Result<Option<f32>, String> {
    match score_str {
        Some(score_str) => match parse_score_str(score_str) {
            Ok(x) => Ok(Some(x)),
            Err(e) => Err(e),
        },
        None => Ok(None),
    }
}

/// Represents a single chapter's worth of source material from a `Course`
/// extant in the database.
#[derive(Clone, Debug)]
pub struct BookCh {
    /// The [`Course`](crate::course::Course) symbol of the course to which
    /// this `Chapter` belongs.
    pub sym: String,
    /// The `Chapter`'s order in the sequence of Chapters in the course
    /// (Chapter 1, 2, etc.).
    pub seq: i16,
    // Gets set in the constructor of the `Pace` calendar.
    pub level: f32,
}

impl PartialEq for BookCh {
    fn eq(&self, other: &Self) -> bool {
        self.sym == other.sym && self.seq == other.seq
    }
}
impl Eq for BookCh {}

/// Represents material for a "custom" goal (not from an extant Course in
/// the database.) This is currently not supported.
///
/// `id` would be the value of the database's primary key from the table of
/// custom goals (if it existed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CustomCh(i64);

/// Currently, only `Source::Book` values are supported, and trying to do
/// anything wtih a `Source::Custom` will yield you an error.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Source {
    Book(BookCh),
    Custom(CustomCh),
}

/// Internal representation of a single student's single pace goal.
#[derive(Clone, Debug)]
pub struct Goal {
    /// Database table primary key.
    pub id: i64,
    /// `uname` of Student to whom this `Goal` is assigned.
    pub uname: String,
    /// Source of the material.
    pub source: Source,
    /// Whether the material in question is review.
    pub review: bool,
    /// Whether the material in question is incomplete from a prior academic year.
    pub incomplete: bool,
    /// `Goal`'s due date. `Goal`s not part of a student's assigned pace (for
    /// example, extra chapters a student completes after completing their
    /// assigned Goals) will have due dates of `None`.
    pub due: Option<Date>,
    /// The date the `Goal` is finished. As-of-yet unfinished `Goal`s will have
    /// done dates of `None`.
    pub done: Option<Date>,
    /// The number of tries the student took to successfully display mastery
    /// of the material. As-of-yet unfinished `Goal`s will have `tries`
    /// values of `None`.
    pub tries: Option<i16>,
    /// Weight of the `Goal` relative to the entire weight of the course of
    /// which it's a part. (If a student's assigned pace for the year consists
    /// of exactly all the `Chapter`s of a single course, their weights should
    /// sum to exactly 1.0).
    ///
    /// Should get set in the constructor of the `Pace` calendar.
    pub weight: f32,
    /// Score string of a completed Goal (see [`parse_score_str`]).
    /// As-of-yet unfinished `Goal`s will have scores of `None`.
    pub score: Option<String>,
}

impl PartialEq for Goal {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
            && self.uname == other.uname
            && self.source == other.source
            && self.review == other.review
            && self.incomplete == other.incomplete
            && self.due == other.due
            && self.done == other.done
            && self.tries == other.tries
            && self.score == other.score
    }
}

impl Eq for Goal {}

/// Translate `Some("")` to `None`.
fn blank_means_none(s: Option<&str>) -> Option<&str> {
    match s {
        Some(s) => match s.trim() {
            "" => None,
            x => Some(x),
        },
        None => None,
    }
}

impl Goal {
    /**
    Goal .csv rows should look like this

    ```csv
    #uname, sym, seq,     y, m,  d, rev, inc
    jsmith, pha1,  3, 2022, 09, 10,   x,
          ,     ,  9,     ,   , 28,    ,  x
    ```

    Columns `uname`, `sym`, `y`, `m` all default to the value of the previous
    goal, so to save work, you don't need to include them if they're the same
    as the previous line.

    Columns `rev` and `inc` are considered `true` if they have any text
    whatsoever.
     */
    pub fn from_csv_line(row: &csv::StringRecord, prev: Option<&Goal>) -> Result<Goal, String> {
        log::trace!("Goal::from_csv_line( {:?} ) called.", row);

        let uname = match blank_means_none(row.get(0)) {
            Some(s) => s.to_owned(),
            None => match prev {
                Some(g) => g.uname.clone(),
                None => {
                    return Err("No uname".into());
                }
            },
        };

        let seq: i16 = match blank_means_none(row.get(2)) {
            Some(s) => match s.parse() {
                Ok(n) => n,
                Err(_) => {
                    return Err(format!("Unable to parse {:?} as number.", s));
                }
            },
            None => {
                return Err("No chapter number.".into());
            }
        };

        let book_ch = match blank_means_none(row.get(1)) {
            Some(s) => BookCh {
                sym: s.to_owned(),
                seq,
                level: 0.0,
            },
            None => match prev {
                Some(g) => match &g.source {
                    Source::Book(bch) => BookCh {
                        sym: bch.sym.clone(),
                        seq,
                        level: 0.0,
                    },
                    Source::Custom(_) => {
                        return Err("No course symbol.".into());
                    }
                },
                None => {
                    return Err("No course symbol".into());
                }
            },
        };

        let y: i32 = match blank_means_none(row.get(3)) {
            Some(s) => match s.parse() {
                Ok(n) => n,
                Err(_) => {
                    return Err(format!("Unable to parse {:?} as year.", s));
                }
            },
            None => match prev {
                Some(g) => match g.due {
                    Some(d) => d.year(),
                    None => {
                        return Err("No year".into());
                    }
                },
                None => {
                    return Err("No year".into());
                }
            },
        };

        let m: Month = match blank_means_none(row.get(4)) {
            Some(s) => match s.parse::<u8>() {
                Ok(n) => match Month::try_from(n) {
                    Ok(m) => m,
                    Err(_) => {
                        return Err(format!("Not an appropriate Month value: {}", n));
                    }
                },
                Err(_) => {
                    return Err(format!("Unable to parse {:?} as month number.", s));
                }
            },
            None => match prev {
                Some(g) => match g.due {
                    Some(d) => d.month(),
                    None => {
                        return Err("No month".into());
                    }
                },
                None => {
                    return Err("No month".into());
                }
            },
        };

        let d: u8 = match blank_means_none(row.get(5)) {
            Some(s) => match s.parse() {
                Ok(n) => n,
                Err(_) => {
                    return Err(format!("Unable to parse {:?} as day number.", s));
                }
            },
            None => match prev {
                Some(g) => match g.due {
                    Some(d) => d.day(),
                    None => {
                        return Err("No day".into());
                    }
                },
                None => {
                    return Err("No day".into());
                }
            },
        };

        let due = match Date::from_calendar_date(y, m, d) {
            Ok(d) => d,
            Err(_) => {
                return Err(format!("{}-{}-{} is not a valid date", &y, &m, &d));
            }
        };

        let review = blank_means_none(row.get(6)).is_some();
        let incomplete = blank_means_none(row.get(7)).is_some();

        let g = Goal {
            // This doesn't matter; it will be set upon database insertion.
            id: 0,
            uname,
            source: Source::Book(book_ch),
            review,
            incomplete,
            due: Some(due),
            // No goals read from .csv files can possibly be done.
            done: None,
            // Will get set once it's done.
            tries: None,
            // Will get set in the `Pace` calendar constructror.
            weight: 0.0,
            // Goals read from .csv files should have no score yet.
            score: None,
        };

        Ok(g)
    }
}

impl Ord for Goal {
    fn cmp(&self, other: &Self) -> Ordering {
        use Ordering::*;

        match &self.due {
            Some(d) => match &other.due {
                Some(e) => {
                    return d.cmp(e);
                }
                None => {
                    return Less;
                }
            },
            None => match &other.due {
                Some(_) => return Greater,
                None => { /* fallthrough */ }
            },
        }

        match &self.done {
            Some(d) => match &other.done {
                Some(e) => {
                    return d.cmp(e);
                }
                None => {
                    return Less;
                }
            },
            None => match &other.done {
                Some(_) => {
                    return Greater;
                }
                None => { /* fallthrough */ }
            },
        }

        match &self.source {
            Source::Book(BookCh {
                sym: _,
                seq: n,
                level: slev,
            }) => match &other.source {
                Source::Book(BookCh {
                    sym: _,
                    seq: m,
                    level: olev,
                }) => {
                    if slev < olev {
                        Less
                    } else if slev > olev {
                        Greater
                    } else {
                        n.cmp(m)
                    }
                }
                _ => Equal,
            },
            _ => Equal,
        }
    }
}

impl PartialOrd for Goal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Represents a student's entire assigned pace for one year.
#[derive(Debug)]
pub struct Pace {
    /// A copy of the [`Student`] data to whom this `Pace` is assigned.
    pub student: Student,
    /// A copy of the student's [`Teacher`] data.
    pub teacher: Teacher,
    /// The pace [`Goal`]s the student has assigned to them.
    pub goals: Vec<Goal>,
    /// Sum of the weights of all the _assigned_ `Goal`s (that is, those
    /// with `Some` due dates).
    pub total_weight: f32,
    /// Sum of the weights of the `Goal`s whose due dates have passed.
    pub due_weight: f32,
    /// Some of the weights of the so-far-completed `Goal`s (that is, those
    /// with `Some` done dates.)
    pub done_weight: f32,
}

fn affirm_goal(mut g: Goal, glob: &Glob) -> Result<Goal, String> {
    match glob.users.get(&g.uname) {
        Some(User::Student(_)) => { /* This is the happy path. */ }
        _ => {
            return Err(format!("{:?} is not a student user name.", &g.uname));
        }
    }

    match g.source {
        Source::Book(ref mut b) => {
            let crs = match glob.course_by_sym(&b.sym) {
                Some(c) => c,
                None => {
                    return Err(format!("{:?} is not a course symbol.", &b.sym));
                }
            };
            let chp = match crs.chapter(b.seq) {
                Some(ch) => ch,
                None => {
                    return Err(format!(
                        "Course {:?} ({}) does not have a chapter {}.",
                        &b.sym, &crs.title, b.seq
                    ));
                }
            };
            b.level = crs.level;
            let crs_wgt = match crs.weight {
                Some(w) => w,
                None => {
                    return Err(format!(
                    "Course {:?} has not been appropriately initialized! This is a bad bug. Make Dan fix it.",
                    &b.sym
                ));
                }
            };
            g.weight = chp.weight / crs_wgt;
        }
        Source::Custom(_) => {
            return Err("Custom Goals not yet supported.".to_owned());
        }
    }

    Ok(g)
}

impl Pace {
    /// Instantiate a new `Pace` calendar.
    pub fn new(s: Student, t: Teacher, mut goals: Vec<Goal>, glob: &Glob) -> Result<Pace, String> {
        log::trace!(
            "Pace::new( [ Student {:?} ], [ Teacher {:?} ], [ {} Goals ] ) called.",
            &s.base.uname,
            &t.base.uname,
            &goals.len()
        );

        goals.sort();
        let now = crate::now();

        let mut total_weight: f32 = 0.0;
        let mut due_weight: f32 = 0.0;
        let mut done_weight: f32 = 0.0;
        for g in goals.iter_mut() {
            let source = match &mut g.source {
                Source::Book(bch) => bch,
                _ => {
                    return Err("Custom chapters not supported.".into());
                }
            };
            let crs = match glob.course_by_sym(&source.sym) {
                Some(crs) => crs,
                None => {
                    return Err(format!("Unknown course symbol {:?}", &source.sym));
                }
            };
            let chp = match crs.chapter(source.seq) {
                Some(chp) => chp,
                None => {
                    return Err(format!(
                        "Course {:?} ({}) doesn't have a chapter {}.",
                        &source.sym, &crs.title, &source.seq
                    ));
                }
            };

            let weight = match crs.weight {
                Some(w) => chp.weight / w,
                None => {
                    return Err(format!(
                        "Course {:?} ({}) has not had its weights set.",
                        &source.sym, &crs.title
                    ));
                }
            };

            source.level = crs.level;
            g.weight = weight;
            if let Some(due_date) = &g.due {
                total_weight += weight;
                if due_date < &now {
                    due_weight += weight;
                }
            }
            if g.done.is_some() {
                done_weight += weight;
            }
        }

        let p = Pace {
            student: s,
            teacher: t,
            goals,
            total_weight,
            due_weight,
            done_weight,
        };

        log::debug!("{:#?}", &p);

        Ok(p)
    }

    /**
    Read a series of goals from data in CSV format and return them as a `Vec`
    of `Pace`s.

    Goal .csv rows should look like this

    ```csv
    #uname, sym, seq,     y, m,  d, rev, inc
    jsmith, pha1,  3, 2022, 09, 10,   x,
          ,     ,  9,     ,   , 28,    ,  x
    ```

    Columns `uname`, `sym`, `y`, `m`, `d` all default to the value of the
    previous goal, so to save work, you don't need to include them if they're
    the same as the previous line.

    Columns `rev` and `inc` are considered `true` if they have any text
    whatsoever.
     */
    pub fn from_csv<R: Read>(r: R, glob: &Glob) -> Result<Vec<Pace>, String> {
        log::trace!("Pace::from_csv(...) called.");

        let mut csv_reader = csv::ReaderBuilder::new()
            .comment(Some(b'#'))
            .trim(csv::Trim::All)
            .flexible(true)
            .has_headers(false)
            .from_reader(r);

        let mut goals_by_uname: HashMap<String, Vec<Goal>> = HashMap::new();

        let mut prev_goal: Option<Goal> = None;
        for (n, res) in csv_reader.records().enumerate() {
            match res {
                Ok(record) => {
                    // If all the fields in a record are blank, we skip it.
                    let mut is_blank = true;
                    for r in record.iter() {
                        if !r.is_empty() {
                            is_blank = false;
                            break;
                        }
                    }
                    if is_blank {
                        continue;
                    }

                    let res = Goal::from_csv_line(&record, prev_goal.as_ref());
                    match res {
                        Ok(g) => match affirm_goal(g, glob) {
                            Ok(g) => {
                                if let Some(v) = goals_by_uname.get_mut(&g.uname) {
                                    (*v).push(g.clone());
                                } else {
                                    let v = vec![g.clone()];
                                    goals_by_uname.insert(g.uname.clone(), v);
                                }
                                prev_goal = Some(g)
                            }
                            Err(e) => {
                                let estr = match record.position() {
                                    Some(p) => format!("Error on line {}: {}", p.line(), &e),
                                    None => format!("Error in CSV record {}: {}", &n, &e),
                                };
                                return Err(estr);
                            }
                        },
                        Err(e) => {
                            let estr = match record.position() {
                                Some(p) => format!("Error on line {}: {}", p.line(), &e),
                                None => format!("Error in CSV record {}: {}", &n, &e),
                            };
                            return Err(estr);
                        }
                    }
                }
                Err(e) => {
                    let estr = match e.position() {
                        Some(p) => format!("Error on line {}: {}", p.line(), &e),
                        None => format!("Error in CSV record {}: {}", &n, &e),
                    };
                    return Err(estr);
                }
            }
        }

        let mut cals: Vec<Pace> = Vec::with_capacity(goals_by_uname.len());
        for (uname, mut goals) in goals_by_uname.drain() {
            let student = match glob.users.get(&uname) {
                Some(User::Student(s)) => s.clone(),
                _ => {
                    return Err(format!("{:?} is not a Student user name.", &uname));
                }
            };
            let teacher = match glob.users.get(&student.teacher) {
                Some(User::Teacher(t)) => t.clone(),
                _ => {
                    return Err(format!(
                        "Student {:?} ({} {}) has nonexistent teachdr {:?} on record.",
                        &uname, &student.rest, &student.last, &student.teacher
                    ));
                }
            };

            goals.sort();
            let total_weight = goals.iter().map(|g| g.weight).sum();

            let p = Pace {
                student,
                teacher,
                goals,
                total_weight,
                due_weight: 0.0,
                done_weight: 0.0,
            };

            cals.push(p);
        }

        Ok(cals)
    }

    /// Given an academic calendar represented by a (sorted, duh) slice of
    /// [`Date`]s, distribute this `Pace`'s due dates throughout the year,
    /// proportionally according to the weights of the `Goal`s.
    pub fn autopace(&mut self, dates: &[Date]) -> Result<(), String> {
        log::trace!(
            "Pace[ {:?} ]::autopace( [ {} dates ] ) called.",
            &self.student.base.uname,
            &dates.len()
        );

        if dates.is_empty() {
            return Err("You require 1 or more Dates in order to autopace a Pace calendar.".into());
        }
        let total_n_due = self.goals.iter().filter(|g| g.due.is_some()).count();
        if total_n_due < 2 {
            return Err("You require at least 2 Goals with due dates in order to autopace.".into());
        }

        // This is really to prevent division by zero.
        if self.total_weight < 0.001 {
            return Err(
                "This student doesn't have enough material with due dates to autopace.".into(),
            );
        }

        let mut running_weight: f32 = 0.0;
        let n_dates: f32 = dates.len() as f32;
        for g in self.goals.iter_mut() {
            if let Some(d) = &mut g.due {
                running_weight += g.weight;
                let frac = running_weight / self.total_weight;
                let idx = (n_dates * frac).ceil() as usize;
                let due = dates[idx - 1];
                *d = due;
            }
        }

        Ok(())
    }
}

/**
Represents the state of the `Goal` on the current day:
  * `Done`: completed before the due date
  * `Late`: completed after the due date
  * `Overdue`: The due date has passed, but the goal is still uncompleted.
  * `Yet`: The goal is uncompleted, but the due date is also still in the future.
*/
#[derive(Debug)]
pub enum GoalStatus {
    Done,
    Late,
    Overdue,
    Yet,
}

/**
All the calculated data necessary to display a [`Goal`] and its current
status.

This is an abstraction meant to be used in several different contexts which
each display the `Goal`'s data slightly differently.
*/
#[derive(Debug)]
pub struct GoalDisplay<'a> {
    /// The ID of the goal.
    pub id: i64,
    /// Title of the [`Course`](crate::course::Course) to which this `Goal` belongs.
    pub course: &'a str,
    /// Title of the textbook (or other source) form which this material
    /// is drawn.
    pub book: &'a str,
    /// Title of the Chapter or section containing the material covered
    /// by this `Goal` (probably "Chapter N").
    pub title: &'a str,
    /// Material covered (if this information is available).
    pub subject: Option<&'a str>,
    /// Whether the `Goal` in question is a review of previously-covered material.
    pub rev: bool,
    /// Whether this `Goal` represents material incomplete from a prior academic year.
    pub inc: bool,
    /// When the `Goal` is due (if it's due).
    pub due: Option<Date>,
    /// When the `Goal` was completed (if it's complete).
    pub done: Option<Date>,
    /// How many attempts it took to display mastery (if it's complete).
    pub tries: Option<i16>,
    /// The string of characters the teacher has used to represent the score
    /// achieved on this `Goal` (if it's complete).
    pub mark: MiniString<MEDSTORE>,
    /// The value of that string of characters (if it's complete).
    pub score: Option<f32>,
    /// The status of this `Goal` on the current date.
    pub status: GoalStatus,
}

impl<'a> GoalDisplay<'a> {
    /// Generate all the information necessary to display the given [`Goal`].
    fn from_goal(g: &'a Goal, glob: &'a Glob, today: &Date) -> Result<GoalDisplay<'a>, String> {
        let bch = match &g.source {
            Source::Book(bch) => bch,
            _ => {
                return Err(format!("Goal {}: custom sources unsupported.", &g.id));
            }
        };

        let crs = glob
            .course_by_sym(&bch.sym)
            .ok_or_else(|| format!("Goal {}: no course with symbol {:?}.", &g.id, &bch.sym))?;
        let chp = crs.chapter(bch.seq).ok_or_else(|| {
            format!(
                "Goal {}: Course {:?} has no Chapter {}",
                &g.id, &bch.sym, &bch.seq
            )
        })?;

        let mut mark: MiniString<MEDSTORE> = MiniString::new();
        if let Some(s) = g.score.as_deref() {
            write!(&mut mark, "{}", s)
                .map_err(|e| format!("Error writing goal mark {:?}: {}", s, &e))?;
        }

        let score = maybe_parse_score_str(g.score.as_deref())?;

        let status = if let Some(due) = &g.due {
            if let Some(done) = &g.done {
                if done > due {
                    GoalStatus::Late
                } else {
                    GoalStatus::Done
                }
            } else if today > due {
                GoalStatus::Overdue
            } else {
                GoalStatus::Yet
            }
        } else if g.done.is_some() {
            GoalStatus::Done
        } else {
            GoalStatus::Yet
        };

        let gd = GoalDisplay {
            id: g.id,
            course: crs.title.as_str(),
            book: crs.book.as_str(),
            title: chp.title.as_str(),
            subject: chp.subject.as_deref(),
            rev: g.review,
            inc: g.incomplete,
            due: g.due,
            done: g.done,
            tries: g.tries,
            mark,
            score,
            status,
        };

        Ok(gd)
    }
}

/// A single line (of possibly several) in a semester summary of a student's
/// progress.
#[derive(Debug, Serialize)]
pub struct SummaryDisplay {
    pub label: &'static str,
    pub value: MiniString<MEDSTORE>,
}

/// Represents a single row of data to display in a `Pace` calendar display.
///
/// This could either be `Goal` information, or a line of semester summary info.
#[derive(Debug)]
pub enum RowDisplay<'a> {
    Goal(GoalDisplay<'a>),
    Summary(SummaryDisplay),
}

/**
All the information necessary to display a `[Pace`] calendar to a user,
without a bunch (or at least a bunch _more_) calculations or hash lookups.

This is an abstraction meant to be used in several different situations in
which these data are displayed in slightly different ways.
*/
#[derive(Debug)]
pub struct PaceDisplay<'a> {
    pub uname: &'a str,
    pub email: &'a str,
    pub last: &'a str,
    pub rest: &'a str,
    pub tuname: &'a str,
    pub teacher: &'a str,
    pub temail: &'a str,
    pub previously_inc: bool,
    pub semf_inc: bool,
    pub sems_inc: bool,
    pub has_review_chapters: bool,
    pub has_incomplete_chapters: bool,
    pub weight_due: f32,
    pub weight_done: f32,
    pub weight_scheduled: f32,
    pub n_due: usize,
    pub n_done: usize,
    pub n_scheduled: usize,
    pub fall_due: usize,
    pub fall_done: usize,
    pub spring_due: usize,
    pub spring_done: usize,
    pub fall_notices: i16,
    pub spring_notices: i16,
    pub fall_tests: f32,
    pub spring_tests: f32,
    pub fall_exam_frac: f32,
    pub spring_exam_frac: f32,
    pub fall_exam: Option<f32>,
    pub spring_exam: Option<f32>,
    pub fall_total: Option<f32>,
    pub spring_total: Option<f32>,
    /// The index in the `rows` vector of the most-recently-completed goal.
    pub last_completed_goal: Option<usize>,

    pub rows: Vec<RowDisplay<'a>>,
}

/// Generate semester summary lines (if necessary).
///
/// Produces 0-4 lines, depending on what the student has done (or at
/// least what information is available about what the student has done).
///
/// This shouldn't be called for the Summer term.
fn generate_summary(
    term: Term,
    sem_frac: f32,
    n_notices: i16,
    exam_frac: f32,
    exam_score: Option<f32>,
    sem_inc: bool,
) -> Result<SmallVec<[SummaryDisplay; 4]>, String> {
    log::trace!(
        "generate_summary( {:?}, {}, {}, {}, {:?}) called.",
        &term,
        &sem_frac,
        &n_notices,
        &exam_frac,
        &exam_score
    );

    let mut lines: SmallVec<[SummaryDisplay; 4]> = SmallVec::new();

    let int_score = (sem_frac * 100.0).round() as i32;
    let label = match term {
        Term::Fall => "Fall Test Average",
        Term::Spring => "Spring Test Average",
        // This shouldn't be called for the Summer term, so just return an
        // empty Vec of rows.
        Term::Summer => {
            return Ok(lines);
        }
    };
    let mut value: MiniString<MEDSTORE> = MiniString::new();
    write!(&mut value, "{}", &int_score)
        .map_err(|e| format!("Error writing score {:?}: {}", &int_score, &e))?;
    let line = SummaryDisplay { label, value };
    lines.push(line);

    if let Some(f) = exam_score {
        let int_score = (100.0 * f).round() as i32;
        let label = "Exam Score";
        let mut value: MiniString<MEDSTORE> = MiniString::new();
        write!(&mut value, "{}", &int_score)
            .map_err(|e| format!("Error writing exam score {:?}: {}", &int_score, &e))?;
        let line = SummaryDisplay { label, value };
        lines.push(line);

        let sem_final = (exam_frac * f) + ((1.0 - exam_frac) * sem_frac);
        let mut sem_pct = 100.0 * sem_final;

        if n_notices > 0 {
            let label = "Notices";
            let mut value: MiniString<MEDSTORE> = MiniString::new();
            write!(&mut value, "-{}", &n_notices)
                .map_err(|e| format!("Error writing # notices {:?}: {}", &n_notices, &e))?;
            let line = SummaryDisplay { label, value };
            lines.push(line);

            sem_pct -= n_notices as f32;
        }

        let int_pct = sem_pct.round() as i32;
        let label = match term {
            Term::Fall => "Fall Semester Grade",
            Term::Spring => "Spring Semester Grade",
            _ => unreachable!(),
        };
        let mut value: MiniString<MEDSTORE> = MiniString::new();
        write!(&mut value, "{}", &int_pct)
            .map_err(|e| format!("Error writing semester grade {:?}: {}", &int_pct, &e))?;
        if sem_inc {
            write!(&mut value, " (I)")
                .map_err(|e| format!("Error writing semester grade: {}", &e))?;
        }
        let line = SummaryDisplay { label, value };
        lines.push(line);
    }

    Ok(lines)
}

impl<'a> PaceDisplay<'a> {
    /// Slam through all the calculations and hash lookups necessary to render
    /// this calendar in whatever format and to whichever interested party
    /// is...interested.
    pub fn from(p: &'a Pace, glob: &'a Glob) -> Result<PaceDisplay<'a>, String> {
        log::trace!(
            "GoalDisplay::from( [ Pace {:?} ], [ Glob ] ) called.",
            &p.student.base.uname
        );

        let today = crate::now();
        let semf_end = match glob.dates.get("end-fall") {
            Some(d) => d,
            None => {
                return Err("Date \"end-fall\" not set by Admin.".to_owned());
            }
        };
        let sems_end = match glob.dates.get("end-spring") {
            Some(d) => d,
            None => {
                return Err("Date \"end-spring\" not set by Admin.".to_owned());
            }
        };

        let mut previously_inc = false;
        let mut has_review_chapters = false;
        let mut has_incomplete_chapters = false;
        let mut semf_inc = false;
        let mut sems_inc = false;
        let mut weight_due: f32 = 0.0;
        let mut weight_done: f32 = 0.0;
        let mut weight_scheduled: f32 = 0.0;
        let mut semf_done: usize = 0;
        let mut sems_done: usize = 0;
        let mut semf_total: f32 = 0.0;
        let mut sems_total: f32 = 0.0;
        let mut n_due: usize = 0;
        let mut n_done: usize = 0;
        let mut n_scheduled: usize = 0;
        let mut fall_due: usize = 0;
        let mut fall_done: usize = 0;
        let mut spring_due: usize = 0;
        let mut spring_done: usize = 0;
        let mut semf_last_id: Option<i64> = None;
        let mut sems_last_id: Option<i64> = None;
        let mut last_completed_goal: Option<usize> = None;

        for g in p.goals.iter() {
            if let Some(d) = &g.due {
                if d < &today {
                    n_due += 1;
                    weight_due += g.weight;
                }
                if g.done.is_none() {
                    if d < semf_end {
                        semf_inc = true;
                    } else {
                        sems_inc = true;
                    }
                }
                n_scheduled += 1;
                weight_scheduled += g.weight;
            }

            if let Some(d) = &g.done {
                let score = maybe_parse_score_str(g.score.as_deref())
                    .map_err(|e| format!("Error parsing stored score {:?}: {}", &g.score, &e))?
                    .ok_or_else(|| format!("Goal [id {}] has done date but no score.", &g.id))?;

                if d < semf_end {
                    semf_total += score;
                    semf_done += 1;
                    semf_last_id = Some(g.id);
                } else if d < sems_end {
                    sems_total += score;
                    sems_done += 1;
                    sems_last_id = Some(g.id);
                }

                n_done += 1;
                weight_done += g.weight;
            } else if g.incomplete {
                previously_inc = true;
            }

            if g.review {
                has_review_chapters = true;
            }
            if g.incomplete {
                has_incomplete_chapters = true;
            }

            if let Some(d) = &g.due {
                if d < semf_end {
                    fall_due += 1;
                    if g.done.is_some() {
                        fall_done += 1;
                    }
                } else if d < sems_end {
                    spring_due += 1;
                    if g.done.is_some() {
                        spring_done += 1;
                    }
                }
            }
        }

        let fall_tests = if semf_done > 0 {
            semf_total / (semf_done as f32)
        } else {
            0.0_f32
        };

        let spring_tests = if sems_done > 0 {
            sems_total / (sems_done as f32)
        } else {
            0.0_f32
        };

        let fall_exam = maybe_parse_score_str(p.student.fall_exam.as_deref()).map_err(|e| {
            format!(
                "Unable to parse fall exam score {:?}: {}",
                p.student.fall_exam.as_deref().unwrap_or(""),
                &e
            )
        })?;

        let spring_exam = maybe_parse_score_str(p.student.spring_exam.as_deref()).map_err(|e| {
            format!(
                "Unable to parse spring exam score {:?}: {}",
                p.student.spring_exam.as_deref().unwrap_or(""),
                &e
            )
        })?;

        let fall_total: Option<f32> = match fall_exam {
            Some(f) => {
                let exam = f * p.student.fall_exam_fraction;
                let tests = fall_tests * (1.0 - p.student.fall_exam_fraction);
                let notices = (p.student.fall_notices as f32) * 0.01;
                Some(exam + tests - notices)
            }
            None => None,
        };

        let spring_total: Option<f32> = match spring_exam {
            Some(f) => {
                let exam = f * p.student.spring_exam_fraction;
                let tests = spring_tests * (1.0 - p.student.spring_exam_fraction);
                let notices = (p.student.spring_notices as f32) * 0.01;
                Some(exam + tests - notices)
            }
            None => None,
        };

        let mut fall_summary: SmallVec<[SummaryDisplay; 4]> = if semf_last_id.is_some() {
            if semf_done > 0 {
                generate_summary(
                    Term::Fall,
                    fall_tests,
                    p.student.fall_notices,
                    p.student.fall_exam_fraction,
                    fall_exam,
                    semf_inc,
                )?
            } else {
                SmallVec::new()
            }
        } else {
            SmallVec::new()
        };

        let mut spring_summary: SmallVec<[SummaryDisplay; 4]> = if sems_last_id.is_some() {
            if sems_done > 0 {
                generate_summary(
                    Term::Spring,
                    spring_tests,
                    p.student.spring_notices,
                    p.student.spring_exam_fraction,
                    spring_exam,
                    sems_inc,
                )?
            } else {
                SmallVec::new()
            }
        } else {
            SmallVec::new()
        };

        let n_sum_rows = fall_summary.len() + spring_summary.len();
        let mut rows: Vec<RowDisplay> = Vec::with_capacity(p.goals.len() + n_sum_rows);

        for g in p.goals.iter() {
            let gd = GoalDisplay::from_goal(g, glob, &today).map_err(|e| {
                format!(
                    "Unable to generate display info from Goal {}: {}",
                    &g.id, &e
                )
            })?;
            if gd.done.is_some() {
                last_completed_goal = Some(rows.len());
            }
            rows.push(RowDisplay::Goal(gd));

            if Some(g.id) == semf_last_id {
                rows.extend(fall_summary.drain(..).map(RowDisplay::Summary));
            } else if Some(g.id) == sems_last_id {
                rows.extend(spring_summary.drain(..).map(RowDisplay::Summary));
            }
        }

        let pd = PaceDisplay {
            uname: p.student.base.uname.as_str(),
            email: p.student.base.email.as_str(),
            last: p.student.last.as_str(),
            rest: p.student.rest.as_str(),
            tuname: p.teacher.base.uname.as_str(),
            teacher: p.teacher.name.as_str(),
            temail: p.teacher.base.email.as_str(),
            previously_inc,
            semf_inc,
            sems_inc,
            has_review_chapters,
            has_incomplete_chapters,
            weight_due,
            weight_done,
            weight_scheduled,
            fall_due,
            fall_done,
            spring_due,
            spring_done,
            fall_notices: p.student.fall_notices,
            spring_notices: p.student.spring_notices,
            fall_tests,
            spring_tests,
            fall_exam_frac: p.student.fall_exam_fraction,
            spring_exam_frac: p.student.spring_exam_fraction,
            fall_exam,
            spring_exam,
            fall_total,
            spring_total,
            n_due,
            n_done,
            n_scheduled,
            last_completed_goal,
            rows,
        };

        log::debug!("{:#?}", &pd);

        Ok(pd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Glob;
    use crate::course::Course;
    use crate::tests::ensure_logging;
    use crate::user::{BaseUser, Role, User};
    use crate::*;

    use std::fs::{read_to_string, File};

    use serial_test::serial;

    static AUTH_CONN: &str =
        "host=localhost user=camp_test password='camp_test' dbname=camp_auth_test";
    static DATA_CONN: &str =
        "host=localhost user=camp_test password='camp_test' dbname=camp_store_test";

    static COURSE_FILES: &[&str] = &[
        "test/env/course_0.mix",
        "test/env/course_1.mix",
        "test/env/course_2.mix",
        "test/env/course_3.mix",
    ];

    static BOSS: (&str, &str) = ("boss", "boss@camelthingy.com");
    static TEACHERS: &[(&str, &str, &str)] = &[
        ("bob", "Mr Bob", "bob@school.com"),
        ("sal", "Ms Sally, not Sal Khan", "sally@school.com"),
        ("yak", "Yakov Smirnoff", "yakov@school.com"),
    ];
    const STUDENT_FILE: &str = "test/env/students.csv";
    const GOALS_FILE: &str = "test/env/goals.csv";
    const DATES: &[(&str, &str)] = &[("end-fall", "2023-01-10")];

    const CONFIG_FILE: &str = "test/env/config.toml";

    async fn init_env() -> Result<Glob, String> {
        ensure_logging();

        let mut g = config::load_configuration(CONFIG_FILE).await.unwrap();

        let courses: Vec<Course> = COURSE_FILES
            .iter()
            .map(|fname| File::open(fname).unwrap())
            .map(|f| Course::from_reader(f).unwrap())
            .collect();

        let boss = BaseUser {
            uname: BOSS.0.to_owned(),
            role: Role::Boss,
            salt: String::new(),
            email: BOSS.1.to_owned(),
        }
        .into_boss();

        let student_csv = read_to_string(STUDENT_FILE).unwrap();

        let teachers: Vec<User> = TEACHERS
            .iter()
            .map(|(uname, name, email)| {
                BaseUser {
                    uname: uname.to_string(),
                    role: Role::Teacher,
                    salt: String::new(),
                    email: email.to_string(),
                }
                .into_teacher(name.to_string())
            })
            .collect();

        {
            let data = g.data();
            data.read().await.insert_courses(&courses).await?;
        }

        g.insert_user(&boss).await.unwrap();
        for u in teachers.iter() {
            g.insert_user(u).await.unwrap();
        }
        g.refresh_users().await.unwrap();
        g.upload_students(&student_csv).await.unwrap();

        {
            let data_handle = g.data();
            let data = data_handle.read().await;
            for (date_name, date_val) in DATES.iter() {
                data.set_date(date_name, &Date::parse(date_val, DATE_FMT).unwrap())
                    .await
                    .unwrap();
            }
        }
        g.refresh_dates().await.unwrap();

        g.refresh_courses().await.unwrap();
        g.refresh_users().await.unwrap();

        Ok(g)
    }

    async fn teardown_env(g: Glob) -> Result<(), String> {
        use std::fmt::Write;

        let mut err_msgs = String::new();

        {
            let data = g.data();
            let dread = data.read().await;
            if let Err(e) = dread.nuke_database().await {
                log::error!("Error tearing down data DB: {}", &e);
                writeln!(&mut err_msgs, "Data DB: {}", &e).unwrap();
            }

            let auth = g.auth();
            let aread = auth.read().await;
            if let Err(e) = aread.nuke_database().await {
                log::error!("Error tearing down auth DB: {}", &e);
                writeln!(&mut err_msgs, "Auth DB: {}", &e).unwrap();
            }
        }

        if err_msgs.is_empty() {
            Ok(())
        } else {
            Err(err_msgs)
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_env() {
        let g = init_env().await.unwrap();
        log::info!(
            "Glob has {} courses, {} users.",
            &g.courses.len(),
            &g.users.len()
        );

        teardown_env(g).await.unwrap();
    }

    #[tokio::test]
    #[serial]
    async fn goals_from_csv() {
        let g = init_env().await.unwrap();
        let goals = Pace::from_csv(File::open(GOALS_FILE).unwrap(), &g).unwrap();
        log::info!(
            "Read {} Goals from test Goal file {:?}.",
            &goals.len(),
            GOALS_FILE
        );

        for goal in goals.iter() {
            println!("{:#?}", goal);
        }

        teardown_env(g).await.unwrap();
    }

    #[tokio::test]
    #[serial]
    async fn show_pace_display() {
        let g = init_env().await.unwrap();
        let paces = Pace::from_csv(File::open(GOALS_FILE).unwrap(), &g).unwrap();
        log::info!(
            "Read {} Paces from test Goal file {:?}.",
            &paces.len(),
            GOALS_FILE
        );
        for p in paces.iter() {
            g.insert_goals(&p.goals).await.unwrap();
        }

        let p = g.get_pace_by_student("dval").await.unwrap();
        println!("{:#?}", &p);
        let p_disp = PaceDisplay::from(&p, &g).unwrap();
        println!("\n{:#?}\n", &p_disp);

        teardown_env(g).await.unwrap();
    }
}

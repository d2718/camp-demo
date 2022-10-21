/*!
Information about various courses and chapters.

Example of human-readable course data input format:

```text
title = "Core Precalculus"
sym = "pc"
book = "Precalculus: Functions and Graphs"
level = 12.1

# Last three columns are optional.
# Weights will default to 1.0, titles will default to "Chapter N", and
# subjects will default to nothing.
#
#chapter,   weight,     title,      subject
1,          8,          Chapter 1,  Topics from Algebra
2,          9,          Chapter 2,  Graphs and Functions
3,          8,          Chapter 3,  Polynomial and Rational Functions
4,          8,          Chapter 4,  Exponential and Logarithmic Functions
5,          9,          Chapter 5,  Trigonometric Functions
6,          8,          Chapter 6,  Analytic Trigonometry
7,          8,          Chapter 7,  Applications of Trigonometry
```
*/
use std::io::{BufRead, BufReader, Cursor, Read};

use serde::{Deserialize, Serialize};

/// Count the number of newlines in a `&str`.
fn count_newlines(s: &str) -> usize {
    s.as_bytes().iter().filter(|&b| *b == b'\n').count()
}

/// Return whether `s` is entirely composed of ASCII whitespace characters.
fn all_ascii_whitespace(s: &str) -> bool {
    for b in s.as_bytes().iter() {
        if !b.is_ascii_whitespace() {
            return false;
        }
    }
    true
}

/**
Attempt to read a line from the supplied reader into the supplied buffer and
advance the supplied current line number by one. Report an error on error OR
on EOF.

This function is called by `read_toml_csv_split()` below to simplify reading
line-by-line while also throwing an error on an unexpected EOF.
*/
fn read_line_or_complain<R: Read>(
    reader: &mut BufReader<R>,
    buffer: &mut String,
    line_n: &mut usize,
) -> Result<(), String> {
    buffer.clear();
    match reader.read_line(buffer) {
        Ok(0) => Err(format!("Unexpected end of data at line {}.", line_n)),
        Ok(_) => {
            *line_n += 1;
            Ok(())
        }
        Err(e) => Err(format!("Error reading at line {}: {}", line_n, &e)),
    }
}

/**
From a reader, split it into a TOML first half and a CSV second half.

The two halves should be separated by at least one blank line. Return value is
```ignore
(
    usize,  # number of lines of leading whitespace
    String, # TOML first half
    usize,  # number of lines of interstitial whitespace
    String  # CSV second half
)
```
The numbers of lines of whitespace are necessary so that later parse errors
can be reported with a correct line number.
*/
fn read_toml_csv_split<R: Read>(r: R) -> Result<(usize, String, usize, String), String> {
    // This code is stateful, inelegant, fidgety, and somewhat brittle.

    log::trace!("read_toml_csv_split(...) called.");

    // Capacities here are overestimates to avoid multiple reallocations.
    let mut toml_chunk = String::with_capacity(1024);
    let mut csv_chunk = String::with_capacity(2048);

    let mut reader = BufReader::new(r);
    // Again, this is an overestimate of the amount of capacity needed.
    let mut line = String::with_capacity(128);
    let mut line_n: usize = 1;

    // Here we are allowing for (ignoring) any leading whitespace.
    let mut n_leading_lines: usize = 0;
    read_line_or_complain(&mut reader, &mut line, &mut line_n)?;
    while all_ascii_whitespace(&line) {
        n_leading_lines += 1;
        read_line_or_complain(&mut reader, &mut line, &mut line_n)?;
    }

    // Now that we have lines with whitespace, we are reading them and
    // pushing them onto the `toml_chunk` until we hit whitespace again.
    while !all_ascii_whitespace(&line) {
        toml_chunk.push_str(&line);
        read_line_or_complain(&mut reader, &mut line, &mut line_n)?;
    }

    // We toss aside our lines of interstitial whitespace until we hit
    // text again.
    let mut n_middle_lines: usize = 0;
    while all_ascii_whitespace(&line) {
        n_middle_lines += 1;
        read_line_or_complain(&mut reader, &mut line, &mut line_n)?;
    }

    // In our final block, we push everything (including any blank lines)
    // onto `csv_chunk` until we hit EOF (`reader.read_line()` returns
    // `Ok(0)`).
    csv_chunk.push_str(&line);
    loop {
        line.clear();
        let res = reader.read_line(&mut line);
        line_n += 1;
        match res {
            Ok(0) => {
                log::trace!(
                    "Successfully returns ({} blank lines, {} bytes of TOML, {} blank lines, {} bytes of CSV, )",
                    &n_leading_lines, &toml_chunk.len(), &n_middle_lines, &csv_chunk.len(),
                );
                return Ok((n_leading_lines, toml_chunk, n_middle_lines, csv_chunk));
            }
            Ok(_) => {
                csv_chunk.push_str(&line);
            }
            Err(e) => {
                return Err(format!("Error reading at line {}: {}", &line_n, &e));
            }
        }
    }
}

/// Represents the material covered by a "custom" [`Goal`](crate::pace::Goal)
/// not represented by a Chapter in any current Courses in the database.
///
/// This is currently not supported, but may be in the future.
pub struct Custom {
    /// Database primary key.
    pub id: i64,
    /// Name of the teacher who created the custom material (and who is
    /// allowed to delete it).
    pub uname: String,
    /// Title of the custom material.
    pub title: String,
    /// Associated Goal `weight`. This should be a fraction of a complete
    /// hypothetical course.
    pub weight: f32,
}

/// Represents the material covered by a [`Goal`](crate::pace::Goal) that
/// consists of a specific Chapter of one of the Courses in the database.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Chapter {
    /// Database primary key.
    pub id: i64,
    /// `id` of the Course to which this Chapter belongs.
    pub course_id: i64,
    /// The number of the chapter in the text.
    ///
    /// This really should be a `usize`, but it has to map to a two-byte
    /// integer in the database.
    pub seq: i16,
    /// The title of the Chapter. In general this will be automatically
    /// generated as "Chapter N" (where `N` is `self.seq`), but it can be
    /// changed after generation.
    pub title: String,
    /// It's nice for informational purposes if the subject of the chapter is
    /// also stored in the database, but not necessary.
    pub subject: Option<String>,
    /// Chapter weight relative to other Chapters in the Course.
    pub weight: f32,
}

impl Chapter {
    /// Attempts to create a `Chapter` from the info in a line of CSV data.
    ///
    /// Called by [`Course::from_reader`] when reading the CSV Chapter part.
    pub fn from_csv_line(line: &csv::StringRecord) -> Result<Chapter, String> {
        log::trace!("Chapter::from_csv_line( {:?} ) called.", line);

        let seq: i16 = match line.get(0) {
            None => return Err("line must start with chapter value".to_owned()),
            Some(text) => text.parse::<i16>().map_err(|e| {
                format!(
                    "{:?} is not a valid chapter number: {}. (Hint: try a non-negative integer.)",
                    &text, &e
                )
            })?,
        };

        let weight: f32 = match line.get(1) {
            None => 1.0_f32,
            Some(text) => text.parse::<f32>()
                .map_err(|e| format!(
                    "{:?} is not a valid weight: {}. (Hint: try a decimal number, like \"1\" or \"3.5\".)",
                    &text, &e
                ))?,
        };

        let title: String = match line.get(2) {
            None => format!("Chapter {}", &seq),
            Some(text) => {
                if text.is_empty() {
                    format!("Chapter {}", &seq)
                } else {
                    text.to_owned()
                }
            }
        };

        let subject: Option<String> = line.get(3).map(|s| s.to_owned());

        let ch = Chapter {
            id: 0,
            course_id: 0,
            seq,
            title,
            subject,
            weight,
        };
        log::trace!("Chapter::from_csv_line() returns: {:?}", &ch);
        Ok(ch)
    }
}

/**
The purpose of the `CourseHeader` is to get deserialized from the JSON header
of the human-readable course data input format, in the course of instantiating
a `Course` struct from the human-readable course data.
*/
#[derive(Debug, Deserialize)]
struct CourseHeader {
    title: String,
    sym: String,
    book: String,
    level: f32,
}

/**
A `Course` represents the requirements for a single academic year-long course
of Mathematics. This is almost universally some chunk of chapters (or partial
chapters) from a single textbook.
*/
#[derive(Debug, Deserialize, Serialize)]
pub struct Course {
    pub id: i64,
    pub sym: String,
    pub book: String,
    pub title: String,
    pub level: f32,
    pub weight: Option<f32>,
    chapters: Vec<Chapter>,
}

impl Course {
    /**
    Attempt to read data for and instantiate a single `Course` from data in
    "course file" format.

    This format consists of a TOML header followed by a list of Chapter
    data in CSV format. See the submodule documentation for details.
    */
    pub fn from_reader<R: Read>(r: R) -> Result<Course, String> {
        log::trace!("Course::from_reader(...) called.");

        let (n_head_blanks, toml_header, n_between_blanks, csv_body) = read_toml_csv_split(r)?;

        let head: CourseHeader = match toml::from_str(&toml_header) {
            Ok(header) => header,
            Err(e) => match e.line_col() {
                None => {
                    return Err(e.to_string());
                }
                Some((line, col)) => {
                    let estr = format!(
                        "Error at line {}, character {}: {}",
                        line + n_head_blanks + 1,
                        col + 1,
                        &e
                    );
                    return Err(estr);
                }
            },
        };

        let csv_start_line_n = n_head_blanks + count_newlines(&toml_header) + n_between_blanks;

        let body_bytes = csv_body.as_bytes();
        let mut csv_reader = csv::ReaderBuilder::new()
            .comment(Some(b'#'))
            .trim(csv::Trim::All)
            .flexible(true)
            .has_headers(false)
            .from_reader(Cursor::new(body_bytes));

        // We overestimate the required capacity, and then shrink it later.
        let mut chapters: Vec<Chapter> = Vec::with_capacity(64);

        for (n, res) in csv_reader.records().enumerate() {
            match res {
                Ok(record) => match Chapter::from_csv_line(&record) {
                    Ok(chapt) => {
                        chapters.push(chapt);
                    }
                    Err(e) => {
                        let estr = match record.position() {
                            Some(p) => format!(
                                "Error on line {}: {}",
                                p.line() as usize + csv_start_line_n,
                                &e
                            ),
                            None => format!("Error in CSV record {}: {}", &n, &e),
                        };
                        return Err(estr);
                    }
                },
                Err(e) => {
                    let estr = match e.position() {
                        Some(p) => format!(
                            "Error on line {}: {}",
                            p.line() as usize + csv_start_line_n,
                            &e
                        ),
                        None => format!("Error in CSV record {}: {}", &n, &e),
                    };
                    return Err(estr);
                }
            }
        }

        chapters.shrink_to_fit();
        if chapters.is_empty() {
            return Err("Course file contains no chapters.".to_owned());
        }
        let weight = Some(chapters.iter().map(|ch| ch.weight).sum());

        let c = Course {
            id: 0,
            sym: head.sym,
            book: head.book,
            title: head.title,
            level: head.level,
            weight,
            chapters,
        };
        Ok(c)
    }

    pub fn new(id: i64, sym: String, book: String, title: String, level: f32) -> Self {
        Self {
            id,
            sym,
            book,
            title,
            level,
            weight: None,
            chapters: Vec::new(),
        }
    }

    /// Builder-pattern method to add `Chapter`s after the fact.
    pub fn with_chapters(self, chapters: Vec<Chapter>) -> Self {
        let mut new = self;
        new.weight = Some(chapters.iter().map(|ch| ch.weight).sum());
        new.chapters = chapters;
        new
    }

    /// Return a reference to Chapter `n` in the course, if it exists.
    pub fn chapter(&self, n: i16) -> Option<&Chapter> {
        // Right now this is a linear search. This may change in the future
        // if the data structure holding `Chapter`s becomes something other
        // than a `Vec`, but I'm not too woried about performance here.
        for ch in (&self.chapters).iter() {
            if ch.seq == n {
                return Some(ch);
            }
        }
        None
    }

    /// Return an iterator over all the `&Chapter`s.
    pub fn all_chapters(&self) -> impl Iterator<Item = &Chapter> {
        self.chapters.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::ensure_logging;

    use std::fs;

    #[test]
    fn test_toml_csv_split() {
        ensure_logging();

        let (good_toml, good_csv) = (
            fs::read_to_string("test/good_course_0.toml").unwrap(),
            fs::read_to_string("test/good_course_0.csv").unwrap(),
        );
        let f = fs::File::open("test/good_course_0.mix").unwrap();
        assert_eq!((0, good_toml, 1, good_csv), read_toml_csv_split(f).unwrap());

        let (good_toml, good_csv) = (
            fs::read_to_string("test/good_course_1.toml").unwrap(),
            fs::read_to_string("test/good_course_1.csv").unwrap(),
        );
        let f = fs::File::open("test/good_course_1.mix").unwrap();
        assert_eq!((2, good_toml, 3, good_csv), read_toml_csv_split(f).unwrap());

        let res = read_toml_csv_split(fs::File::open("test/bad_course_0.mix").unwrap());
        log::trace!("{:?}", &res);
        assert!(res.is_err());
        let res = read_toml_csv_split(fs::File::open("test/bad_course_1.mix").unwrap());
        log::trace!("{:?}", &res);
        assert!(res.is_err());
    }

    #[test]
    fn test_course_from_reader() {
        ensure_logging();

        let good = fs::read_to_string("test/good_course_0.debug").unwrap();
        let f = fs::File::open("test/good_course_0.mix").unwrap();
        assert_eq!(good, format!("{:#?}", &Course::from_reader(f).unwrap()));

        let good = fs::read_to_string("test/good_course_1.debug").unwrap();
        let f = fs::File::open("test/good_course_1.mix").unwrap();
        assert_eq!(good, format!("{:#?}", &Course::from_reader(f).unwrap()));
    }

    #[test]
    fn test_course_get_chapter() {
        ensure_logging();

        let crs = Course::from_reader(fs::File::open("test/good_course_0.mix").unwrap()).unwrap();

        assert!(crs.chapter(0).is_none());
        assert!(crs.chapter(8).is_none());
        let chapt = fs::read_to_string("test/pc_ch4.debug").unwrap();
        assert_eq!(chapt, format!("{:#?}", crs.chapter(4).unwrap()));
    }

    #[test]
    fn make_course_serialized() {
        use serde_json::to_writer_pretty;

        let crs = Course::from_reader(fs::File::open("test/good_course_0.mix").unwrap()).unwrap();

        println!("Debug:\n{:#?}\n", &crs);

        let mut buff: Vec<u8> = Vec::new();
        buff.extend_from_slice(b"serde_json:\n");
        to_writer_pretty(&mut buff, &crs).unwrap();
        buff.push(b'\n');
        let buff = String::from_utf8(buff).unwrap();

        println!("{}", &buff);
    }
}

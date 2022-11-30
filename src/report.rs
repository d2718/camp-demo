/*!
Internal representations, containers, and functions for data only necessary
for report-writing season.
*/
use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write as FmtWrite,
    io::Write as IoWrite,
};

use serde::{Deserialize, Serialize};
use time::{format_description::FormatItem, macros::format_description};

use crate::{
    blank_string_means_none,
    config::Glob,
    format_maybe_date,
    inter::{render_raw_template, write_raw_template},
    pace::{GoalDisplay, PaceDisplay, RowDisplay, Term},
    MiniString, UnifiedError, SMALLSTORE, MEDSTORE,
};

const DATE_FMT: &[FormatItem] = format_description!("[month repr:short] [day]");
const TIMESTAMP_FMT: &[FormatItem] = format_description!(
    "[year]-[month]-[day] [hour]:[minute]:[second] UTC"
);

fn write_percent(frac: f32) -> Result<MiniString<SMALLSTORE>, String> {
    let pct = (frac * 100.0_f32).round();
    let mut s: MiniString<SMALLSTORE> = MiniString::new();
    write!(&mut s, "{:.0}", &pct).map_err(|e| format!("Error writing {}: {:?}", &pct, &e))?;
    Ok(s)
}

fn write_maybe_percent(maybe_frac: Option<f32>) -> Result<MiniString<SMALLSTORE>, String> {
    match maybe_frac {
        Some(f) => write_percent(f),
        None => Ok(MiniString::new()),
    }
}

fn max_chunk_lengths(chunks: &[Vec<&str>]) -> Result<Vec<usize>, &'static str> {
    let max_len = match chunks.iter()
        .map(|line| line.len())
        .max()
    {
        Some(n) => n,
        None => { return Err("table has zero lines"); },
    };
    let mut maxen = vec![0usize; max_len];
    
    for line in chunks.iter() {
        for (n, chunk) in line.iter().enumerate() {
            let len = chunk.len();
            if len > maxen[n] { maxen[n] = len; }
        }
    }
    
    Ok(maxen)
}

fn format_markdown_table(table_input: String) -> Result<String, String> {
    let lines: Vec<&str> = table_input.split("\n")
        .map(|s| s.trim()).filter(|s| !s.is_empty()).collect();
    
    let chunks: Vec<Vec<&str>> = lines.iter()
        .map(|line| line.split('|').map(|s| s.trim()).collect())
        .collect();
    
    let maxen = match max_chunk_lengths(&chunks) {
        Ok(v) => v,
        Err(_) => {
            log::warn!("format_markdown_table( [String] ): table has zero lines");
            return Ok(String::new());
        },
    };
    
    // For each chunk, the max chunk length for that column, plus a space of
    // padding on each side, plus one pipe character. Finally one for the
    // extra pipe fence post, and one for the newline.
    let line_length: usize = maxen.iter().map(|&n| n+3).sum::<usize>() + 2;
    let output_length = line_length * chunks.len();
    println!("output length: {}", &output_length);
    
    let mut output = String::with_capacity(output_length);
    
    for line in chunks.iter() {
        output.push('|');
        for (n, chunk) in line.iter().enumerate() {
            if maxen[n] > 0 {
                write!(&mut output, " {:width$} |", chunk, width = maxen[n])
                    .map_err(|e| format!(
                        "Error writing formatted table: {}", &e
                    ))?;
            }
        }
        output.push('\n');
    }
    
    Ok(output)
}

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

impl From<FactStatus> for &'static str {
    fn from(fs: FactStatus) -> &'static str {
        use FactStatus::*;
        match fs {
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
    pub fall_complete: Vec<String>,
    pub spring_complete: Vec<String>,
    pub summer_complete: Vec<String>,
    pub mastery: Vec<Mastery>,
}

fn fact_status_display(factstatus: FactStatus) -> &'static str {
    match factstatus {
        FactStatus::Not => "Not Mastered",
        FactStatus::Mastered => "Mastered",
        FactStatus::Excused => "Excused",
    }
}

#[derive(Debug, Serialize)]
struct FactSetDisplay {
    add: &'static str,
    sub: &'static str,
    mul: &'static str,
    div: &'static str,
}

impl Default for FactSetDisplay {
    fn default() -> Self {
        FactSetDisplay {
            add: fact_status_display(FactStatus::Not),
            sub: fact_status_display(FactStatus::Not),
            mul: fact_status_display(FactStatus::Not),
            div: fact_status_display(FactStatus::Not),
        }
    }
}

impl From<FactSet> for FactSetDisplay {
    fn from(fs: FactSet) -> Self {
        FactSetDisplay {
            add: fact_status_display(fs.add),
            sub: fact_status_display(fs.sub),
            mul: fact_status_display(fs.mul),
            div: fact_status_display(fs.div),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ReportGoalData<'a> {
    course: &'a str,
    title: &'a str,
    due: MiniString<SMALLSTORE>,
    done: MiniString<SMALLSTORE>,
    tries: Option<i16>,
    score: MiniString<SMALLSTORE>,
    mastery: &'static str,
}

impl<'a> ReportGoalData<'a> {
    fn new(
        gd: GoalDisplay<'a>,
        mastery: Option<MasteryStatus>,
    ) -> Result<ReportGoalData<'a>, String> {
        let due = format_maybe_date(DATE_FMT, &gd.due)?;
        let done = format_maybe_date(DATE_FMT, &gd.done)?;
        let score = write_maybe_percent(gd.score)?;
        let mastery = match mastery {
            Some(ms) => ms.as_str(),
            None => "",
        };

        let rgd = ReportGoalData {
            course: gd.course,
            title: gd.title,
            tries: gd.tries,
            due,
            done,
            score,
            mastery,
        };

        Ok(rgd)
    }
}

#[derive(Debug, Serialize)]
pub struct SocialData<'a, 'b> {
    category: &'a str,
    fall_score: &'b str,
    spring_score: &'b str,
}

impl<'a, 'b> SocialData<'a, 'b> {
    pub fn new<K, V>(k: &'a K, u: Option<&'b V>, v: Option<&'b V>) -> SocialData<'a, 'b>
    where
        K: AsRef<str>,
        V: AsRef<str>,
    {
        let k = k.as_ref();
        let u = match u {
            Some(u) => u.as_ref(),
            None => "",
        };
        let v = match v {
            Some(v) => v.as_ref(),
            None => "",
        };

        SocialData {
            category: k,
            fall_score: u,
            spring_score: v,
        }
    }
}

/// For serializing report markdown document.
#[derive(Debug, Serialize)]
pub struct ReportData<'a> {
    rest: &'a str,
    last: &'a str,
    teacher: &'a str,
    academic_year: MiniString<SMALLSTORE>,
    term: &'a str,
    pace_lines: String,
    facts_table: String,
    social_lines: String,
    fall_reqs: &'a str,
    spring_reqs: &'a str,
    summer_reqs: &'a str,
    fall_remain: usize,
    spring_remain: usize,
    summer_remain: usize,
    fall_complete: String,
    spring_complete: String,
    summer_complete: String,
    requirement_statement: String,
    fall_tests: MiniString<SMALLSTORE>,
    spring_tests: MiniString<SMALLSTORE>,
    fall_notices: i16,
    spring_notices: i16,
    exam_weight: MiniString<SMALLSTORE>,
    fall_exam: MiniString<SMALLSTORE>,
    spring_exam: MiniString<SMALLSTORE>,
    fall_pct: MiniString<SMALLSTORE>,
    fall_letter: &'a str,
    spring_pct: MiniString<SMALLSTORE>,
    spring_letter: &'a str,
    summary_lines: String,
    timestamp: MiniString<MEDSTORE>,
}

fn reqs_complete(is_incomplete: bool) -> &'static str {
    if is_incomplete {
        "No"
    } else {
        "Yes"
    }
}

fn letter_grade(frac: Option<f32>) -> &'static str {
    match frac {
        Some(f) => {
            let f = (100.0 * f).round();
            if f < 70.0 {
                "I"
            } else if f < 73.0 {
                "C-"
            } else if f < 77.0 {
                "C"
            } else if f < 80.0 {
                "C+"
            } else if f < 83.0 {
                "B-"
            } else if f < 87.0 {
                "B"
            } else if f < 90.0 {
                "B+"
            } else if f < 93.0 {
                "A-"
            } else if f < 97.0 {
                "A"
            } else {
                "A+"
            }
        }
        None => "",
    }
}

fn collect_course_names<S>(syms: &[S], glob: &Glob) -> Result<String, String>
where S: AsRef<str> + std::fmt::Debug
{
    log::trace!("collect_course_names( {:?}, [ &Glob ] ) called.", syms);

    let target = match syms.len() {
        0 => { return Ok(String::new()); },
        n => n - 1,
    };

    let mut list = String::new();
    for (n, sym) in syms.iter().enumerate() {
        let sym = sym.as_ref();
        match glob.course_by_sym(sym) {
            Some(crs) => { list.push_str(&crs.title); },
            None => { return Err(format!(
                "{:?} is not a valid course symbol.", sym
            )); },
        }
        if n < target {
            list.push(',');
            list.push(' ');
        }
    }
    
    Ok(list)
}

impl<'a, 'b> ReportData<'a> {
    fn assemble(
        mut pd: PaceDisplay<'a>,
        sc: ReportSidecar,
        term: Term,
        glob: &Glob,
    ) -> Result<ReportData<'a>, String> {
        let academic_year = glob.academic_year_string();

        let facts_status = match sc.facts {
            None => FactSetDisplay::default(),
            Some(fs) => FactSetDisplay::from(fs),
        };

        let facts_table = {
            let table = render_raw_template("facts_table", &facts_status)
                .map_err(|e| format!(
                    "Unable to write fact mastery table: {}", &e
                ))?;
            format_markdown_table(table).map_err(|e| format!(
                "Unable to format fact mastery table: {}", &e
            ))?
        };

        let academic_year_end = match glob.dates.get("end-spring") {
            Some(d) => d,
            None => {
                return Err("Admin has not set \"end-spring\" date.".to_owned());
            },
        };

        let fall_tests = write_percent(pd.fall_tests)
            .map_err(|e| format!("Error writing fall test average: {}", &e))?;
        let spring_tests = write_percent(pd.spring_tests)
            .map_err(|e| format!("Error writing spring test average: {}", &e))?;
        let fall_pct = write_maybe_percent(pd.fall_total)
            .map_err(|e| format!("Error writing fall semester grade: {}", &e))?;
        let spring_pct = write_maybe_percent(pd.spring_total)
            .map_err(|e| format!("Error writing spring semester grade: {}", &e))?;
        
        let pace_head_file = match term {
            Term::Fall | Term::Spring => "data/report_pace_head.md",
            Term::Summer => "data/report_pace_head_summer.md",
        };

        let pace_lines = {
            let mastery: BTreeMap<i64, MasteryStatus> =
                sc.mastery.iter().map(|m| (m.id, m.status)).collect();

            let mut lines = std::fs::read(pace_head_file)
                .map_err(|e| format!(
                    "Unable to read file {:?}: {}", pace_head_file, &e
                ))?;

            for gd in pd
                .rows
                .drain(..)
                .filter(|rd| matches!(rd, RowDisplay::Goal(_)))
                .map(|rd| match rd {
                    RowDisplay::Goal(gd) => gd,
                    _ => {
                        panic!("This shouldn't happen.");
                    }
                })
            {
                match term {
                    Term::Fall | Term::Spring => {
                        let mast = if gd.done.is_some() {
                            mastery.get(&gd.id).copied()
                        } else {
                            None
                        };
                        let line = ReportGoalData::new(gd, mast)?;

                        crate::inter::write_raw_template("report_goal", &line, &mut lines)?;
                    },
                    Term::Summer => {
                        // Skip any Goal completed during the Fall or Spring
                        // Semesters; show only Goals that are incomplete or
                        // completed during the Summer.
                        if let Some(d) = &gd.done {
                            if d <= academic_year_end {
                                continue;
                            }
                        }
                        let line = ReportGoalData::new(gd, None)?;

                        crate::inter::write_raw_template("report_summer_goal", &line, &mut lines)?;
                    },
                }
            }

            let table = String::from_utf8(lines)
                .map_err(|e| format!("Report goal lines are not UTF-8: {}", &e))?;
            format_markdown_table(table).map_err(|e| format!(
                "Unable to format pace goals table: {}", &e
            ))?
        };

        let social_lines = {
            let mut lines = std::fs::read("data/report_social_head.md")
                .map_err(|e| format!(
                    "Unable to read file \"data/report_social_head.md\": {}", &e
                ))?;

            for cat in glob.social_traits.iter() {
                let line = SocialData::new(cat, sc.fall_social.get(cat), sc.spring_social.get(cat));

                write_raw_template("social_goal", &line, &mut lines)?;
            }

            let table = String::from_utf8(lines).map_err(|e| {
                format!(
                    "Report social/emotional/behavioral lines are not UTF-8: {}",
                    &e
                )
            })?;
            format_markdown_table(table).map_err(|e| format!(
                "Unable to format social/emotional/behavioral goals table: {}", &e
            ))?
        };

        let exam_weight = match term {
            Term::Fall => write_percent(pd.fall_exam_frac)?,
            Term::Spring => write_percent(pd.spring_exam_frac)?,
            Term::Summer => write_percent(pd.spring_exam_frac)?,
        };

        let fall_letter = if pd.semf_inc {
            "I"
        } else {
            letter_grade(pd.fall_total)
        };
        let spring_letter = if pd.sems_inc {
            "I"
        } else {
            letter_grade(pd.spring_total)
        };

        // Shouldn't technically need saturating subtraction here, because
        // spring|fall_done shouldn't be able to exceed spring|fall_due.
        let fall_remain = pd.fall_due.saturating_sub(pd.fall_done);
        let spring_remain = pd.spring_due.saturating_sub(pd.spring_done);
        let tot_remain = (pd.fall_due + pd.spring_due).saturating_sub(pd.n_done);

        let requirement_statement = match term {
            Term::Fall => {
                if pd.semf_inc {
                    let s = if fall_remain > 1 { "s" } else { "" };
                    format!(
                        "Your student has _not_ completed their requirements for the semester.
They have {} chapter{} left before their Fall requirements are complete.",
                        &fall_remain, s
                    )
                } else {
                    "Your student has completed their requirements for the semester.".to_owned()
                }
            }
            _ => {
                if pd.sems_inc {
                    let s = if tot_remain > 1 { "s" } else { "" };
                    format!(
                        "Your student has not yet completed their requirements for the year.
They have {} chapter{} left before their {} academic year is complete.",
                        &tot_remain, s, &academic_year
                    )
                } else {
                    "Your student has completed their requirements for the year.".to_owned()
                }
            }
        };

        let fall_complete = collect_course_names(&sc.fall_complete, glob)
            .map_err(|e| format!(
                "error writing list of coures completed Fall Semester: {}", &e
            ))?;
        let spring_complete = collect_course_names(&sc.spring_complete, glob)
            .map_err(|e| format!(
                "error writing list of courses completed Spring Semester: {}", &e
            ))?;
        let summer_complete = collect_course_names(&sc.summer_complete, glob)
            .map_err(|e| format!(
                "error writing list of courses completed during Summer: {}", &e
            ))?;
        
        let mut timestamp: MiniString<MEDSTORE> = MiniString::new();
        time::OffsetDateTime::now_utc().format_into(&mut timestamp, &TIMESTAMP_FMT)
            .map_err(|e| format!(
                "error formatting timestamp: {}", &e
            ))?;

        let rd = ReportData {
            rest: pd.rest,
            last: pd.last,
            teacher: pd.teacher,
            academic_year,
            term: term.as_str(),
            pace_lines,
            facts_table,
            social_lines,
            fall_reqs: reqs_complete(pd.semf_inc),
            spring_reqs: reqs_complete(pd.sems_inc),
            summer_reqs: reqs_complete(pd.semf_inc || pd.sems_inc),
            fall_remain,
            spring_remain,
            summer_remain: tot_remain,
            requirement_statement,
            fall_complete,
            spring_complete,
            summer_complete,
            fall_tests,
            spring_tests,
            fall_notices: pd.fall_notices,
            spring_notices: pd.spring_notices,
            exam_weight,
            fall_exam: write_maybe_percent(pd.fall_exam)?,
            spring_exam: write_maybe_percent(pd.spring_exam)?,
            fall_pct,
            fall_letter,
            spring_pct,
            spring_letter,
            summary_lines: String::new(),
            timestamp,
        };

        log::debug!("{:#?}", &rd);

        Ok(rd)
    }
}

pub async fn generate_report_markup(
    uname: &str,
    term: Term,
    glob: &Glob,
) -> Result<String, UnifiedError> {
    log::trace!(
        "generate_report_markup( {:?}, {:?}, [ &Glob ]) called.",
        uname,
        &term
    );

    let this_year = glob.academic_year();

    let p = glob.get_pace_by_student(uname).await?;
    let pd = PaceDisplay::from(&p, glob)?;
    let sc = glob.data().read().await.get_report_sidecar(uname, this_year).await?;

    let mut rd = ReportData::assemble(pd, sc, term, glob)?;

    let summary_name = match term {
        Term::Fall => "fall_summary",
        Term::Spring => "spring_summary",
        Term::Summer => "summer_summary",
    };

    let summary_lines = render_raw_template(summary_name, &rd)
        .map_err(|e| format!("Error rendering template {:?}: {}", &summary_name, &e))?;
    let summary_lines = format_markdown_table(summary_lines).map_err(|e| format!(
        "Unable to format {:?} table: {}", summary_name, &e
    ))?;
    rd.summary_lines = summary_lines;

    let template_name = match term {
        Term::Fall | Term::Spring => "report",
        Term::Summer => "report_summer",
    };

    let text = render_raw_template(template_name, &rd)
        .map_err(|e| format!("Error rendering template {:?}: {}", summary_name, &e))?;

    Ok(text)
}

pub async fn render_markdown(text: String, glob: &Glob) -> Result<Vec<u8>, UnifiedError> {
    use hyper::{body, Body, Client, Method, Request};

    log::trace!(
        "render_markdown( [ {} bytes of text ], [ &G ] ) called.",
        &text.len()
    );
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()
        .https_only()
        .enable_http1()
        .build();
    let client: Client<_, hyper::Body> = Client::builder().build(https);

    let format: &str = match glob.pandoc_format.as_ref() {
        Some(fmt) => fmt,
        None => "markdown+smart+raw_attribute",
    };

    let req = Request::builder()
        .method(Method::POST)
        .uri(&glob.pandoc_uri)
        .header("Authorization", &glob.pandoc_auth)
        .header("Content-Type", "text/markdown")
        .header("x-camp-from", format)
        .body(Body::from(text))
        .map_err(|e| format!("Error building report PDF rendering request: {}", &e))?;

    let resp = client
        .request(req)
        .await
        .map_err(|e| format!("Error from PDF rendering request: {}", &e))
        .map_err(|e| format!("Error sending PDF rendering request: {}", &e))?;

    if resp.status() != 200 {
        let reason = match resp.status().canonical_reason() {
            Some(s) => format!("{} ({})", s, resp.status().as_u16()),
            None => format!("{}", resp.status().as_u16()),
        };
        return Err(format!(
            "PDF rendering service returned {} (expected 200) while attempting to render report into PDF.",
            &reason
        ).into());
    }

    let bytes = body::to_bytes(resp.into_body())
        .await
        .map_err(|e| format!("Error reading response from PDF rendering service: {}", &e))?;
    Ok(bytes.to_vec())
}

#[cfg(test)]
mod tests {
    use serial_test::serial;

    use crate::{config, tests::ensure_logging};

    use super::*;

    static CONFIG: &str = "fakeprod_data/config.toml";
    static UNAME: &str = "zmilk";
    static OUTDIR: &str = "scratch/";

    #[tokio::test]
    #[serial]
    async fn fall_markdown() -> Result<(), Box<dyn std::error::Error>> {
        ensure_logging();
        let glob = config::load_configuration(CONFIG).await?;
        let text = generate_report_markup(UNAME, Term::Fall, &glob).await?;
        let mut fname = String::from(OUTDIR);
        fname.push_str(UNAME);
        fname.push_str("_fall.md");
        std::fs::write(&fname, &text.as_bytes())?;
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn spring_markdown() -> Result<(), Box<dyn std::error::Error>> {
        ensure_logging();
        let glob = config::load_configuration(CONFIG).await?;
        let text = generate_report_markup(UNAME, Term::Spring, &glob).await?;
        let mut fname = String::from(OUTDIR);
        fname.push_str(UNAME);
        fname.push_str("_spring.md");
        std::fs::write(&fname, &text.as_bytes())?;
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn render_report() -> Result<(), Box<dyn std::error::Error>> {
        ensure_logging();
        let glob = config::load_configuration(CONFIG).await?;
        let text = generate_report_markup(UNAME, Term::Spring, &glob).await?;
        let pdf_bytes = render_markdown(text, &glob).await?;
        let mut fname = String::from(OUTDIR);
        fname.push_str(UNAME);
        fname.push_str("_spring.pdf");
        std::fs::write(&fname, &pdf_bytes)?;
        Ok(())
    }
}

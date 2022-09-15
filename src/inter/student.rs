/*!
Displaying individual student calendars.
*/
use time::{format_description::FormatItem, macros::format_description, Date};

use crate::{
    pace::{GoalDisplay, GoalStatus, PaceDisplay, RowDisplay, SummaryDisplay},
    user::Student,
    MiniString, SMALLSTORE,
};

use super::*;

const DATE_FMT: &[FormatItem] = format_description!("[month repr:short] [day]");

/// The data required to render the `"student_goal_row"` template when
/// generating the student's view.
#[derive(Debug, Serialize)]
struct GoalData<'a> {
    course: &'a str,
    book: &'a str,
    chapter: &'a str,
    subject: &'a str,
    ri: &'a str,
    due: MiniString<SMALLSTORE>,
    due_from: MiniString<SMALLSTORE>,
    done: MiniString<SMALLSTORE>,
    done_from: MiniString<SMALLSTORE>,
    tries: Option<i16>,
    score: Option<i32>,
    goal_class: &'a str,
}

/// Data required to render the "summary_row" template when generating
/// the student's view.
#[derive(Debug, Serialize)]
struct SummaryData<'a> {
    text: &'a str,
    score: &'a str,
}

/// Write the display data for a single goal to a buffer of bytes.
///
/// Used in generating the student's view.
fn write_goal(buff: &mut Vec<u8>, g: &GoalDisplay, today: &Date) -> Result<(), String> {
    let ri = match (g.rev, g.inc) {
        (false, false) => "",
        (true, false) => " R*",
        (false, true) => " I*",
        (true, true) => " R* I*",
    };

    let mut due: MiniString<SMALLSTORE> = MiniString::new();
    let mut due_from: MiniString<SMALLSTORE> = MiniString::new();
    let mut done: MiniString<SMALLSTORE> = MiniString::new();
    let mut done_from: MiniString<SMALLSTORE> = MiniString::new();

    if let Some(d) = &g.due {
        d.format_into(&mut due, &DATE_FMT)
            .map_err(|e| format!("Failed to format date {:?}: {}", d, &e))?;
        match (*d - *today).whole_days() {
            i @ 2..=i64::MAX => {
                write!(&mut due_from, "in {} days", i).map_err(|e| e.to_string())?;
            }
            1 => {
                write!(&mut due_from, "tomorrow").map_err(|e| e.to_string())?;
            }
            0 => {
                write!(&mut due_from, "today").map_err(|e| e.to_string())?;
            }
            -1 => {
                write!(&mut due_from, "yesterday").map_err(|e| e.to_string())?;
            }
            i @ i64::MIN..=-2 => {
                write!(&mut due_from, "{} days ago", -i).map_err(|e| e.to_string())?;
            }
        }
    }

    if let Some(n) = &g.done {
        n.format_into(&mut done, &DATE_FMT)
            .map_err(|e| format!("Failed to format date {:?}: {}", n, &e))?;
    }

    if let (Some(d), Some(n)) = (&g.due, &g.done) {
        match (*d - *n).whole_days() {
            i @ 2..=i64::MAX => {
                write!(&mut done_from, "{} days early", &i).map_err(|e| e.to_string())?;
            }
            1 => {
                write!(&mut done_from, "one day early").map_err(|e| e.to_string())?;
            }
            0 => {
                write!(&mut done_from, "on time").map_err(|e| e.to_string())?;
            }
            -1 => {
                write!(&mut done_from, "one day late").map_err(|e| e.to_string())?;
            }
            i @ i64::MIN..=-2 => {
                write!(&mut done_from, "{} days late", -i).map_err(|e| e.to_string())?;
            }
        }
    }

    let score = g.score.map(|f| (100.0 * f).round() as i32);

    let goal_class = match g.status {
        GoalStatus::Done => "done",
        GoalStatus::Late => "late",
        GoalStatus::Overdue => "overdue",
        GoalStatus::Yet => "yet",
    };

    let data = GoalData {
        course: g.course,
        book: g.book,
        chapter: g.title,
        subject: g.subject.unwrap_or(""),
        ri,
        due,
        due_from,
        done,
        done_from,
        tries: g.tries,
        score,
        goal_class,
    };

    write_template("student_goal_row", &data, buff)
        .map_err(|e| format!("Error writing goal {:?}: {}", g, &e))
}

/// Write the display data for a summary row to a buffer of bytes.
///
/// For generating the student's view.
fn write_summary(buff: &mut Vec<u8>, s: &SummaryDisplay) -> Result<(), String> {
    let data = SummaryData {
        text: s.label,
        score: s.value.as_str(),
    };

    write_template("summary_row", &data, buff)
        .map_err(|e| format!("Error writing summary row w/data {:?}: {}", &data, &e))
}

/**
Determine whether the student's login credentials check out, then render the
view they are supposed to see.
*/
pub async fn login(s: Student, form: LoginData, glob: Arc<RwLock<Glob>>) -> Response {
    let glob = glob.read().await;
    match glob
        .auth()
        .read()
        .await
        .check_password(&s.base.uname, &form.password, &s.base.salt)
        .await
    {
        Err(e) => {
            log::error!(
                "auth::Db::check_password( {:?}, {:?}, {:?} ) error: {}",
                &s.base.uname,
                &form.password,
                &s.base.salt,
                &e
            );
            return html_500();
        }
        Ok(AuthResult::Ok) => { /* This is the happy path; proceed. */ }
        Ok(AuthResult::BadPassword) => {
            return respond_bad_password(&s.base.uname);
        }
        Ok(x) => {
            log::warn!(
                "auth::Db::check_password( {:?}, {:?}, {:?} ) returned {:?}, which shouldn't happen.",
                &s.base.uname, &form.password, &s.base.salt, &x
            );
            return respond_bad_password(&s.base.uname);
        }
    }

    let p = match glob.get_pace_by_student(&s.base.uname).await {
        Ok(p) => p,
        Err(e) => {
            log::error!(
                "Glob::get_pace_by_student( {:?} ) error: {}",
                &s.base.uname,
                &e
            );
            return html_500();
        }
    };

    let pd = match PaceDisplay::from(&p, &glob) {
        Ok(pd) => pd,
        Err(e) => {
            log::error!(
                "PaceDisplay::from( [ Pace {:?} ] ) error: {}\npace data: {:#?} )",
                &p.student.base.uname,
                &e,
                &p
            );
            return html_500();
        }
    };

    let today = crate::now();

    let mut goals_buff: Vec<u8> = Vec::new();

    for row_display in pd.rows.iter() {
        match row_display {
            RowDisplay::Goal(g) => {
                if let Err(e) = write_goal(&mut goals_buff, g, &today) {
                    log::error!("Error writing goal: {}\ndata: {:?}", &e, g);
                    return html_500();
                }
            }
            RowDisplay::Summary(s) => {
                if let Err(e) = write_summary(&mut goals_buff, s) {
                    log::error!("Error writing summary line: {}\ndata: {:?}", &e, s);
                    return html_500();
                }
            }
        }
    }

    let rows = match String::from_utf8(goals_buff) {
        Ok(s) => s,
        Err(e) => {
            log::error!("Buffer of Goal lines for some reaosn not UTF-8: {}", &e);
            return html_500();
        }
    };

    let rev_foot = if pd.has_review_chapters {
        "*R after a chapter indicates previously-completed material that requires review."
    } else {
        ""
    };
    let inc_foot = if pd.has_incomplete_chapters {
        "*I after a chapter indicates material incomplete from a prior acadmic year."
    } else {
        ""
    };

    let data = json!({
        "name": format!("{} {}", pd.rest, pd.last),
        "uname": pd.uname,
        "teacher": pd.teacher,
        "temail":  pd.temail,
        "n_done": pd.n_done,
        "n_due": pd.n_due,
        "n_total": pd.n_scheduled,
        "rows": rows,
        "rev_foot": rev_foot,
        "inc_foot": inc_foot,
    });

    serve_raw_template(StatusCode::OK, "student", &data, vec![])
}

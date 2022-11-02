/*!
Subcrate for generation of "Boss" page and responding to the few "Boss"
API calls.
*/
use core::fmt::Write as CoreWrite;
use std::io::Write as IoWrite;
use std::{
    str::FromStr,
    sync::Arc,
};

use axum::{
    extract::Extension,
    http::header,
    http::header::{HeaderMap, HeaderName},
    response::{IntoResponse, Response},
    Json,
};
use futures::stream::{FuturesUnordered, StreamExt};
use serde::{Deserialize, Serialize};
use smallstr::SmallString;
use time::{format_description::FormatItem, macros::format_description, Date};
use tokio::sync::RwLock;

use super::*;
use crate::{
    auth::AuthResult,
    config::Glob,
    pace::{GoalDisplay, GoalStatus, Pace, PaceDisplay, RowDisplay, Term},
    store::Store,
    user::{BaseUser, User},
    MiniString, MEDSTORE, SMALLSTORE,
};

const DATE_FMT: &[FormatItem] = format_description!("[month repr:short] [day]");

/**
Ensure a Boss's login credentials check out, generate 'em a key, and serve
the Boss view.
*/
pub async fn login(base: BaseUser, form: LoginData, glob: Arc<RwLock<Glob>>) -> Response {
    log::trace!("boss::login( {:?}, {:?}, [ Glob ] ) called.", &base, &form);

    let auth_response = {
        glob.read()
            .await
            .auth()
            .read()
            .await
            .check_password_and_issue_key(&base.uname, &form.password, &base.salt)
            .await
    };

    let auth_key = match auth_response {
        Err(e) => {
            log::error!(
                "auth:Db::check_password( {:?}, {:?}, {:?} ): {}",
                &base.uname,
                &form.password,
                &base.salt,
                &e
            );
            return html_500();
        }
        Ok(AuthResult::Key(k)) => k,
        Ok(AuthResult::BadPassword) => {
            return respond_bad_password(&base.uname);
        }
        Ok(x) => {
            log::warn!(
                "auth::Db::check_password( {:?}, {:?}, {:?} ) returned {:?}, which shouldn't happen.",
                &base.uname, &form.password, &base.salt, &x
            );
            return respond_bad_password(&base.uname);
        }
    };

    let calendar_string = match make_boss_calendars(glob.clone()).await {
        Ok(s) => s,
        Err(e) => {
            log::error!("Error attempting to write boss calendars: {}", &e);
            return respond_login_error(StatusCode::INTERNAL_SERVER_ERROR, &e);
        }
    };

    let archive_buttons_string = match make_archive_buttons(glob.clone()).await {
        Ok(s) => s,
        Err(e) => {
            log::error!("Error attempting to generate boss archive buttons: {}", &e);
            return respond_login_error(StatusCode::INTERNAL_SERVER_ERROR, &e);
        }
    };

    let data = json!({
        "uname": &base.uname,
        "key": &auth_key,
        "calendars": calendar_string,
        "archives": archive_buttons_string,
    });

    serve_raw_template(StatusCode::OK, "boss", &data, vec![])
}

/// Hods data for rendering the `"boss_archive_button"` template.
#[derive(Serialize)]
struct TeacherData<'a> {
    uname: &'a str,
    name: &'a str,
}

async fn make_archive_buttons(glob: Arc<RwLock<Glob>>) -> Result<String, String> {
    let glob = glob.read().await;

    let mut output: Vec<u8> = Vec::new();
    for (uname, u) in glob.users.iter() {
        if let User::Teacher(t) = u {
            let td = TeacherData {
                uname: uname,
                name: &t.name,
            };
            write_template("boss_archive_button", &td, &mut output)
                .map_err(|e| format!("Error writing archive button: {}", &e))?;
        }
    }

    String::from_utf8(output).map_err(|e| format!(
        "Boss archive buttons String not UTF-8: {}", &e
    ))
}

/// Holds data for rendering the `"boss_goal_row"` template.
#[derive(Serialize)]
struct GoalData<'a> {
    row_class: &'a str,
    row_bad: &'a str,
    course: &'a str,
    book: &'a str,
    chapter: &'a str,
    review: &'a str,
    incomplete: &'a str,
    due: MiniString<SMALLSTORE>,
    done: MiniString<SMALLSTORE>,
    score: MiniString<SMALLSTORE>,
}

/// Render the `"boss_goal_row"` template ta [`Write`]r.
fn write_cal_goal<W: Write>(g: &GoalDisplay, buff: W) -> Result<(), String> {
    let row_class = match g.status {
        GoalStatus::Done => "done",
        GoalStatus::Late => "late",
        GoalStatus::Overdue => "overdue",
        GoalStatus::Yet => "yet",
    };

    let row_bad = if g.inc && g.done.is_none() {
        " bad"
    } else {
        ""
    };

    let review = if g.rev { " R " } else { "" };
    let incomplete = if g.inc { " I " } else { "" };

    let mut due: MiniString<SMALLSTORE> = MiniString::new();
    if let Some(d) = g.due {
        d.format_into(&mut due, DATE_FMT)
            .map_err(|e| format!("Error writing due date {:?}: {}", &d, &e))?;
    }

    let mut done: MiniString<SMALLSTORE> = MiniString::new();
    if let Some(d) = g.done {
        d.format_into(&mut done, DATE_FMT)
            .map_err(|e| format!("Error writing done date {:?}: {}", &d, &e))?;
    }

    let mut score: MiniString<SMALLSTORE> = MiniString::new();
    if let Some(f) = g.score {
        let pct = (100.0 * f).round() as i32;
        write!(&mut score, "{} %", &pct)
            .map_err(|e| format!("Error writing score {:?}: {}", &pct, &e))?;
    }

    let data = GoalData {
        row_class,
        row_bad,
        review,
        incomplete,
        due,
        done,
        score,
        course: g.course,
        book: g.book,
        chapter: g.title,
    };

    write_raw_template("boss_goal_row", &data, buff)
}

/// Holds the data for rendering the `"boss_pace_table"` template.
#[derive(Serialize)]
struct PaceData<'a> {
    table_class: SmallString<MEDSTORE>,
    uname: &'a str,
    name: String,
    rest: &'a str,
    tuname: &'a str,
    teacher: &'a str,
    n_done: usize,
    n_due: usize,
    lag: i32,
    lagstr: SmallString<SMALLSTORE>,
    rows: String,
}

/// Render the `"boss_pace_table"` template to a [`Write`]r.
fn write_cal_table<W: Write>(p: &Pace, glob: &Glob, mut buff: W) -> Result<(), String> {
    log::trace!(
        "make_cal_table( [ {:?} Pace], [ Glob ] ) called.",
        &p.student.base.uname
    );

    let pd = PaceDisplay::from(p, glob).map_err(|e| {
        format!(
            "Error generating PaceDisplay for {:?}: {}\npace data: {:?}",
            &p.student.base.uname, &e, &p
        )
    })?;

    let mut table_class: SmallString<MEDSTORE> = SmallString::from_str("cal");
    if pd.previously_inc {
        write!(&mut table_class, " inc")
            .map_err(|e| format!("Error writing table class: {}", &e))?;
    }
    if pd.weight_done < pd.weight_due {
        write!(&mut table_class, " lag")
            .map_err(|e| format!("Error writing table class: {}", &e))?;
    }
    if pd.n_done < pd.n_due {
        write!(&mut table_class, " count")
            .map_err(|e| format!("Error writing table class: {}", &e))?;
    }

    let name = format!("{}, {}", pd.last, pd.rest);

    let lag = if pd.weight_scheduled.abs() < 0.001 {
        0
    } else {
        (100.0 * (pd.weight_done - pd.weight_due) / pd.weight_scheduled) as i32
    };
    let mut lagstr: SmallString<SMALLSTORE> = SmallString::new();
    write!(&mut lagstr, "{:+}%", &lag).map_err(|e| format!("Error writing lag string: {}", &e))?;

    let mut rows: Vec<u8> = Vec::new();
    for row in pd.rows.iter() {
        if let RowDisplay::Goal(g) = row {
            write_cal_goal(g, &mut rows).map_err(|e| {
                format!("Error writing cal for {:?}: {}", &p.student.base.uname, &e)
            })?;
        }
    }
    let rows = String::from_utf8(rows).map_err(|e| {
        format!(
            "Calendar rows for {:?} not UTF-8 for some reason: {}",
            &p.student.base.uname, &e
        )
    })?;

    let data = PaceData {
        table_class,
        name,
        lag,
        lagstr,
        rows,
        uname: pd.uname,
        rest: pd.rest,
        tuname: pd.tuname,
        teacher: pd.teacher,
        n_done: pd.n_done,
        n_due: pd.n_due,
    };

    write_raw_template("boss_pace_table", &data, &mut buff)
}

/// Generate a `String` of HTML data containing all student pace calendar data.
pub async fn make_boss_calendars(glob: Arc<RwLock<Glob>>) -> Result<String, String> {
    log::trace!("make_boss_page( [ Glob ] ) called.");

    let glob = glob.read().await;
    let tunames: Vec<&str> = glob
        .users
        .iter()
        .map(|(uname, user)| match user {
            User::Teacher(_) => Some(uname),
            _ => None,
        })
        .filter(|opt| opt.is_some())
        .map(|ok| ok.unwrap().as_str())
        .collect();

    let n_students: usize = glob
        .users
        .iter()
        .map(|(_, u)| matches!(u, User::Student(_)))
        .filter(|b| *b)
        .count();

    let mut paces: Vec<Pace> = Vec::with_capacity(n_students);
    {
        let mut retrievals = FuturesUnordered::new();
        for tuname in tunames.iter() {
            retrievals.push(glob.get_paces_by_teacher(tuname));
        }

        while let Some(res) = retrievals.next().await {
            match res {
                Ok(mut pace_vec) => {
                    paces.append(&mut pace_vec);
                }
                Err(e) => {
                    return Err(format!("Error retrieving goals from database: {}", &e));
                }
            }
        }
    }

    let mut buff: Vec<u8> = Vec::new();

    for p in paces.iter() {
        if let Err(e) = write_cal_table(p, &glob, &mut buff) {
            return Err(format!("Error generating list of pace calendars: {}", &e));
        }
    }

    let buff =
        String::from_utf8(buff).map_err(|e| format!("Pace calendar not valid UTF-8: {}", &e))?;

    Ok(buff)
}

/// Handle "Boss API" requests. Requests to "/boss" get routed here.
///
/// Right now the only API calls the Boss can make have to do with sending
/// autogenerated emails to parents.
pub async fn api(
    headers: HeaderMap,
    body: Option<String>,
    Extension(glob): Extension<Arc<RwLock<Glob>>>,
) -> Response {
    let uname: &str = match headers.get("x-camp-uname") {
        Some(uname) => match uname.to_str() {
            Ok(s) => s,
            Err(_) => {
                return text_500(None);
            }
        },
        None => {
            return text_500(None);
        }
    };

    let u = {
        let glob = glob.read().await;
        if let Some(u) = glob.users.get(uname) {
            u.clone()
        } else {
            return text_500(None);
        }
    };

    match u {
        User::Boss(_) => { /* Okay, request may proceed. */ }
        _ => {
            return (
                StatusCode::FORBIDDEN,
                "Who is this? What's your operating number?".to_owned(),
            )
                .into_response();
        }
    };

    let action = match headers.get("x-camp-action") {
        Some(act) => match act.to_str() {
            Ok(s) => s,
            Err(_) => {
                return respond_bad_request("x-camp-action header unrecognizable.".to_owned());
            }
        },
        None => {
            return respond_bad_request("Request must have an x-camp-action header.".to_owned());
        }
    };

    match action {
        "compose-email" => compose_email(body, glob.clone()).await,
        "send-email" => send_email(body, glob.clone()).await,
        "email-all" => email_all(glob.clone()).await,
        "download-report" => download_report(&headers, glob.clone()).await,
        "report-archive" => download_archive(&headers, glob.clone()).await,
        x => respond_bad_request(format!(
            "{:?} is not a recognizable x-camp-action value.",
            x
        )),
    }
}

/// Data required to render the `"boss_email"` template, generating the text
/// of a parent email.
#[derive(Serialize)]
struct EmailData<'a> {
    uname: &'a str,
    full_name: String,
    date: MiniString<MEDSTORE>,
    n_done: usize,
    n_due_str: MiniString<MEDSTORE>,
    n_scheduled: usize,
    last_done_statement: String,
    service_uri: &'a str,
    teacher: &'a str,
    temail: &'a str,
}

/// Generate the body of a parent email.
fn generate_email(pd: PaceDisplay<'_>, service_uri: &str, today: &Date) -> Result<String, String> {
    let full_name = format!("{} {}", pd.rest, pd.last);
    let mut date: MiniString<MEDSTORE> = MiniString::new();
    today
        .format_into(&mut date, DATE_FMT)
        .map_err(|e| format!("Error formatting today's date: {}", &e))?;
    let mut n_due_str: MiniString<MEDSTORE> = MiniString::new();
    match pd.n_due {
        1 => write!(&mut n_due_str, "1 goal whose due date has"),
        n => write!(&mut n_due_str, "{} goals whose due dates have", &n),
    }
    .map_err(|e| format!("Error writing # of due goals ({}): {}", &pd.n_due, &e))?;

    let last_done_statement = if let Some(n) = pd.last_completed_goal {
        let last_goal = match pd.rows.get(n) {
            None => {
                return Err("last_completed_goal index does *not* exist in rows vector!".to_owned());
            }
            Some(RowDisplay::Summary(_)) => {
                return Err(
                    "last_completed_goal index references a summary row, not a goal row!"
                        .to_owned(),
                );
            }
            Some(RowDisplay::Goal(gd)) => gd,
        };
        let last_goal_date = last_goal
            .done
            .ok_or_else(|| "Last Goal marked as 'done' but doesn't have a done date!".to_owned())?;

        let mut last_date_str: MiniString<MEDSTORE> = MiniString::new();
        let mut last_date_delta: MiniString<MEDSTORE> = MiniString::new();
        let mut last_due_delta: MiniString<MEDSTORE> = MiniString::new();

        last_goal_date
            .format_into(&mut last_date_str, DATE_FMT)
            .map_err(|e| {
                format!(
                    "Error formatting last due date {:?}: {}",
                    &last_goal_date, &e
                )
            })?;

        match (last_goal_date - *today).whole_days() {
            i @ 2..=i64::MAX => write!(&mut last_date_delta, "in {} days", i),
            1 => write!(&mut last_date_delta, "tomorrow"),
            0 => write!(&mut last_date_delta, "today"),
            -1 => write!(&mut last_date_delta, "yesterday"),
            i @ i64::MIN..=-2 => write!(&mut last_date_delta, "{} days ago", -i),
        }
        .map_err(|e| format!("Error writing time since last goal: {}", &e))?;

        match &last_goal.due {
            Some(done) => match (*done - last_goal_date).whole_days() {
                i @ 2..=i64::MAX => write!(&mut last_due_delta, "{} days early", &i),
                1 => write!(&mut last_due_delta, "one day early"),
                0 => write!(&mut last_due_delta, "on time"),
                -1 => write!(&mut last_due_delta, "one day late"),
                i @ i64::MIN..=-2 => write!(&mut last_due_delta, "{} days late", -i),
            },
            None => write!(&mut last_due_delta, "unscheduled"),
        }
        .map_err(|e| format!("Error writing last goal's promptness: {}", &e))?;

        format!(
            "\nYour student last completed a goal {}, on {} ({}).\n",
            &last_date_delta, &last_date_str, &last_due_delta
        )
    } else {
        String::new()
    };

    let data = EmailData {
        full_name,
        date,
        n_due_str,
        last_done_statement,
        service_uri,
        uname: pd.uname,
        n_done: pd.n_done,
        n_scheduled: pd.n_scheduled,
        teacher: pd.teacher,
        temail: pd.temail,
    };

    render_raw_template("boss_email", &data)
}

/// Structure for sending/receiving parent email text to/from the frontend
/// for editing before making a Sendgrid request to actually send the email.
#[derive(Deserialize, Serialize)]
struct EmailEnvelope {
    uname: String,
    student_name: Option<String>,
    text: String,
}

/**
Generate a parent email and send it to the frontend for editing.

Req'ments:
```text
x-camp-action: compose-email
```
Body should contain `uname` of student about whom to generate an email.
*/
async fn compose_email(body: Option<String>, glob: Arc<RwLock<Glob>>) -> Response {
    let uname = match body {
        Some(uname) => uname,
        None => {
            return respond_bad_request(
                "Request must include the uname of subject Student as a body.".to_owned(),
            );
        }
    };

    let (text, student_name) = {
        let glob = glob.read().await;
        let p = match glob.get_pace_by_student(&uname).await {
            Ok(p) => p,
            Err(e) => {
                log::error!("Error getting pace for Student {:?}: {}", &uname, &e);
                return text_500(Some(format!(
                    "Error retrieving pace information for {:?}: {}",
                    &uname, &e
                )));
            }
        };

        let pd = match PaceDisplay::from(&p, &glob) {
            Ok(pd) => pd,
            Err(e) => {
                log::error!(
                    "Error generating PaceDisplay info for Student {:?}: {}\npace data: {:?}",
                    &uname,
                    &e,
                    &p
                );
                return text_500(Some(format!(
                    "Error generating pace display information for {:?}: {}",
                    &uname, &e
                )));
            }
        };

        let student_name = format!("{} {}", pd.rest, pd.last);
        let today = crate::now();

        let text = match generate_email(pd, &glob.uri, &today) {
            Ok(text) => text,
            Err(e) => {
                log::error!(
                    "Error generating parent email text for {:?}: {}\npace data: {:?}",
                    &uname,
                    &e,
                    &p
                );
                return text_500(Some(format!("Error generating email: {}", &e)));
            }
        };

        (text, student_name)
    };

    let data = EmailEnvelope {
        uname,
        student_name: Some(student_name),
        text,
    };

    (
        StatusCode::OK,
        [(
            HeaderName::from_static("x-camp-action"),
            HeaderValue::from_static("edit-email"),
        )],
        Json(data),
    )
        .into_response()
}

/// Data required to render the `"boss_parent_email"` template, generating the
/// JSON body of a Sendgrid request to send a parent email.
#[derive(Debug, Serialize)]
struct SendgridData<'a> {
    /// parent email address
    pub parent: &'a str,
    /// student name
    pub name: &'a str,
    /// text of the email (as rendered from the `"boss_email"` template)
    pub text: &'a str,
}

/**
Respond to a request to send a parent email.

Req'ments:
```text
x-camp-action: send-email
```
Body should JSON-deserialize to an `EmailEnvelope` with the appropriate
`text` body and `uname` user name.
*/
async fn send_email(body: Option<String>, glob: Arc<RwLock<Glob>>) -> Response {
    let body = match body {
        Some(body) => body,
        None => {
            return respond_bad_request(
                "Request must have application/json body with email details.".to_owned(),
            );
        }
    };

    let env: EmailEnvelope = match serde_json::from_str(&body) {
        Ok(env) => env,
        Err(e) => {
            log::error!(
                "Error deserializing JSON as EmailEnvelope: {}\nJSON data: {:?}",
                &e,
                &body
            );
            return text_500(Some(format!(
                "Unable to deserialize body to EmailEnvelope: {}",
                &e
            )));
        }
    };

    {
        let glob = glob.read().await;
        let stud = match glob.users.get(&env.uname) {
            Some(User::Student(s)) => s,
            x => {
                log::error!(
                    "EmailEnvelope uname {:?} is not a Student; is {:?}",
                    env.uname,
                    x
                );
                return text_500(Some(format!(
                    "{:?} is not the user name of a Student.",
                    env.uname
                )));
            }
        };

        let mut name: MiniString<MEDSTORE> = MiniString::new();
        if let Err(e) = write!(&mut name, "{} {}", &stud.rest, &stud.last) {
            log::error!("Error writing student name as MiniString: {}", &e);
            return text_500(Some(format!("Error writing student name: {}", &e)));
        }

        let data = SendgridData {
            parent: &stud.parent,
            name: name.as_str(),
            text: &env.text,
        };

        let request_body = match render_json_template("boss_parent_email", &data) {
            Ok(bod) => bod,
            Err(e) => {
                log::error!("Error rendering template: {}\ndata: {:?}", &e, &data);
                return text_500(Some(format!("Error generating sendgrid request: {}", &e)));
            }
        };

        if let Err(e) = make_sendgrid_request(request_body, &glob, name).await {
            log::error!("Error making Sendgrid request: {}", &e);
            return text_500(Some(format!("Error making Sendgrid request: {}", &e)));
        }
    }

    (
        StatusCode::OK,
        [(
            HeaderName::from_static("x-camp-action"),
            HeaderValue::from_static("none"),
        )],
    )
        .into_response()
}

/// Directly generate a JSON Sendgrid request body (bypassing the round-trip)
/// to the frontend for editing.
///
/// This is used when auto-emailing parents of _all_ students at once.
fn sendgrid_request_from_pace(p: &Pace, glob: &Glob, today: &Date) -> Result<String, String> {
    let pd = PaceDisplay::from(p, glob)
        .map_err(|e| format!("Error generating pace display info: {}", &e))?;
    let email_body = generate_email(pd, &glob.uri, today)
        .map_err(|e| format!("Error generating email: {}", &e))?;
    let name = format!("{}, {}", &p.student.rest, &p.student.last);
    let data = SendgridData {
        parent: &p.student.parent,
        name: &name,
        text: &email_body,
    };
    render_json_template("boss_parent_email", &data)
        .map_err(|e| format!("Error rendering Sendgrid request template: {}", &e))
}

/**
Respond to a request to email the parents of _all_ students.

This does not allow for editing any of the emails like sending them
individually does.

Req'ments:
```
x-camp-action: email-all
```

Use sparingly.
*/
async fn email_all(glob: Arc<RwLock<Glob>>) -> Response {
    let mut failures: Vec<String> = Vec::new();

    {
        let glob = glob.read().await;
        let tunames: Vec<&str> = glob
            .users
            .iter()
            .map(|(uname, user)| match user {
                User::Teacher(_) => Some(uname),
                _ => None,
            })
            .filter(|opt| opt.is_some())
            .map(|ok| ok.unwrap().as_str())
            .collect();

        {
            let mut retrievals = FuturesUnordered::new();
            let mut sends = FuturesUnordered::new();

            for tuname in tunames.iter() {
                retrievals.push(glob.get_paces_by_teacher(tuname));
            }

            let today = crate::now();

            while let Some(res) = retrievals.next().await {
                match res {
                    Ok(mut pace_vec) => {
                        for p in pace_vec.drain(..) {
                            match sendgrid_request_from_pace(&p, &glob, &today) {
                                Ok(req_body) => {
                                    let mut name: MiniString<MEDSTORE> = MiniString::new();
                                    if let Err(e) = write!(
                                        &mut name,
                                        "{}, {}",
                                        &p.student.last, &p.student.rest
                                    ) {
                                        let estr = format!(
                                            "{}, {}: Error writing student name: {}",
                                            &p.student.last, &p.student.rest, &e
                                        );
                                        failures.push(estr);
                                        continue;
                                    }
                                    sends.push(make_sendgrid_request(req_body, &glob, name));
                                }
                                Err(e) => {
                                    let estr =
                                        format!("{}, {}: {}", &p.student.last, &p.student.rest, &e);
                                    failures.push(estr);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let estr = format!("Error retrieving some goals: {}", &e);
                        failures.push(estr);
                    }
                }
            }

            while let Some(res) = sends.next().await {
                if let Err(e) = res {
                    failures.push(e);
                }
            }
        }
    }

    if failures.is_empty() {
        (
            StatusCode::OK,
            [(
                HeaderName::from_static("x-camp-action"),
                HeaderValue::from_static("none"),
            )],
        )
            .into_response()
    } else {
        let err_body = format!(
            "Encountered the following errors while emailing all students' parents:\n{}",
            failures.join("\n")
        );

        (
            StatusCode::from_u16(512).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            err_body,
        )
            .into_response()
    }
}

async fn download_report(headers: &HeaderMap, glob: Arc<RwLock<Glob>>) -> Response {
    let suname = match get_head("x-camp-student", headers) {
        Ok(uname) => uname,
        Err(e) => { return respond_bad_request(e); },
    };
    let term = match get_head("x-camp-term", headers) {
        Ok(term) => term,
        Err(e) => { return respond_bad_request(e); },
    };
    let term = match Term::from_str(term) {
        Ok(term) => term,
        Err(e) => {
            log::warn!(
                "Invalid x-camp-term value ({:?}) in attempt to download report for {:?}: {}",
                term, suname, &e
            );
            return respond_bad_request(format!(
                "Invalid x-camp-term value {:?}: {}", term, &e
            ));
        },
    };

    let glob = glob.read().await;

    let stud = match glob.users.get(suname) {
        Some(User::Student(s)) => s,
        _ => {
            log::warn!(
                "Report for non-student {:?} requested.", suname
            );
            return respond_bad_request(format!(
                "{:?} is not the user name of a student in the system.", suname
            ));
        },
    };

    let pdf_data = {
        let data_handle = glob.data();
        let data = data_handle.read().await;
        let mut client = match data.connect().await {
            Ok(c) => c,
            Err(e) => {
                log::error!(
                    "Error getting DB connection to retrieve report PDF for {:?}: {}",
                    suname, &e
                );
                return text_500(Some(format!(
                    "Error connecting to the database: {}", &e
                )));
            },
        };
        let t = match client.transaction().await {
            Ok(t) => t,
            Err(e) => {
                log::error!(
                    "Error opening Transaction to retrieve report PDF for {:?}: {}",
                    suname, &e
                );
                return text_500(Some(format!(
                    "Error initiating database transaction: {}", &e
                )));
            },
        };

        let pdf_data = match Store::get_final(&t, suname, term).await {
            Ok(Some(v)) => v,
            Ok(None) => {
                return (
                    StatusCode::NOT_FOUND,
                    format!(
                        "{} {} does not yet have a {} report in the system.",
                        &stud.rest, &stud.last, &term
                    ),
                ).into_response();
            },
            Err(e) => {
                log::error!(
                    "Error querying database for {} report for {:?}: {}",
                    &term, suname, &e
                );
                return text_500(Some(format!(
                    "Error retrieving report from database: {}", &e
                )));
            },
        };

        if let Err(e) = t.commit().await {
            log::error!(
                "<WEIRD!> Error committing transaction to retrieve {} PDF report for {:?}: {}",
                &term, suname, &e
            );
            return text_500(Some(format!(
                "Error committing transaction (weird, I know): {}", &e
            )));
        }

        pdf_data
    };

    // The first thing this function does is respond with an error if there's
    // no "x-camp-student" or "x-camp-term" headers, so these are both
    // guaranteed to be here.
    let suname_header = headers.get("x-camp-student").unwrap().clone();
    let term_header = headers.get("x-camp-term").unwrap().clone();

    (
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/pdf"),
            ),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_static("inline"),
            ),
            (
                HeaderName::from_static("x-camp-action"),
                HeaderValue::from_static("download-pdf"),
            ),
            (
                HeaderName::from_static("x-camp-student"),
                suname_header,
            ),
            (
                HeaderName::from_static("x-camp-term"),
                term_header,
            ),
        ],
        pdf_data
    ).into_response()
}

async fn download_archive(headers: &HeaderMap, glob: Arc<RwLock<Glob>>) -> Response {
    let tuname = match get_head("x-camp-teacher", headers) {
        Ok(uname) => uname,
        Err(e) => { return respond_bad_request(e); },
    };
    let term_str = match get_head("x-camp-term", headers) {
        Ok(term) => term,
        Err(e) => { return respond_bad_request(e); },
    };
    let term = match Term::from_str(term_str) {
        Ok(term) => term,
        Err(e) => {
            log::warn!(
                "Invalid x-camp-term value ({:?}) in attempt to download report for {:?}: {}",
                term_str, tuname, &e
            );
            return respond_bad_request(format!(
                "Invalid x-camp-term value {:?}: {}", term_str, &e
            ));
        },
    };

    let glob = glob.read().await;
    let data = match glob.get_reports_archive_by_teacher(tuname, term).await {
        Ok(Some(bytes)) => bytes,
        Ok(None) => {
            return (
                StatusCode::NOT_FOUND,
                format!(
                    "{} does not have any {} reports completed.",
                    tuname, term.as_str()
                ),
            ).into_response();
        },
        Err(e) => {
            log::error!(
                "Error attempting to generate {} report archive for {:?}: {}",
                term_str, tuname, &e
            );
            return text_500(Some(format!(
                "Error generating archive: {}", &e
            )));
        },
    };

    let disposition_str = format!(
        "attachment; filename=\"{}_{}.zip\"", tuname, term_str
    );
    let disposition_value = match HeaderValue::from_str(&disposition_str) {
        Ok(val) => val,
        Err(e) => {
            log::error!(
                "Error generating Content-Disposition header value ({:?}): {}",
                &disposition_str, &e
            );
            return text_500(Some(format!(
                "Error generating Content-Disposition header value: {}", &e
            )));
        },
    };

    (
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/zip"),
            ),
            (
                header::CONTENT_DISPOSITION,
                disposition_value,
            ),
            (
                HeaderName::from_static("x-camp-action"),
                HeaderValue::from_static("download-archive"),
            ),
        ],
        data
    ).into_response()
}
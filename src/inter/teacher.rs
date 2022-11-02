/*!
Subcrate for interoperation with Teacher users.
*/
use std::{
    collections::HashMap,
    io::Cursor,
    str::FromStr,
};

use axum::{
    extract::Extension,
    http::header,
    http::header::{HeaderMap, HeaderName, HeaderValue},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::Date;
use tokio::sync::RwLock;

use super::*;
use crate::{
    auth::AuthResult,
    config::Glob,
    course::Course,
    pace::{maybe_parse_score_str, BookCh, Goal, Pace, Source, Term},
    report, report::ReportSidecar,
    store::Store,
    user::*,
    DATE_FMT,
};

/// Attempt to parse a [`Date`] of the format "2021-01-27", which is the
/// format in which the frontend encodes dates.
fn maybe_parse_date(date_opt: Option<&str>) -> Result<Option<Date>, String> {
    match date_opt {
        Some(date_str) => match Date::parse(date_str, DATE_FMT) {
            Ok(d) => Ok(Some(d)),
            Err(e) => Err(format!("Unable to parse {:?} as Date: {}", date_str, &e)),
        },
        None => Ok(None),
    }
}

/// Check that the teacher's login credentials check out, generate them a
/// key, then serve them the page that contains their view.
pub async fn login(t: Teacher, form: LoginData, glob: Arc<RwLock<Glob>>) -> Response {
    log::trace!(
        "teacher::login( {:?}, ... , [ glob ]) called.",
        &t.base.uname
    );

    let auth_response = {
        glob.read()
            .await
            .auth()
            .read()
            .await
            .check_password_and_issue_key(&t.base.uname, &form.password, &t.base.salt)
            .await
    };

    let auth_key = match auth_response {
        Err(e) => {
            log::error!(
                "auth::Db::check_password_and_issue_key( {:?}, {:?}, {:?} ): {}",
                &t.base.uname,
                &form.password,
                &t.base.salt,
                &e
            );

            return html_500();
        }
        Ok(AuthResult::Key(k)) => k,
        Ok(AuthResult::BadPassword) => {
            return respond_bad_password(&t.base.uname);
        }
        Ok(x) => {
            log::warn!(
                "auth::Db::check_password_and_issue_key( {:?}, {:?}. {:?} returned {:?}, which shouldn't ever happen.",
                &t.base.uname, &form.password, &t.base.salt, &x
            );

            return respond_bad_password(&t.base.uname);
        }
    };

    let data = json!({
        "uname": &t.base.uname,
        "key": &auth_key,
        "name": &t.name,
    });

    serve_template(StatusCode::OK, "teacher", &data, vec![])
}

/**
All requests from the teacher's front-end view get funneled through this
function.

This will then generate the response that should be sent back to the teacher's
front-end, regardless of whether it's an error.

A previous layer should have already ensured that the teacher's key
checks out.
*/
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

    // While a service layer has already checked that the request's `uname`
    // header and `key` header are a valid combination, we have not yet
    // verified that the user in question is actually a _teacher_. Here's
    // where that gets checked.
    match u {
        User::Teacher(_) => { /* Okay, approved, you can be here. */ }
        _ => {
            return (
                StatusCode::FORBIDDEN,
                "Who is this? What's you're operating number?".to_owned(),
            )
                .into_response();
        }
    }

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
        "populate-dates" => populate_dates(glob.clone()).await,
        "populate-courses" => populate_courses(glob.clone()).await,
        "populate-goals" => populate_goals(&headers, glob.clone()).await,
        "populate-traits" => populate_traits(glob.clone()).await,
        "add-goal" => insert_goal(body, glob.clone()).await,
        "update-goal" => update_goal(body, glob.clone()).await,
        "delete-goal" => delete_goal(body, glob.clone()).await,
        "update-numbers" => update_numbers(body, glob.clone()).await,
        "autopace" => autopace(body, glob.clone()).await,
        "clear-goals" => clear_goals(body, glob.clone()).await,
        "upload-goals" => upload_goals(&headers, body, glob.clone()).await,
        "show-sidecar" => show_sidecar(&headers, body, glob.clone()).await,
        "update-sidecar" => update_sidecar(&headers, body, glob.clone()).await,
        "render-report" => generate_report(&headers, body, glob.clone()).await,
        "discard-pdf" => discard_pdf(&headers, glob.clone()).await,
        x => respond_bad_request(format!("{:?} is not a recognized x-camp-action value.", &x)),
    }
}

/**
Generate a response to a request that requires no other data or action.
*/
fn respond_ok() -> Response {
    (
        StatusCode::OK,
        [(
            HeaderName::from_static("x-camp-action"),
            HeaderValue::from_static("none"),
        )]
    ).into_response()
}

/**
Respond to a request for the "special dates" stored in the database.

Header which gets us here:
```
x-camp-action: populate-dates
```
*/
async fn populate_dates(glob: Arc<RwLock<Glob>>) -> Response {
    let dates_bucket: HashMap<String, String> = glob
        .read()
        .await
        .dates
        .iter()
        .map(|(n, d)| (n.clone(), d.to_string()))
        .collect();

    (
        StatusCode::OK,
        [(
            HeaderName::from_static("x-camp-action"),
            HeaderValue::from_static("populate-dates"),
        )],
        Json(&dates_bucket),
    )
        .into_response()
}

/*
The `ChapterData` and `CourseData` structs serialize data from `Chapter`
and `Course` structs in a way that the frontend expects them.
*/

#[derive(Debug, Deserialize, Serialize)]
struct ChapterData<'a> {
    id: i64,
    sym: &'a str,
    seq: i16,
    title: &'a str,
    subject: Option<&'a str>,
    weight: f32,
}

#[derive(Debug, Deserialize, Serialize)]
struct CourseData<'a> {
    id: i64,
    sym: &'a str,
    book: &'a str,
    title: &'a str,
    level: f32,
    weight: f32,
    chapters: Vec<ChapterData<'a>>,
}

impl<'a> CourseData<'a> {
    /// Extract and format the data necessary to send information about a
    /// `Course` to the frontend.
    fn from_course(crs: &'a Course) -> Result<CourseData<'a>, String> {
        let tot_wgt = match crs.weight {
            None => {
                return Err(format!(
                    "Course {:?} ({}) doesn't have an initialized total weight.",
                    &crs.sym, &crs.title
                ));
            }
            Some(w) => {
                if w < 0.0001 {
                    return Err(format!(
                        "Total course weight of course {:?} ({}) risks division by zero.",
                        &crs.sym, &crs.title
                    ));
                } else {
                    w
                }
            }
        };

        let chapters: Vec<ChapterData<'a>> = crs
            .all_chapters()
            .map(|ch| ChapterData {
                id: ch.id,
                sym: &crs.sym,
                seq: ch.seq,
                title: &ch.title,
                subject: match &ch.subject {
                    Some(s) => Some(s.as_str()),
                    None => None,
                },
                weight: ch.weight / tot_wgt,
            })
            .collect();

        let crsd = CourseData {
            id: crs.id,
            sym: &crs.sym,
            book: &crs.book,
            title: &crs.title,
            level: crs.level,
            weight: tot_wgt,
            chapters,
        };
        Ok(crsd)
    }
}

/**
Respond to a request for data about all the courses in the system.

Header that gets us here:
```
x-camp-action: populate-courses
```
*/
async fn populate_courses(glob: Arc<RwLock<Glob>>) -> Response {
    let glob = glob.read().await;

    let mut course_data: Vec<CourseData> = Vec::with_capacity(glob.courses.len());
    for (_, crs) in glob.courses.iter() {
        match CourseData::from_course(crs) {
            Ok(crsd) => {
                course_data.push(crsd);
            }
            Err(e) => {
                log::warn!("Error serializing: {}", &e);
            }
        }
    }

    (
        StatusCode::OK,
        [(
            HeaderName::from_static("x-camp-action"),
            HeaderValue::from_static("populate-courses"),
        )],
        Json(&course_data),
    )
        .into_response()
}

/*
The GoalData and CourseData structs serialize data about student pace
and goals in a way that the frontend expects them.

The GaolData will also _deserialize_ information about a goal as sent
from the fontend.
*/

#[derive(Debug, Deserialize, Serialize)]
struct GoalData<'a> {
    id: i64,
    #[serde(skip_serializing)]
    uname: &'a str,
    sym: &'a str,
    seq: i16,
    rev: bool,
    inc: bool,
    due: Option<String>,
    done: Option<String>,
    tries: Option<i16>,
    weight: f32,
    score: Option<&'a str>,
}

impl<'a> GoalData<'a> {
    /// Called upon receipt of new or updated information about a `Goal` from
    /// the frontend to convert that data into the appropriate internal format.
    fn into_goal(self) -> Result<Goal, String> {
        let source = BookCh {
            sym: self.sym.to_owned(),
            seq: self.seq,
            // doesn't matter on insertion
            level: 0.0,
        };

        let _ = maybe_parse_score_str(self.score)?;

        let g = Goal {
            id: self.id,
            uname: self.uname.to_owned(),
            source: Source::Book(source),
            review: self.rev,
            incomplete: self.inc,
            due: maybe_parse_date(self.due.as_deref())
                .map_err(|e| format!("Bad due date: {}", &e))?,
            done: maybe_parse_date(self.done.as_deref())
                .map_err(|e| format!("Bad done date: {}", &e))?,
            tries: self.tries,
            weight: self.weight,
            score: self.score.map(|s| s.to_owned()),
        };

        Ok(g)
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct PaceData<'a> {
    uname: &'a str,
    last: &'a str,
    rest: &'a str,
    tuname: &'a str,
    total_weight: f32,
    due_weight: f32,
    done_weight: f32,
    goals: Vec<GoalData<'a>>,
    /// Fall/Spring exams
    fex: Option<&'a str>,
    sex: Option<&'a str>,
    fex_frac: f32,
    sex_frac: f32,
    /// Fall/Spring notices
    fnot: i16,
    snot: i16,
}

impl<'a> PaceData<'a> {
    /// Marshal the data from a student's pace calendar in a way that can
    /// get serialized for transmission to the frontend.
    pub fn from_pace(pcal: &'a Pace) -> Result<PaceData, String> {
        let mut goals: Vec<GoalData> = Vec::with_capacity(pcal.goals.len());
        for g in pcal.goals.iter() {
            let src = match &g.source {
                Source::Book(bch) => bch,
                _ => {
                    return Err(format!(
                        "Student {:?} ({}, {}) has Goal w/ (unsupported) custom Source.",
                        &pcal.student.base.uname, &pcal.student.last, &pcal.student.rest
                    ));
                }
            };

            let gdat = GoalData {
                id: g.id,
                uname: "",
                sym: &src.sym,
                seq: src.seq,
                rev: g.review,
                inc: g.incomplete,
                due: g.due.map(|d| d.to_string()),
                done: g.done.map(|d| d.to_string()),
                tries: g.tries,
                weight: g.weight,
                score: g.score.as_deref(),
            };

            goals.push(gdat);
        }

        let pdat = PaceData {
            uname: &pcal.student.base.uname,
            last: &pcal.student.last,
            rest: &pcal.student.rest,
            tuname: &pcal.teacher.base.uname,
            total_weight: pcal.total_weight,
            due_weight: pcal.due_weight,
            done_weight: pcal.done_weight,
            goals,
            fex: pcal.student.fall_exam.as_deref(),
            sex: pcal.student.spring_exam.as_deref(),
            fex_frac: pcal.student.fall_exam_fraction,
            sex_frac: pcal.student.spring_exam_fraction,
            fnot: pcal.student.fall_notices,
            snot: pcal.student.spring_notices,
        };

        Ok(pdat)
    }
}

/**
Respond to a request for data about all the goals for all the students of
the logged-in teacher.

The header to get this:
```
x-camp-action: populate-goals
```
*/
async fn populate_goals(headers: &HeaderMap, glob: Arc<RwLock<Glob>>) -> Response {
    let uname = match get_head("x-camp-uname", headers) {
        Ok(uname) => uname,
        Err(e) => { return text_500(Some(e)); }
    };

    let pace_cals = match glob.read().await.get_paces_by_teacher(uname).await {
        Ok(goals) => goals,
        Err(e) => {
            return text_500(Some(format!("{}", &e)));
        }
    };

    let mut pace_data: Vec<PaceData> = Vec::with_capacity(pace_cals.len());
    for p in pace_cals.iter() {
        match PaceData::from_pace(p) {
            Ok(pd) => {
                pace_data.push(pd);
            }
            Err(e) => {
                log::error!("{}", &e);
            }
        }
    }

    (
        StatusCode::OK,
        [(
            HeaderName::from_static("x-camp-action"),
            HeaderValue::from_static("populate-goals"),
        )],
        Json(pace_data),
    )
        .into_response()
}

async fn populate_traits(glob: Arc<RwLock<Glob>>) -> Response {
    let glob = glob.read().await;

    (
        StatusCode::OK,
        [(
            HeaderName::from_static("x-camp-action"),
            HeaderValue::from_static("populate-traits"),
        )],
        Json(&glob.social_traits),
    )
        .into_response()
}

/**
Send a single pace calendar's worth of data (for student `uname`) to
the frontend.

This is generally called by a handler function to generate a response when
the teacher makes a request that alter's a student's pace calendar in some way.
*/
async fn update_pace(uname: &str, glob: Arc<RwLock<Glob>>) -> Response {
    let p = match glob.read().await.get_pace_by_student(uname).await {
        Ok(p) => p,
        Err(e) => {
            log::error!("Error getting Pace for student {:?}: {}", uname, &e);
            return text_500(Some(format!(
                "Error retrieving updated Pace from database: {}",
                &e
            )));
        }
    };

    let pdata = match PaceData::from_pace(&p) {
        Ok(pdata) => pdata,
        Err(e) => {
            return text_500(Some(format!("Unable to serialize response: {}", &e)));
        }
    };

    (
        StatusCode::OK,
        [(
            HeaderName::from_static("x-camp-action"),
            HeaderValue::from_static("update-pace"),
        )],
        Json(pdata),
    )
        .into_response()
}

/**
Insert a new `Goal` into the database.

What' gets us here:
```
x-camp-action: add-goal
```
And the request body should be JSON-deserializable into a `GoalData`.
*/
async fn insert_goal(body: Option<String>, glob: Arc<RwLock<Glob>>) -> Response {
    let body = match body {
        Some(body) => body,
        None => {
            return respond_bad_request(
                "Request needs application/json body with Goal details.".to_owned(),
            );
        }
    };

    let gdata: GoalData = match serde_json::from_str(&body) {
        Ok(gdata) => gdata,
        Err(e) => {
            log::error!("Error deserialzing {:?} as GoalData: {}", &body, &e);
            return text_500(Some("Unable to deserializse as GoalData.".to_owned()));
        }
    };

    let g = match gdata.into_goal() {
        Ok(g) => g,
        Err(e) => {
            return text_500(Some(format!("Error reading Goal data: {}", &e)));
        }
    };

    if let Err(e) = glob
        .read()
        .await
        .data()
        .read()
        .await
        .insert_one_goal(&g)
        .await
    {
        log::error!("Error inserting Goal {:?} into database: {}", &g, &e);
        return text_500(Some(format!("Error inserting Goal into database: {}", &e)));
    }

    update_pace(&g.uname, glob).await
}

/**
Respond to a request to update a single goal in the database.

Header that gets us here:
```
x-camp-action: update-goal
```
The body of the request should be JSON-deserializable into a `GoalData` with
the `id` of the [`Goal`] to change and the updated data.
*/
async fn update_goal(body: Option<String>, glob: Arc<RwLock<Glob>>) -> Response {
    let body = match body {
        Some(body) => body,
        None => {
            return respond_bad_request(
                "Request needs application/json body with Goal details.".to_owned(),
            );
        }
    };

    let gdata: GoalData = match serde_json::from_str(&body) {
        Ok(gdata) => gdata,
        Err(e) => {
            log::error!("Error deserialzing {:?} as GoalData: {}", &body, &e);
            return text_500(Some("Unable to deserializse as GoalData.".to_owned()));
        }
    };

    let g = match gdata.into_goal() {
        Ok(g) => g,
        Err(e) => {
            return text_500(Some(format!("Error reading Goal data: {}", &e)));
        }
    };

    if let Err(e) = glob.read().await.data().read().await.update_goal(&g).await {
        log::error!("Error inserting Goal {:?} into database: {}", &g, &e);
        return text_500(Some(format!("Error inserting Goal into database: {}", &e)));
    }

    update_pace(&g.uname, glob).await
}

/**
Respond to a request to delete a single goal from the database.

Header that gets us here:
```
x-camp-action: delete-goal
```
With a body parseable into the `id` of the [`Goal`] to delete.
*/
async fn delete_goal(body: Option<String>, glob: Arc<RwLock<Glob>>) -> Response {
    let body = match body {
        Some(body) => body,
        None => {
            return respond_bad_request(
                "Request needs application/json body with Goal details.".to_owned(),
            );
        }
    };

    let id: i64 = match &body.parse() {
        Ok(n) => *n,
        Err(e) => {
            log::error!("Error deserializing {:?} as i64: {}", &body, &e);
            return text_500(Some("Unable to deserialize into integer.".to_owned()));
        }
    };

    let uname = match glob.read().await.data().read().await.delete_goal(id).await {
        Ok(uname) => uname,
        Err(e) => {
            log::error!("Error deleting Goal w/id {} from database: {}", &id, &e);
            return text_500(Some(format!("Error deleting from database: {}", &e)));
        }
    };

    update_pace(&uname, glob).await
}

/**
Respond to a request to update the exam/notice data in the expandable
"more" row at the bottom of a student's pace calendar display in the
Teacher view.

Header:
```
x-camp-action: update-numbers
```
The body should JSON-deserialize into a `PaceData` struct with the six values
in question set appropriately.
*/
async fn update_numbers(body: Option<String>, glob: Arc<RwLock<Glob>>) -> Response {
    let body = match body {
        Some(body) => body,
        None => {
            return respond_bad_request(
                "Request needs application/json body with PaceData details.".to_owned(),
            );
        }
    };

    let pdata: PaceData = match serde_json::from_str(&body) {
        Ok(pdata) => pdata,
        Err(e) => {
            log::error!("Error deserializing {:?} into PaceData: {}", &body, &e);
            return text_500(Some("Unable to deserialize request data.".to_owned()));
        }
    };

    log::debug!("update_numbers() rec'd body:\n{:#?}\n", &pdata);

    let mut s = match glob.read().await.users.get(pdata.uname) {
        Some(User::Student(s)) => s.clone(),
        _ => {
            log::error!("Data uname {:?} not a Student.", &pdata.uname);
            return text_500(Some(format!("{:?} is not a Student.", &pdata.uname)));
        }
    };

    s.fall_notices = pdata.fnot;
    s.spring_notices = pdata.snot;
    s.fall_exam = match maybe_parse_score_str(pdata.fex) {
        Err(e) => {
            log::error!("Error parsing fall exam score from {:?}: {}.", &pdata, &e);
            return text_500(Some(format!(
                "{:?} is not a valid Fall Exam score: {}",
                pdata.fex, &e
            )));
        }
        Ok(Some(_)) => pdata.fex.map(|s| s.to_string()),
        Ok(None) => None,
    };
    s.spring_exam = match maybe_parse_score_str(pdata.sex) {
        Err(e) => {
            log::error!("Error parsing spring exam score from {:?}: {}.", &pdata, &e);
            return text_500(Some(format!(
                "{:?} is not a valid Spring Exam score: {}",
                pdata.sex, &e
            )));
        }
        Ok(Some(_)) => pdata.sex.map(|s| s.to_string()),
        Ok(None) => None,
    };
    s.fall_exam_fraction = pdata.fex_frac;
    s.spring_exam_fraction = pdata.sex_frac;

    {
        let mut glob = glob.write().await;
        let data = glob.data();
        let data_reader = data.read().await;
        let mut client = match data_reader.connect().await {
            Ok(c) => c,
            Err(e) => {
                log::error!("Error connection with database: {}", &e);
                return text_500(Some(format!("Error connecting w/database: {}", &e)));
            }
        };
        let t = match client.transaction().await {
            Ok(t) => t,
            Err(e) => {
                log::error!("Error beginning transaction: {}", &e);
                return text_500(Some(format!(
                    "Error beginning database transaction: {}",
                    &e
                )));
            }
        };

        if let Err(e) = data_reader.update_student(&t, &s).await {
            log::error!("Error updating student w/ data {:?}: {}", &s, &e);
            return text_500(Some(format!("Error updating student: {}", &e)));
        }

        if let Err(e) = t.commit().await {
            log::error!("Error committing transaction: {}", &e);
            return text_500(Some(format!(
                "Error committing database transaction: {}",
                &e
            )));
        }

        if let Err(e) = glob.refresh_users().await {
            log::error!("Error refreshing user hash from database: {}", &e);
            return text_500(Some("Unable to reread users from database.".to_owned()));
        }
    }

    update_pace(pdata.uname, glob).await
}

/**
Respond to a request to autopace a student's goals.

Header:
```
x-camp-action: autopace
```
With a body containing the `uname` of the student to autopace.
*/
async fn autopace(body: Option<String>, glob: Arc<RwLock<Glob>>) -> Response {
    let body = match body {
        Some(body) => body,
        None => {
            return respond_bad_request("Request needs Student user name in body.".to_owned());
        }
    };

    let uname: &str = &body;

    {
        let glob = glob.read().await;
        let mut p = match glob.get_pace_by_student(uname).await {
            Ok(p) => p,
            Err(e) => {
                log::error!("Error retrieving pace data for {:?}: {}", uname, &e);
                return text_500(Some(format!(
                    "Error retrieving pace data from database: {}",
                    &e
                )));
            }
        };

        if let Err(e) = p.autopace(&glob.calendar) {
            log::error!(
                "Error calling Pace::autopace( [ {} dates ] ) for {:?}: {}",
                &glob.calendar.len(),
                &p,
                &e
            );
            return text_500(Some(format!("Error pacing due dates: {}", &e)));
        }

        let data = glob.data();
        if let Err(e) = data.read().await.update_due_dates(&p.goals).await {
            log::error!("Error updating dates from {:?}: {}", &p, &e);
            return text_500(Some(format!(
                "Error updating due dates in database: {}",
                &e
            )));
        };
    }

    update_pace(uname, glob).await
}

/**
Respond to a request to delete all of a student's goals.

Header:
```
x-camp-action: clear-goals
```
With a body containing the `uname` of the student in question.
*/
async fn clear_goals(body: Option<String>, glob: Arc<RwLock<Glob>>) -> Response {
    let body = match body {
        Some(body) => body,
        None => {
            return respond_bad_request("Request needs student user name in body.".to_owned());
        }
    };

    let uname: &str = &body;

    {
        let glob = glob.read().await;
        let data = glob.data();
        let data_reader = data.read().await;
        let mut client = match data_reader.connect().await {
            Ok(client) => client,
            Err(e) => {
                let estr = format!("Error connecting to database: {}", &e);
                log::error!("{}", &estr);
                return text_500(Some(estr));
            }
        };
        let t = match client.transaction().await {
            Ok(t) => t,
            Err(e) => {
                let estr = format!("Error beginning transaction: {}", &e);
                log::error!("{}", &estr);
                return text_500(Some(estr));
            }
        };

        if let Err(e) = data_reader.delete_goals_by_student(&t, uname).await {
            log::error!("Error deleting goals for {:?}: {}", uname, &e);
            return text_500(Some(format!("Error deleting goals: {}", &e)));
        }

        if let Err(e) = t.commit().await {
            log::error!("Error committing clear-goals transaction: {}", &e);
            return text_500(Some(format!("Error committing transaction: {}", &e)));
        }
    }

    update_pace(uname, glob).await
}

/**
Respond to a request to add a collection of goals from information in CSV
format.

Header:
```
x-camp-action: upload-goals
```
With the body being the CSV data in question. For the CSV format, see
[`Pace::from_csv`].
*/
async fn upload_goals(
    headers: &HeaderMap,
    body: Option<String>,
    glob: Arc<RwLock<Glob>>,
) -> Response {
    let body = match body {
        Some(body) => body,
        None => {
            return respond_bad_request("Request needs text/csv body of Goal details.".to_owned());
        }
    };

    let tuname = match get_head("x-camp-uname", headers) {
        Ok(uname) => uname,
        Err(e) => { return text_500(Some(e)); },
    };

    let mut others_students = String::new();
    let mut goals: Vec<Goal> = Vec::new();
    {
        let glob = glob.read().await;

        let reader = Cursor::new(body);
        let mut pcals = match Pace::from_csv(reader, &glob) {
            Ok(pcals) => pcals,
            Err(e) => {
                return respond_bad_request(e);
            }
        };

        for p in pcals.iter_mut() {
            if p.teacher.base.uname == tuname {
                goals.append(&mut p.goals);
            } else {
                others_students.push('\n');
                others_students.push_str(&p.student.base.uname);
            }
        }

        if !others_students.is_empty() {
            let mut estr = String::from(
                "The following students with Goals in the Goals file you just submitted are not yours:"
            );
            estr.extend(others_students.drain(..));

            return (StatusCode::FORBIDDEN, estr).into_response();
        }

        match glob.insert_goals(&goals).await {
            Ok(n) => {
                log::trace!("{} inserted {} goals.", tuname, &n);
            }
            Err(e) => {
                log::error!("Error inserting Goals: {}", &e);
                return text_500(Some(format!("Error inserting Goals into database: {}", &e)));
            }
        }
    }

    populate_goals(headers, glob).await
}

async fn show_sidecar(
    headers: &HeaderMap,
    body: Option<String>,
    glob: Arc<RwLock<Glob>>,
) -> Response {
    let body = match body {
        Some(body) => body,
        None => {
            return respond_bad_request("Request needs student user name in body.".to_owned());
        }
    };

    let uname = &body;

    let tuname = match get_head("x-camp-uname", headers) {
        Ok(uname) => uname,
        Err(e) => { return text_500(Some(e)); },
    };

    let glob = glob.read().await;

    match glob.users.get(uname) {
        Some(User::Student(s)) => {
            if s.teacher != tuname {
                let estr = format!("The student {:?} is not yours.", uname);
                return (StatusCode::FORBIDDEN, estr).into_response();
            }
        }
        _ => {
            let estr = format!(
                "The uname {:?} does not belong to a student in the system.",
                uname
            );
            return respond_bad_request(estr);
        }
    }

    let data_guard = glob.data();
    let data = data_guard.read().await;

    let sidecar = match data.get_report_sidecar(uname).await {
        Ok(sc) => sc,
        Err(e) => {
            log::error!("Error fetching sidecar for student {:?}: {}", uname, &e);
            return text_500(Some(format!(
                "Error fetching extra reporting information for {:?}: {}",
                uname, &e
            )));
        }
    };

    (
        StatusCode::OK,
        [(
            HeaderName::from_static("x-camp-action"),
            HeaderValue::from_static("show-sidecar"),
        )],
        Json(sidecar),
    )
        .into_response()
}

async fn update_sidecar(
    headers: &HeaderMap,
    body: Option<String>,
    glob: Arc<RwLock<Glob>>,
) -> Response {
    let body = match body {
        Some(body) => body,
        None => {
            return respond_bad_request(
                "Request needs application/json body with ReportSidecar details.".to_owned(),
            );
        }
    };

    let sidecar: ReportSidecar = match serde_json::from_str(&body) {
        Ok(sc) => sc,
        Err(e) => {
            log::error!(
                "Unable to deserialize as ReportSidecar: {}; data:\n{}",
                &e,
                &body
            );
            let estr = format!(
                "Request body did not deserialize into ReportSidecar: {}",
                &e
            );
            return respond_bad_request(estr);
        }
    };

    let tuname = match get_head("x-camp-uname", headers) {
        Ok(uname) => uname,
        Err(e) => { return text_500(Some(e)); },
    };
    let term = match get_head("x-camp-term", headers) {
        Ok(term) => term,
        Err(e) => { return respond_bad_request(e); }
    };
    let term = match term {
        "fall" => Term::Fall,
        "spring" => Term::Spring,
        x => {
            log::error!(
                "Header \"x-camp-action\" value {:?} shouldn't make it this far.", x
            );
            return respond_bad_request(format!(
                "Unrecognized x-camp-term value {:?}", x
            ));
        },
    };

    let glob = glob.read().await;

    match glob.users.get(&sidecar.uname) {
        Some(User::Student(s)) => {
            if s.teacher != tuname {
                let estr = format!("The student {:?} is not yours.", &sidecar.uname);
                return (StatusCode::FORBIDDEN, estr).into_response();
            }
        },
        _ => {
            let estr = format!(
                "The uname {:?} does not belong to a student in the system.",
                &sidecar.uname
            );
            return respond_bad_request(estr);
        }
    }

    let data_guard = glob.data();
    let data = data_guard.read().await;

    if let Err(e) = data.set_report_sidecar(&sidecar).await {
        log::error!("Error setting report sidecar: {}\ndata: {:?}", &e, &sidecar);
        let estr = format!("Error saving report sidecar info: {}", &e);
        return text_500(Some(estr));
    }

    let text = match report::generate_report_markup(&sidecar.uname, term, &glob).await {
        Ok(text) => text,
        Err(e) => {
            log::error!(
                "Error generating {} report markdown for {:?}: {}",
                term, &sidecar.uname, &e
            );
            return text_500(Some(format!(
                "Error generating report markup: {}", &e
            )));
        },
    };

    let term = match term {
        Term::Fall => HeaderValue::from_static("fall"),
        Term::Spring => HeaderValue::from_static("spring"),
        x => {
            log::error!(
                "Somehow a {:?} has made it this far; this shouldn't happen.", &x
            );
            return respond_bad_request(format!(
                "Reports for {} are as of yet unsupported.", &x
            ));
        },
    };

    let suname = match HeaderValue::from_str(&sidecar.uname) {
        Ok(uname) => uname,
        Err(e) => { 
            let estr = format!(
                "Error turning student uname {:?} into a header value: {}",
                &sidecar.uname, &e
            );
            log::error!("Error generating report markdown response: {}", &estr);
            return text_500(Some(estr));
        },
    };

    (
        StatusCode::OK,
        [
            (
                HeaderName::from_static("x-camp-action"),
                HeaderValue::from_static("edit-markdown"),
            ),
            (
                HeaderName::from_static("x-camp-student"),
                suname,
            ),
            (
                HeaderName::from_static("x-camp-term"),
                term,
            ),
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/markdown"),
            ),
        ],
        text
    )
        .into_response()
}

async fn generate_report(
    headers: &HeaderMap,
    body: Option<String>,
    glob: Arc<RwLock<Glob>>
) -> Response {
    let suname = match get_head("x-camp-student", headers) {
        Ok(uname) => uname,
        Err(e) => { return respond_bad_request(e); },
    };
    let tuname = match get_head("x-camp-uname", headers) {
        Ok(uname) => uname,
        Err(e) => { return text_500(Some(e)); },
    };
    let term = match get_head("x-camp-term", headers) {
        Ok(term) => term,
        Err(e) => { return respond_bad_request(e); },
    };
    let term = match Term::from_str(term) {
        Ok(term) => term,
        Err(e) => {
            log::warn!(
                "Invalid x-camp-term value ({:?}) in attempt to generate report for {:?}: {}",
                term, suname, &e
            );
            return respond_bad_request(format!(
                "Invalid x-camp-term value {:?}: {}", term, &e
            ));
        },
    };
    let body = match body {
        Some(body) => body,
        None => {
            return respond_bad_request(
                "Request needs application/json body with ReportSidecar details.".to_owned(),
            );
        }
    };

    let glob = glob.read().await;

    match glob.users.get(suname) {
        Some(User::Student(s)) => {
            if s.teacher != tuname {
                let estr = format!("The student {:?} is not yours.", &suname);
                return (StatusCode::FORBIDDEN, estr).into_response();
            }
        },
        _ => {
            let estr = format!(
                "The uname {:?} does not belong to a student in the system.",
                &suname
            );
            return respond_bad_request(estr);
        }
    }

    {
        let data_guard = glob.data();
        let data = data_guard.read().await;
        let mut client = match data.connect().await {
            Ok(client) => client,
            Err(e) => { return text_500(Some(e.to_string())); },
        };
        let t = match client.transaction().await {
            Ok(trans) => trans,
            Err(e) => { return text_500(Some(e.to_string())); },
        };
        if let Err(e) = Store::set_draft(&t, suname, term, &body).await {
            log::error!(
                "Error attempting to store {} report draft for {:?}: {}",
                &term, &suname, &e
            );
            return text_500(Some(format!(
                "Error attempting to store report draft in database: {}", &e
            )));
        }
        if let Err(e) = t.commit().await {
            log::error!(
                "Error committing transaction for storing {} report draft for {:?}: {}",
                &term, &suname, &e
            );
            return text_500(Some(format!(
                "Error committing report draft to database: {}", &e
            )));
        }
    }

    let pdf_data = match report::render_markdown(body, &glob).await {
        Ok(data) => data,
        Err(e) => {
            log::error!(
                "Error attempting to render {} report PDF for {:?}: {}",
                &term, suname, &e
            );
            return text_500(Some(format!(
                "Error generating PDF file: {}", &e
            )));
        },
    };

    {
        let data_guard = glob.data();
        let data = data_guard.read().await;
        let mut client = match data.connect().await {
            Ok(client) => client,
            Err(e) => { return text_500(Some(e.to_string())); },
        };
        let t = match client.transaction().await {
            Ok(trans) => trans,
            Err(e) => { return text_500(Some(e.to_string())); },
        };
        if let Err(e) = Store::set_final(&t, suname, term, &pdf_data).await {
            log::error!(
                "Error attempting to store final {} report PDF for {:?}: {}",
                &term, &suname, &e
            );
            return text_500(Some(format!(
                "Error attempting to store report PDF in database: {}", &e
            )));
        }
        if let Err(e) = t.commit().await {
            log::error!(
                "Error committing transaction for storing {} report PDF for {:?}: {}",
                &term, &suname, &e
            );
            return text_500(Some(format!(
                "Error committing final report PDF to database: {}", &e
            )));
        }
    }

    let uname = match HeaderValue::from_str(suname) {
        Ok(uname) => uname,
        Err(e) => {
            let estr = format!(
                "Error converting student uname {:?} into header value: {}",
                suname, &e
            );
            log::error!("{}", &estr);
            return text_500(Some(estr));
        },
    };

    (
        StatusCode::OK,
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_static("application/pdf"),
            ),
            (
                HeaderName::from_static("x-camp-action"),
                HeaderValue::from_static("display-pdf"),
            ),
            (
                HeaderName::from_static("x-camp-student"),
                uname,
            ),
            (
                HeaderName::from_static("x-camp-term"),
                headers.get("x-camp-term").unwrap().clone(),
            ),
        ],
        pdf_data
    ).into_response()
}

async fn discard_pdf(headers: &HeaderMap, glob: Arc<RwLock<Glob>>) -> Response {
    let suname = match get_head("x-camp-student", headers) {
        Ok(uname) => uname,
        Err(e) => { return respond_bad_request(e); },
    };
    let tuname = match get_head("x-camp-uname", headers) {
        Ok(uname) => uname,
        Err(e) => { return text_500(Some(e)); },
    };
    let term = match get_head("x-camp-term", headers) {
        Ok(uname) => uname,
        Err(e) => { return respond_bad_request(e); },
    };
    let term = match Term::from_str(term) {
        Ok(t) => t,
        Err(e) => {
            log::warn!(
                "Invalid x-camp-term value ({:?}) in attempt to generate report for {:?}: {}",
                term, suname, &e
            );
            return respond_bad_request(format!(
                "Invalid x-camp-term value {:?}: {}", term, &e
            ));
        },
    };

    let glob = glob.read().await;
    match glob.users.get(suname) {
        Some(User::Student(s)) => {
            if s.teacher != tuname {
                let estr = format!("The student {:?} is not yours.", &suname);
                return (StatusCode::FORBIDDEN, estr).into_response();
            }
        },
        _ => {
            let estr = format!(
                "The uname {:?} does not belong to a student in the system.",
                &suname
            );
            return respond_bad_request(estr);
        },
    }

    match glob.data().read().await.clear_final(suname, term).await {
        Ok(()) => { return respond_ok(); },
        Err(e) => {
            log::error!(
                "Error attempting to discard {} report PDF for {:?}: {}",
                &term, suname, &e
            );
            return text_500(Some(format!(
                "Error attempting to discard report PDF: {}", &e
            )));
        }
    }
}
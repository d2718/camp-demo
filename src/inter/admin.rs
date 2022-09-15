/*!
Subcrate for interoperation with Admin users.
*/
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::Arc;

use axum::{
    extract::Extension,
    http::header::{HeaderMap, HeaderName},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use time::Date;
use tokio::sync::RwLock;

use super::*;
use crate::config::Glob;
use crate::course::{Chapter, Course};
use crate::{auth::AuthResult, user::*, DATE_FMT};

/**
Determine whether the Admin's login credentials check out, then send the
initial HTML for the Admin view.

After receiving this initial load of information, the Admin frontend will
automatically send another couple of requests to populate additional
information.
*/
pub async fn login(base: BaseUser, form: LoginData, glob: Arc<RwLock<Glob>>) -> Response {
    log::trace!(
        "admin::login( {:?}, {:?}, [ global state ] ) called.",
        &base,
        &form
    );

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
                "Error: auth::Db::check_password_and_issue_key( {:?}, {:?}, [ Glob ]): {}",
                &base,
                &form,
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
                "auth::Db::check_password_and_issue_key( {:?}, {:?}, [ Glob ] ) returned {:?}, which shouldn't happen.",
                &base, &form, &x
            );
            return respond_bad_password(&base.uname);
        }
    };

    let data = json!({
        "uname": &base.uname,
        "key": &auth_key
    });

    serve_template(StatusCode::OK, "admin", &data, vec![])
}

/**
All requests from the Admin's frontend view get sent to the `/teacher` URI
and then funnelled through this endpoint.

This funciton will dispatch that request appropriately based on the value
of the `x-camp-action` header for executing updates and generating
responses.

A previous layer should have already ensured that the Admin's key
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

    match u {
        User::Admin(_) => { /* Okay, request may proceed. */ }
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
        "populate-users" => populate_users(glob.clone()).await,
        "populate-admins" => populate_role(glob.clone(), Role::Admin).await,
        "populate-bosses" => populate_role(glob.clone(), Role::Boss).await,
        "add-user" => add_user(body, glob.clone()).await,
        "update-user" => update_user(body, glob.clone()).await,
        "delete-user" => delete_user(body, glob.clone()).await,
        "upload-students" => upload_students(body, glob.clone()).await,
        "populate-courses" => populate_courses(glob.clone()).await,
        "upload-course" => upload_course(body, glob.clone()).await,
        "add-course" => add_course(body, glob.clone()).await,
        "delete-course" => delete_course(body, glob.clone()).await,
        "update-course" => update_course(body, glob.clone()).await,
        "add-chapters" => add_chapters(body, glob.clone()).await,
        "update-chapter" => update_chapter(body, glob.clone()).await,
        "delete-chapter" => delete_chapter(body, glob.clone()).await,
        "populate-cal" => populate_calendar(glob.clone()).await,
        "update-cal" => update_calendar(body, glob.clone()).await,
        "populate-dates" => populate_dates(glob).await,
        "set-date" => set_date(body, glob.clone()).await,
        "reset-students" => reset_students(glob.clone()).await,
        x => respond_bad_request(format!(
            "{:?} is not a recognizable x-camp-action value.",
            x
        )),
    }
}

/**
Generate a response for the frontend to populate data about all users of
the given [`Role`].
*/
async fn populate_role(glob: Arc<RwLock<Glob>>, role: Role) -> Response {
    log::trace!("populate_role( Glob, {:?} ) called.", &role);

    let glob = glob.read().await;
    let users: Vec<&User> = glob
        .users
        .iter()
        .map(|(_, u)| u)
        .filter(|&u| u.role() == role)
        .collect();

    (
        StatusCode::OK,
        [(
            HeaderName::from_static("x-camp-action"),
            HeaderValue::from_static("populate-users"),
        )],
        Json(users),
    )
        .into_response()
}

/**
Generate a response for the frontend to populate data about all users
of the system.

Request requirements:
```text
x-camp-action: populate-users
```
*/
async fn populate_users(glob: Arc<RwLock<Glob>>) -> Response {
    log::trace!("populate_all( Glob ) called.");

    let glob = glob.read().await;
    let mut users: Vec<&User> = glob.users.iter().map(|(_, u)| u).collect();
    users.sort_by(|a, b| a.partial_cmp(b).unwrap());

    (
        StatusCode::OK,
        [(
            HeaderName::from_static("x-camp-action"),
            HeaderValue::from_static("populate-users"),
        )],
        Json(users),
    )
        .into_response()
}

/**
Respond to a request to add a user to the database.

Request requirements:
```text
x-camp-action: add-user
```
With a body that should JSON-deserialize into the [`User`] data
in question.
*/
async fn add_user(body: Option<String>, glob: Arc<RwLock<Glob>>) -> Response {
    let body = match body {
        Some(body) => body,
        None => {
            return respond_bad_request("Request requires a JSON body.".to_owned());
        }
    };

    let u: User = match serde_json::from_str(&body) {
        Ok(u) => u,
        Err(e) => {
            log::error!("Error deserializing JSON {:?} as BaseUser: {}", &body, &e);
            return text_500(Some("Unable to deserialize User struct.".to_owned()));
        }
    };

    {
        let mut glob = glob.write().await;
        if let Err(e) = glob.insert_user(&u).await {
            log::error!("Error inserting new user ({:?})into database: {}", &u, &e,);
            return text_500(Some(format!("Unable to insert User into database: {}", &e)));
        }
        if let Err(e) = glob.refresh_users().await {
            log::error!("Error refreshing user hash from database: {}", &e);
            return text_500(Some("Unable to reread users from database.".to_owned()));
        }
    }

    //populate_role(glob, u.role()).await
    populate_users(glob).await
}

/**
Respond to a request to add multiple Students from data in CSV format.

Request requirements:
```text
x-camp-action: upload-students
```
The request body should be CSV data in the specified format
(see [`Student::vec_from_csv_reader`]).
*/
async fn upload_students(body: Option<String>, glob: Arc<RwLock<Glob>>) -> Response {
    let body = match body {
        Some(body) => body,
        None => {
            return respond_bad_request("Request requires a CSV body.".to_owned());
        }
    };

    {
        let glob = glob.read().await;
        if let Err(e) = glob.upload_students(&body).await {
            log::error!(
                "Error uploading new students via CSV: {}\n\nCSV text:\n\n{}\n",
                &e,
                &body
            );
            return text_500(Some(e.to_string()));
        }
    }
    {
        let mut glob = glob.write().await;
        if let Err(e) = glob.refresh_users().await {
            log::error!("Error refreshing user hash from database: {}", &e);
            return text_500(Some("Unable to reread users from database.".to_owned()));
        }
    }

    populate_users(glob).await
}

/**
Respond to a request to update a User's data.

Request requirements:
```text
x-camp-action: update-user
```
The request body should be a JSON-deserializable `User` struct with the
`uname` of the user whose data should be updated with the rest of the
data in the struct.

This action can't change the [`Role`] of a user.
*/
async fn update_user(body: Option<String>, glob: Arc<RwLock<Glob>>) -> Response {
    let body = match body {
        Some(body) => body,
        None => {
            return respond_bad_request("Request requires a JSON body.".to_owned());
        }
    };

    let u: User = match serde_json::from_str(&body) {
        Ok(u) => u,
        Err(e) => {
            log::error!("Error deserializing JSON {:?} as User: {}", &body, &e);
            return text_500(Some("Unable to deserialize User struct.".to_owned()));
        }
    };

    {
        let mut glob = glob.write().await;
        if let Err(e) = glob.update_user(&u).await {
            log::error!("Error updating user {:?}: {}", &u, &e,);
            return text_500(Some(e.to_string()));
        }
        if let Err(e) = glob.refresh_users().await {
            log::error!("Error refreshing user hash from database: {}", &e);
            return text_500(Some("Unable to reread users from database.".to_owned()));
        }
    }

    //populate_role(glob, u.role()).await
    populate_users(glob).await
}

/**
Respond to a request to delete a User form the database.

Req'ments:
```text
x-camp-action: delete-user
```
Body should be `uname` of user to be deleted.
*/
async fn delete_user(body: Option<String>, glob: Arc<RwLock<Glob>>) -> Response {
    let uname = match body {
        Some(uname) => uname,
        None => {
            return respond_bad_request(
                "Request must include the uname to delete as a body.".to_owned(),
            );
        }
    };

    {
        let glob = glob.read().await;
        if let Err(e) = glob.delete_user(&uname).await {
            log::error!("Error deleting user {:?}: {}", uname, &e);
            return text_500(Some(e.to_string()));
        }
    }
    {
        if let Err(e) = glob.write().await.refresh_users().await {
            log::error!("Error refreshing user hash from database: {}", &e);
            return text_500(Some("Unable to reread users from database.".to_owned()));
        }
    }

    populate_users(glob).await
}

//
//
// This section is for dealing with COURSES.
//
//

/**
Generate a response to send data about all extant courses to the frontend.

Multiple request handlers in this module (generally dealing with inserting
or altering `Course`s) use this function to generate their responses.
*/
async fn populate_courses(glob: Arc<RwLock<Glob>>) -> Response {
    let glob = glob.read().await;

    let mut courses: Vec<&Course> = glob.courses.iter().map(|(_, c)| c).collect();

    courses.sort_by(|a, b| {
        a.level
            .partial_cmp(&b.level)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    (
        StatusCode::OK,
        [(
            HeaderName::from_static("x-camp-action"),
            HeaderValue::from_static("populate-courses"),
        )],
        Json(courses),
    )
        .into_response()
}

/**
Reload all the [`Glob`]'s local copies of all [`Course`] (and thus also
[`Chapter`]) data from the database and resend it all to the frontend.

This function should be called at the end of any handler that makes
a change to course data in the database.
*/
async fn refresh_and_repopulate_courses(glob: Arc<RwLock<Glob>>) -> Response {
    {
        let mut glob = glob.write().await;
        if let Err(e) = glob.refresh_courses().await {
            log::error!("Error refreshing course hash from database: {}", &e);
            return text_500(Some(format!(
                "Unable to refresh course data from database: {}",
                &e
            )));
        }
    }

    populate_courses(glob).await
}

/**
Respond to a request to insert a course into the database from information
in hybrid TOML/CSV format.

Req'ments:
```text
x-camp-action: upload course
```
Request body should be data describing the `Course` and its `Chapter`s
as described in  the [`course`] submodule-level documentation.
*/
async fn upload_course(body: Option<String>, glob: Arc<RwLock<Glob>>) -> Response {
    let body = match body {
        Some(body) => body,
        None => {
            return respond_bad_request("Request requires textual body.".to_owned());
        }
    };

    let reader = Cursor::new(body);
    let crs = match Course::from_reader(reader) {
        Ok(crs) => crs,
        Err(e) => {
            return respond_bad_request(e);
        }
    };
    if let Err(e) = Glob::check_course_for_bad_chars(&crs) {
        return respond_bad_request(e);
    }

    {
        let glob = glob.read().await;

        let data = glob.data();
        match data.read().await.insert_courses(&[crs]).await {
            Ok((n_crs, n_ch)) => {
                log::trace!(
                    "Inserted {} Cours(es) and {} Chapter(s) into the Data DB.",
                    n_crs,
                    n_ch
                );
            }
            Err(e) => {
                return text_500(Some(e.into()));
            }
        };
    }

    refresh_and_repopulate_courses(glob).await
}

/**
Respond to a request to add a single course to the database.

In general, when coming from the frontend, this will be a new `Course` with
no chapters as of yet.

Req'ments:
```text
x-camp-action: add-course
```
Request body should be a JSON-deserializable `Course` struct with metadata
about the empty course to add.
*/
async fn add_course(body: Option<String>, glob: Arc<RwLock<Glob>>) -> Response {
    let body = match body {
        Some(body) => body,
        None => {
            return respond_bad_request(
                "Request requires application/json body describing the Course.".to_owned(),
            );
        }
    };

    let crs: Course = match serde_json::from_str(&body) {
        Ok(crs) => crs,
        Err(e) => {
            log::error!("Error deserializing JSON {:?} as Course: {}", &body, &e);
            return text_500(Some("Unable to deserialize to Course struct.".to_owned()));
        }
    };
    if let Err(e) = Glob::check_course_for_bad_chars(&crs) {
        return respond_bad_request(e);
    }

    {
        let glob = glob.read().await;
        let data = glob.data();
        match data.read().await.insert_courses(&[crs]).await {
            Ok((n_crs, n_ch)) => {
                log::trace!(
                    "Inserted {} Cours(es) and {} Chapter(s) into the Data DB.",
                    n_crs,
                    n_ch
                );
            }
            Err(e) => {
                return text_500(Some(e.into()));
            }
        };
    }

    refresh_and_repopulate_courses(glob).await
}

/**
Respond to a request to change a `Course`'s "metadata". (Has no effect on the
course's chapters.)

Req'ments:
```text
x-camp-action: update-course
```
Body should JSON-deserialize to a `Course` with the new metadata.
*/
async fn update_course(body: Option<String>, glob: Arc<RwLock<Glob>>) -> Response {
    let body = match body {
        Some(body) => body,
        None => {
            return respond_bad_request(
                "Request requires applicaiton/json body with Course details.".to_owned(),
            );
        }
    };

    let crs: Course = match serde_json::from_str(&body) {
        Ok(crs) => crs,
        Err(e) => {
            log::error!("Error deserializing JSON {:?} as Course: {}", &body, &e);
            return text_500(Some("Unable to deserialize to Course struct.".to_owned()));
        }
    };
    if let Err(e) = Glob::check_course_for_bad_chars(&crs) {
        return respond_bad_request(e);
    }

    {
        let glob = glob.read().await;
        let data = glob.data();
        if let Err(e) = data.read().await.update_course(&crs).await {
            return text_500(Some(format!("Unable to update Course: {}", &e)));
        };
    }

    refresh_and_repopulate_courses(glob).await
}

/**
Respond to a request to delete a `Course` (and all its constituent `Chapter`s).

Will fail if there are currently any assigned `Goal`s of that `Chapter`.

Req's:
```text
x-camp-action: delete-course
```
Body should be the `sym` of the `Course` in question.
*/
async fn delete_course(body: Option<String>, glob: Arc<RwLock<Glob>>) -> Response {
    let body = match body {
        Some(body) => body,
        None => {
            return respond_bad_request("Request requires sym of Course in body.".to_owned());
        }
    };

    {
        match glob.read().await.delete_course(&body).await {
            Ok((n_crs, n_ch)) => {
                log::trace!("Deleted {} Course, {} Chapters from Data DB.", n_crs, n_ch);
            }
            Err(e) => {
                return text_500(Some(e.to_string()));
            }
        };
    }

    refresh_and_repopulate_courses(glob).await
}

/**
Respond to a request to simultaneously add multiple `Chapter`s to a `Course`.

These will generally come in with only meaningful `ch.course_id` and `ch.seq`
values set, the rest to be filled-in with defaults (and _maybe_ improved
later.)

Req'ments:
```text
x-camp-action: add-chapters
```
The body should JSON-decode to a `Vec` of the relevant `Chapter` data.
*/
async fn add_chapters(body: Option<String>, glob: Arc<RwLock<Glob>>) -> Response {
    let body = match body {
        Some(body) => body,
        None => {
            return respond_bad_request(
                "Request requires application/json body with new Chapter info.".to_owned(),
            );
        }
    };

    let chapters: Vec<Chapter> = match serde_json::from_str(&body) {
        Ok(ch) => ch,
        Err(e) => {
            log::error!("Error deserializing JSON {:?} as Chapter: {}", &body, &e);
            return text_500(Some(
                "Unable to deserialize to vector of Chapters.".to_owned(),
            ));
        }
    };

    for ch in chapters.iter() {
        if let Err(e) = Glob::check_chapter_for_bad_chars(ch) {
            return respond_bad_request(e);
        }
    }

    {
        let glob = glob.read().await;
        let data = glob.data();
        if let Err(e) = data.read().await.insert_chapters(&chapters).await {
            return text_500(Some(format!("Unable to insert Chapter: {}", &e)));
        };
    }

    refresh_and_repopulate_courses(glob).await
}

/**
Respond to a request to delete a specific chapter.

Will fail if any students are assigned a `Goal` of that `Chapter`.

Req'ments:
```text
x-camp-action; delete-chapter
```
Body should be `id` of the chapter in question.
*/
async fn delete_chapter(body: Option<String>, glob: Arc<RwLock<Glob>>) -> Response {
    let body = match body {
        Some(body) => body,
        None => {
            return respond_bad_request("Request requires id of Chapter in body.".to_owned());
        }
    };

    let ch_id: i64 = match body.parse() {
        Ok(n) => n,
        Err(e) => {
            return respond_bad_request(format!(
                "Unable to parse body of request {:?} as Chapter id: {}",
                &body, &e
            ));
        }
    };

    if let Err(e) = glob.read().await.delete_chapter(ch_id).await {
        return text_500(Some(format!("Unable to delete Chapter: {}", &e)));
    };

    refresh_and_repopulate_courses(glob).await
}

/**
Respond to a request to update the information about a `Chapter`.

Req'ments:
```text
x-camp-action: update-chapter
```
Body should be JSON-deserializable `Chapter` struct with the `id` of the
`Chapter` that should be updated, with the rest of the values being the
new data about the `Chapter.
*/
async fn update_chapter(body: Option<String>, glob: Arc<RwLock<Glob>>) -> Response {
    let body = match body {
        Some(body) => body,
        None => {
            return respond_bad_request(
                "Request requires application/json body with Chapter details.".to_owned(),
            );
        }
    };

    let ch: Chapter = match serde_json::from_str(&body) {
        Ok(ch) => ch,
        Err(e) => {
            log::error!("Error deserializing JSON {:?} as Chapter: {}", &body, &e);
            return text_500(Some("Unable to deserialize to Chapter struct.".to_owned()));
        }
    };

    if let Err(e) = Glob::check_chapter_for_bad_chars(&ch) {
        return respond_bad_request(e);
    }

    {
        let glob = glob.read().await;
        let data = glob.data();
        if let Err(e) = data.read().await.update_chapter(&ch).await {
            return text_500(Some(format!("Unable to update Chapter: {}", &e)));
        };
    }

    refresh_and_repopulate_courses(glob).await
}

//
//
// This section is for dealing with the CALENDAR.
//
//

/**
Generate a `Response` for sending all "calendar" data—that is, the list of
"working days" in the current academic year.

Req'ment:
```text
x-camp-action: populate-cal
```
*/
async fn populate_calendar(glob: Arc<RwLock<Glob>>) -> Response {
    let date_strs: Vec<String> = glob
        .read()
        .await
        .calendar
        .iter()
        .map(|d| format!("{}", d))
        .collect();

    (
        StatusCode::OK,
        [(
            HeaderName::from_static("x-camp-action"),
            HeaderValue::from_static("populate-cal"),
        )],
        Json(date_strs),
    )
        .into_response()
}

/**
Reload the local copy of the list of calendar days from the backing database
and send that data to the frontend.

This should be called by any handler that makes changes to the calendar.
*/
async fn refresh_and_repopulate_calendar(glob: Arc<RwLock<Glob>>) -> Response {
    {
        let mut glob = glob.write().await;
        if let Err(e) = glob.refresh_calendar().await {
            log::error!("Error refreshing calendar Vec from database: {}", &e);
            return text_500(Some(format!(
                "Unable to refresh calendar data from database: {}",
                &e
            )));
        }
    }

    populate_calendar(glob).await
}

/**
Respond to a request to set the list of working days for the current academic
year.

Req'ments:
```text
x-camp-action: update-cal
```
Body should JSON-deserialize to a vector of `&str`s that should be parseable
as dates ("2021-01-27" format).
*/
async fn update_calendar(body: Option<String>, glob: Arc<RwLock<Glob>>) -> Response {
    let body: String = match body {
        Some(body) => body,
        None => {
            return respond_bad_request(
                "Request requires application/json body with Array of date strings.".to_owned(),
            );
        }
    };

    let date_strs: Vec<&str> = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            log::error!(
                "Error deserializing JSON {:?} as Vector of &str: {}",
                &body,
                &e
            );
            return text_500(Some("Unable to deserialize to Vector of &str.".to_owned()));
        }
    };

    let mut dates: Vec<Date> = Vec::with_capacity(date_strs.len());
    for s in date_strs.iter() {
        match Date::parse(s, DATE_FMT) {
            Ok(d) => {
                dates.push(d);
            }
            Err(e) => {
                log::error!("Error parsing {:?} as Date: {}", s, &e);
                return text_500(Some(format!("Unable to parse {:?} as Date.", s)));
            }
        }
    }

    {
        let glob = glob.read().await;
        let data = glob.data();
        let reader = data.read().await;
        if let Err(e) = reader.set_calendar(&dates).await {
            return text_500(Some(format!("Unable to update calendar: {}", &e)));
        }
    }

    refresh_and_repopulate_calendar(glob).await
}

/**
Generate a `Response` to send all "special dates" to the frontend.

This should be called by any handler that changes dates. It can also be
invoked directly by:
```text
x-camp-action: populate-dates
```
*/
async fn populate_dates(glob: Arc<RwLock<Glob>>) -> Response {
    let date_map: HashMap<String, String> = glob
        .read()
        .await
        .dates
        .iter()
        .map(|(name, date)| (name.clone(), format!("{}", date)))
        .collect();

    (
        StatusCode::OK,
        [(
            HeaderName::from_static("x-camp-action"),
            HeaderValue::from_static("populate-dates"),
        )],
        Json(date_map),
    )
        .into_response()
}

/**
Respond to a request to add/update a "special date".

Req'ments:
```text
x-camp-action: set-date
```
Body should deserialize into a `(date-name, date-string)` tuple.

Ex:
```text
("end-fall", "2023-01-12")
```
*/
async fn set_date(body: Option<String>, glob: Arc<RwLock<Glob>>) -> Response {
    let body = match body {
        Some(body) => body,
        None => {
            return respond_bad_request(
                "Request requires a body with tuple of (name, date) strings.".to_owned(),
            );
        }
    };

    let (name, date_str): (&str, &str) = match serde_json::from_str(&body) {
        Ok((n, d)) => (n, d),
        Err(_) => {
            return text_500(Some("Unable to deserialize name and date data".to_owned()));
        }
    };

    if date_str.trim() == "" {
        let mut glob = glob.write().await;
        {
            let data = glob.data();
            if let Err(e) = data.read().await.delete_date(name).await {
                log::error!("Error deleting date {:?} from database: {}", name, &e);
                return text_500(Some("Error deleting date from database.".to_owned()));
            }

            if let Err(e) = glob.refresh_dates().await {
                log::error!("Error calling Glob::refresh_dates(): {}", &e);
                return text_500(Some("Error retrieving new dates from database.".to_owned()));
            }
        }
    } else {
        let date = match Date::parse(date_str, DATE_FMT) {
            Ok(d) => d,
            Err(_) => {
                return text_500(Some(format!("Error parsing {:?} as date.", date_str)));
            }
        };

        let mut glob = glob.write().await;
        {
            let data = glob.data();
            if let Err(e) = data.read().await.set_date(name, &date).await {
                log::error!(
                    "Error inserting date {:?}: {} into database: {}",
                    name,
                    &date,
                    &e
                );
                return text_500(Some("Error inserting date into database.".to_owned()));
            };
        }
        if let Err(e) = glob.refresh_dates().await {
            log::error!("Error calling Glob::refresh_dates(): {}", &e);
            return text_500(Some("Error retrieving new dates from database.".to_owned()));
        }
    }

    populate_dates(glob).await
}

/**
Respond to a request to delete all student data (all data from the `students`
table in the database, along with all associated entries in the `users` table,
as well as all goals.)

Use sparingly.

```text
x-camp-action: reset-students
```
*/
async fn reset_students(glob: Arc<RwLock<Glob>>) -> Response {
    {
        let mut glob = glob.write().await;

        let res = glob.yearly_data_nuke().await;

        if let Err(e) = glob.refresh_users().await {
            let mut estr = format!(
                "There was an error refreshing User data from the database: {}",
                &e
            );
            if let Err(e) = res {
                estr = format!("{}\n{}", &estr, &e);
            }

            return text_500(Some(estr));
        }
    }

    populate_users(glob).await
}

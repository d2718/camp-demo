/*!
Interoperation between the client (user) and server.

(Not the application and the database; that's covered by `auth` and `store`.)
*/
use std::{fmt::Debug, io::Write, path::Path, sync::Arc};

use axum::{
    http::header::{HeaderMap, HeaderName, HeaderValue},
    http::{Request, StatusCode},
    middleware::Next,
    response::{Html, IntoResponse, Response},
    Extension,
};
use handlebars::Handlebars;
use once_cell::sync::OnceCell;
use serde::Serialize;
use serde_json::json;
use tokio::sync::RwLock;

use crate::{auth::AuthResult, config::Glob, user::User, MiniString, MEDSTORE};

pub mod admin;
pub mod boss;
pub mod student;
pub mod teacher;

/// [`Handlebars`] struct for rendering HTML-escaped text.
static TEMPLATES: OnceCell<Handlebars> = OnceCell::new();
/// [`Handlebars`] struct for rendering JSON-escaped text.
static JSON_TEMPLATES: OnceCell<Handlebars> = OnceCell::new();
/// [`Handlebars`] struct for rendering unescaped text.
static RAW_TEMPLATES: OnceCell<Handlebars> = OnceCell::new();

/// Text to be sent on an INTERNAL SERVER ERROR when responding to a request
/// that expects HTML.
static HTML_500: &str = r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<title>camp | Error</title>
<link rel="stylesheet" href="/static/camp.css">
</head>
<body>
<h1>Internal Server Error</h1>
<p>(Error 500)</p>
<p>Something went wrong on our end. No further or more
helpful information is available about the problem.</p>
</body>
</html>"#;

/// Default text to be sent on an INTERNAL SERVER ERROR when responding to a
/// request that expects plain text.
static TEXT_500: &str = "An internal error occurred; an appropriate response was inconstructable.";

/**
A trait that's about to get piled on top of `IntoResponse` to facilitate
conveniently adding headers.
*/
trait AddHeaders: IntoResponse + Sized {
    fn add_headers(self, mut new_headers: Vec<(HeaderName, HeaderValue)>) -> Response {
        let mut r = self.into_response();
        let r_headers = r.headers_mut();
        for (name, value) in new_headers.drain(..) {
            r_headers.insert(name, value);
        }

        r
    }
}

/// How convenient.
impl<T: IntoResponse + Sized> AddHeaders for T {}

/// Data type to read the form data from a front-page login request.
#[derive(serde::Deserialize, Debug)]
pub struct LoginData {
    pub uname: String,
    pub password: String,
}

/// Escape function to be used by [`handlebars`] for escaping JSON data.
fn escape_json(s: &str) -> String {
    let mut output = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => output.push_str(r#"\\"#),
            '"' => output.push_str(r#"\""#),
            '\n' => output.push_str(r#"\n"#),
            '\r' => { /* ignore this character */ }
            _ => output.push(c),
        }
    }
    output
}

/**
Initializes the resources used in this module. This function should be called
before any functionality of this module or any of its submodules is used.

Currently the only thing that happens here is loading the templates used by
`serve_template()`, which will panic unless `init()` has been called first.

The argument is the path to the directory where the templates used by
`serve_template()` can be found.
*/
pub fn init<P: AsRef<Path>>(template_dir: P) -> Result<(), String> {
    if TEMPLATES.get().is_some() {
        log::warn!("Templates directory already initialized; ignoring.");
        return Ok(());
    }

    let template_dir = template_dir.as_ref();

    let mut h = Handlebars::new();
    #[cfg(debug_assertions)]
    h.set_dev_mode(true);
    h.register_templates_directory(".html", template_dir)
        .map_err(|e| {
            format!(
                "Error registering templates directory {}: {}",
                template_dir.display(),
                &e
            )
        })?;

    TEMPLATES.set(h).map_err(|old_h| {
        let mut estr = String::from("Templates directory already registered w/templates:");
        for template_name in old_h.get_templates().keys() {
            estr.push('\n');
            estr.push_str(template_name.as_str());
        }
        estr
    })?;

    let mut j = Handlebars::new();
    #[cfg(debug_assertions)]
    j.set_dev_mode(true);
    j.register_templates_directory(".json", template_dir)
        .map_err(|e| {
            format!(
                "Error registering templates directory {}: {}",
                template_dir.display(),
                &e
            )
        })?;
    j.register_escape_fn(escape_json);

    JSON_TEMPLATES.set(j).map_err(|old_j| {
        let mut estr = String::from("Templates directory already registered w/templates:");
        for template_name in old_j.get_templates().keys() {
            estr.push('\n');
            estr.push_str(template_name.as_str());
        }
        estr
    })?;

    let mut r = Handlebars::new();
    #[cfg(debug_assertions)]
    r.set_dev_mode(true);
    r.register_templates_directory(".html", template_dir)
        .map_err(|e| {
            format!(
                "Error registering templates directory {} for .html templates: {}",
                template_dir.display(),
                &e
            )
        })?;
    r.register_templates_directory(".txt", template_dir)
        .map_err(|e| {
            format!(
                "Error registering templates directory {} for .txt templates: {}",
                template_dir.display(),
                &e
            )
        })?;
    r.register_escape_fn(handlebars::no_escape);

    RAW_TEMPLATES.set(r).map_err(|old_h| {
        let mut estr = String::from("Templates directory already registered w/templates:");
        for template_name in old_h.get_templates().keys() {
            estr.push('\n');
            estr.push_str(template_name.as_str());
        }
        estr
    })?;

    Ok(())
}

/**
Return an HTML response in the case of an unrecoverable* error.

(*"Unrecoverable" from the perspective of fielding the current request,
not from the perspective of the program crashing.)
*/
pub fn html_500() -> Response {
    (StatusCode::INTERNAL_SERVER_ERROR, Html(HTML_500)).into_response()
}

pub fn text_500(text: Option<String>) -> Response {
    match text {
        Some(text) => (StatusCode::INTERNAL_SERVER_ERROR, text).into_response(),
        None => (StatusCode::INTERNAL_SERVER_ERROR, TEXT_500.to_owned()).into_response(),
    }
}

/// Render a template with HTML-escaping.
pub fn render_template<T: Serialize>(name: &str, data: &T) -> Result<String, String> {
    TEMPLATES
        .get()
        .unwrap()
        .render(name, data)
        .map_err(|e| format!("Error rendering template {:?}: {}", name, &e))
}

/// Render a template with HTML-escaping to a [`Write`]r.
pub fn write_template<T: Serialize, W: Write>(
    name: &str,
    data: &T,
    writer: W,
) -> Result<(), String> {
    TEMPLATES
        .get()
        .unwrap()
        .render_to_write(name, data, writer)
        .map_err(|e| format!("Error rendering template {:?}: {}", name, &e))
}

/// Render a template with no escaping.
pub fn render_raw_template<T: Serialize>(name: &str, data: &T) -> Result<String, String> {
    RAW_TEMPLATES
        .get()
        .unwrap()
        .render(name, data)
        .map_err(|e| format!("Error rendering raw template {:?}: {}", name, &e))
}

/// Render a template with no escaping to a [`Write`]r.
pub fn write_raw_template<T: Serialize, W: Write>(
    name: &str,
    data: &T,
    writer: W,
) -> Result<(), String> {
    RAW_TEMPLATES
        .get()
        .unwrap()
        .render_to_write(name, data, writer)
        .map_err(|e| format!("Error rendering template {:?}: {}", name, &e))
}

/// Render a template with JSON-escaping.
pub fn render_json_template<T: Serialize>(name: &str, data: &T) -> Result<String, String> {
    JSON_TEMPLATES
        .get()
        .unwrap()
        .render(name, data)
        .map_err(|e| format!("Error rendering template: {:?}: {}", name, &e))
}

/// Render a template with JSON-escaping to a [`Write`]r.
pub fn write_json_template<T: Serialize, W: Write>(
    name: &str,
    data: &T,
    writer: W,
) -> Result<(), String> {
    JSON_TEMPLATES
        .get()
        .unwrap()
        .render_to_write(name, data, writer)
        .map_err(|e| format!("Error rendering template {:?}: {}", name, &e))
}

/// Generate a `Response` by rendering an HTML-escaped template.
pub fn serve_template<S>(
    code: StatusCode,
    template_name: &str,
    data: &S,
    addl_headers: Vec<(HeaderName, HeaderValue)>,
) -> Response
where
    S: Serialize + Debug,
{
    log::trace!(
        "serve_template( {}, {:?}, ... ) called.",
        &code,
        template_name
    );

    match TEMPLATES.get().unwrap().render(template_name, data) {
        Ok(response_body) => (code, Html(response_body)).add_headers(addl_headers),
        Err(e) => {
            log::error!(
                "Error rendering template {:?} with data {:?}:\n{}",
                template_name,
                data,
                &e
            );
            html_500()
        }
    }
}

/// Generate a `Response` by rendering an unescaped template.
pub fn serve_raw_template<S>(
    code: StatusCode,
    template_name: &str,
    data: &S,
    addl_headers: Vec<(HeaderName, HeaderValue)>,
) -> Response
where
    S: Serialize + Debug,
{
    log::trace!(
        "serve_raw_template( {}, {:?}, ... ) called.",
        &code,
        template_name
    );

    match RAW_TEMPLATES.get().unwrap().render(template_name, data) {
        Ok(response_body) => (code, Html(response_body)).add_headers(addl_headers),
        Err(e) => {
            log::error!(
                "Error rendering template {:?} with data {:?}:\n{}",
                template_name,
                data,
                &e
            );
            html_500()
        }
    }
}

/// Generate a respons by serving a static HTML file.
pub fn serve_static<P: AsRef<std::path::Path>>(
    code: StatusCode,
    path: P,
    addl_headers: Vec<(HeaderName, HeaderValue)>,
) -> Response {
    let path = path.as_ref();
    log::trace!(
        "serve_static( {:?}, {}, [ {} add'l headers ] ) called.",
        &code,
        path.display(),
        addl_headers.len()
    );

    let body = match std::fs::read_to_string(path) {
        Ok(body) => body,
        Err(e) => {
            log::error!("Error attempting to serve file {}: {}", path.display(), &e);
            return html_500();
        }
    };

    (code, Html(body)).add_headers(addl_headers)
}

/// Convenience function for generating a response to a login error.
pub fn respond_login_error(code: StatusCode, msg: &str) -> Response {
    log::trace!("respond_login_error( {:?} ) called.", msg);

    let data = json!({ "error_message": msg });

    serve_template(code, "login_error", &data, vec![])
}

pub fn respond_bad_password(uname: &str) -> Response {
    log::trace!("respond_bad_password( {:?} ) called.", uname);

    let data = json!({
        "error_message": "Invalid username/password combination.",
        "uname": uname,
    });

    serve_template(StatusCode::UNAUTHORIZED, "bad_password", &data, vec![])
}

/// Convenience function for generating a response to a key authentication
/// failure.
pub fn respond_bad_key() -> Response {
    log::trace!("respond_bad_key() called.");

    (
        StatusCode::UNAUTHORIZED,
        "Invalid authorization key.".to_owned(),
    )
        .into_response()
}

/// Convenience function for generating a 400 response.
pub fn respond_bad_request(msg: String) -> Response {
    log::trace!("respond_bad_request( {:?} ) called.", &msg);

    (StatusCode::BAD_REQUEST, msg).into_response()
}

/// Middleware function to ensure `x-camp-request-id` header is
/// maintained between request and response.
pub async fn request_identity<B>(req: Request<B>, next: Next<B>) -> Response {
    let id_header = match req.headers().get("x-camp-request-id") {
        Some(id) => id.to_owned(),
        None => {
            return respond_bad_request(
                "Request must have an x-camp-request-id header.".to_owned(),
            );
        }
    };

    let mut response = next.run(req).await;
    response
        .headers_mut()
        .insert("x-camp-request-id", id_header);
    response
}

/**
Middleware function to ensure key authentications for request layers
that require it.

Username should be sent as `x-camp-uname` header; key should be in the
`x-camp-key` header.
*/
pub async fn key_authenticate<B>(req: Request<B>, next: Next<B>) -> Response {
    let glob: &Arc<RwLock<Glob>> = req.extensions().get().unwrap();

    let key = match req.headers().get("x-camp-key") {
        Some(k_val) => match k_val.to_str() {
            Ok(s) => s,
            Err(e) => {
                log::error!(
                    "Failed converting auth key value {:?} to &str: {}",
                    k_val,
                    &e
                );
                return respond_bad_request("x-camp-key value unrecognizable.".to_owned());
            }
        },
        None => {
            return respond_bad_request("Request must have an x-camp-key header.".to_owned());
        }
    };

    let uname = match req.headers().get("x-camp-uname") {
        Some(u_val) => match u_val.to_str() {
            Ok(s) => s,
            Err(e) => {
                log::error!("Failed converting uname value {:?} to &str: {}", u_val, &e);
                return respond_bad_request("x-camp-uname value unrecognizable.".to_owned());
            }
        },
        None => {
            return respond_bad_request("Request must have an x-camp-uname header.".to_owned());
        }
    };

    // Lololol the chain here.
    //
    // But seriously, we return the result, then match on the returned value,
    // instead of just matching on the huge-ass chain expression so that
    // the locks will release.
    let res = glob
        .read()
        .await
        .auth()
        .read()
        .await
        .check_key(uname, key)
        .await;

    match res {
        Err(e) => {
            log::error!(
                "auth::Db::check_key( {:?}, {:?} ) returned error: {}",
                uname,
                key,
                &e
            );

            return text_500(None);
        }
        Ok(AuthResult::InvalidKey) => {
            return (
                StatusCode::UNAUTHORIZED,
                "Invalid authorization key.".to_owned(),
            )
                .into_response();
        }
        Ok(AuthResult::Ok) => {
            // This is the good path. We will just fall through and call the
            // next layer after the match.
        }
        Ok(x) => {
            log::warn!(
                "auth::Db::check_key() returned {:?}, which should never happen.",
                &x
            );
            return text_500(None);
        }
    }

    next.run(req).await
}

/**
Make an HTTP request to the [Sendgrid](https://sendgrid.com/) service to send
an email.

`json_body` should be a valid Sendgrid
[Mail Send v3 request body](https://docs.sendgrid.com/api-reference/mail-send/mail-send),
and the [`Glob`] should have your appropriate Sendgrid credentials.

The `student` parameter is only for generating nice(r) error messages.
*/
pub async fn make_sendgrid_request(
    json_body: String,
    glob: &Glob,
    student: MiniString<MEDSTORE>,
) -> Result<(), String> {
    use hyper::{Body, Client, Method, Uri};

    log::trace!(
        "make_sendgrid_request( [ {} bytes of body ] ) called.",
        json_body.len()
    );
    log::debug!("Sendgrid request body:\n{}", &json_body);

    let target_uri: Uri = "https://api.sendgrid.com/v3/mail/send"
        .parse()
        .map_err(|e| format!("Error parsing target URI: {}", &e))?;
    let https = hyper_rustls::HttpsConnectorBuilder::new()
        .with_native_roots()
        .https_only()
        .enable_http1()
        .build();
    let client: Client<_, hyper::Body> = Client::builder().build(https);

    let req = Request::builder()
        .method(Method::POST)
        .uri(target_uri)
        .header("Authorization", &glob.sendgrid_auth)
        .header("Content-Type", "application/json")
        .body(Body::from(json_body))
        .map_err(|e| format!("Error building sendgrid request: {}", &e))?;

    let resp = client
        .request(req)
        .await
        .map_err(|e| format!("Error from sendgrid request: {}", &e))
        .map_err(|e| format!("Error sending sendgrid request: {}", &e))?;

    if resp.status() == 202 {
        Ok(())
    } else {
        Err(format!(
            "Sendgrid returned {} response (expected 202) while sending email about {}.",
            &resp.status(),
            &student
        ))
    }
}

/// Generate (and send) a password reset email for the supplied [`User`].
///
/// This includes generating and registering a key to use in the password
/// reset process.
pub async fn generate_email(u: &User, glob: &Glob) -> Response {
    let key = match glob.auth().read().await.issue_key(u.uname()).await {
        Err(e) => {
            log::error!("auth::Db::issue_key( {:?} ) returned {:?}", u.uname(), &e);
            return text_500(None);
        }
        Ok(AuthResult::Key(k)) => k,
        Ok(x) => {
            log::warn!(
                "auth::Db::issue_key( {:?} ) returned {:?}, which shouldn't happen.",
                u.uname(),
                &x
            );
            return text_500(None);
        }
    };

    let data = match u {
        User::Student(ref s) => json!({
            "name": format!("{} {}", &s.rest, &s.last),
            "uname":  u.uname(),
            "email": u.email(),
            "parent": &s.parent,
            "key": &key,
        }),
        User::Teacher(ref t) => json!({
            "name": &t.name,
            "uname": u.uname(),
            "email": u.email(),
            "key": &key,
        }),
        User::Admin(_) | User::Boss(_) => json!({
            "name": u.uname(),
            "uname": u.uname(),
            "email": u.email(),
            "key": &key,
        }),
    };

    let render_res = match u {
        User::Student(_) => render_json_template("student_password_email", &data),
        _ => render_json_template("password_email", &data),
    };

    let body = match render_res {
        Err(e) => {
            log::error!("Error rendering email template for {:?}: {}", u, &e);
            return text_500(Some("Error generating email.".to_owned()));
        }
        Ok(body) => body,
    };

    let name: MiniString<MEDSTORE> = MiniString::from(u.uname());

    match make_sendgrid_request(body, glob, name).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => {
            log::error!("Error with Sendgrid request: {}", &e);
            text_500(Some("Error generating email.".to_owned()))
        }
    }
}

/**
Respond to a request to update a [`User`]'s password.

This request should have the following headers:
```
x-camp-action: reset-password (or it won't get here)
x-camp-uname: [ user's user name ]
x-camp-key: [ auth key sent in password reset email]
x-camp-password: [ the new requested password ]
```
*/
pub async fn update_password(u: &User, headers: &HeaderMap, glob: &Glob) -> Response {
    let key = match headers.get("x-camp-key") {
        Some(k_val) => match k_val.to_str() {
            Ok(s) => s,
            Err(e) => {
                log::error!(
                    "Failed converting x-camp-key header value {:?} to &str: {}",
                    k_val,
                    &e
                );
                return text_500(None);
            }
        },
        None => {
            return respond_bad_request("Request must have an x-camp-key header.".to_owned());
        }
    };

    let new_pwd = match headers.get("x-camp-password") {
        Some(p_val) => match p_val.to_str() {
            Ok(s) => s,
            Err(e) => {
                log::error!(
                    "Failed converting x-camp-password header value {:?} to &str: {}",
                    p_val,
                    &e
                );
                return text_500(None);
            }
        },
        None => {
            return respond_bad_request("Request must have an x-camp-password header.".to_owned());
        }
    };

    let auth = glob.auth();
    let auth_handle = auth.read().await;

    match auth_handle.check_key(u.uname(), key).await {
        Err(e) => {
            log::error!(
                "auth::Db::check_key( {:?}, {:?} ) error: {}",
                u.uname(),
                key,
                &e
            );
            return text_500(None);
        }
        Ok(AuthResult::InvalidKey) => {
            return respond_bad_key();
        }
        Ok(AuthResult::Ok) => { /* This is the happy path; proceed. */ }
        Ok(x) => {
            log::warn!(
                "auth::Db::check_key( {:?}. {:?} ) returned {:?}, which shouldn't happen.",
                u.uname(),
                key,
                &x
            );
            return text_500(None);
        }
    }

    match auth_handle.set_password(u.uname(), new_pwd, u.salt()).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => {
            log::error!(
                "auth::Db::set_password( {:?}, {:?}, {:?} ) error: {}",
                u.uname(),
                new_pwd,
                u.salt(),
                &e
            );
            text_500(None)
        }
    }
}

/// API endpoint for HTTP requests sent to "/pwd", which have to do with
/// requesting and executing password resets.
pub async fn password_reset(
    headers: HeaderMap,
    Extension(glob): Extension<Arc<RwLock<Glob>>>,
) -> Response {
    let uname = match headers.get("x-camp-uname") {
        Some(u_val) => match u_val.to_str() {
            Ok(s) => s,
            Err(e) => {
                log::error!(
                    "Failed converting x-camp-uname header value {:?} to &str: {}",
                    u_val,
                    &e
                );
                return text_500(None);
            }
        },
        None => {
            return respond_bad_request("Request must have an x-camp-uname header.".to_owned());
        }
    };

    let action = match headers.get("x-camp-action") {
        Some(a_val) => match a_val.to_str() {
            Ok(s) => s,
            Err(e) => {
                log::error!(
                    "Failed converting x-camp-action header value {:?} to &str: {}",
                    a_val,
                    &e
                );
                return text_500(None);
            }
        },
        None => {
            return respond_bad_request("Request must have an x-camp-action header.".to_owned());
        }
    };

    let glob = glob.read().await;
    let u = match glob.users.get(uname) {
        Some(u) => u,
        None => {
            return StatusCode::OK.into_response();
        }
    };

    match action {
        "request-email" => generate_email(u, &glob).await,
        "reset-password" => update_password(u, &headers, &glob).await,
        x => respond_bad_request(format!(
            "Unrecognized or invalid x-camp-action value: {:?}",
            &x
        )),
    }
}

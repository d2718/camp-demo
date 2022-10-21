/*!
Separate server process for rendering markdown into PDFs.

This should run in a pandoc/latex Docker image.
*/

use std::net::SocketAddr;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Bytes,
    extract::BodyStream,
    http::header::{HeaderMap, HeaderName, HeaderValue},
    http::status::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Extension, Router,
};
use futures::{Stream, StreamExt};
use serde::Deserialize;
use tokio::{io::AsyncWriteExt, process::Command};

static CFG_FILE: &str = "pandocker.toml";
static DEFAULT_AUTH: &str = "1010101-frogsfrogsfrogs";
static AUTH_FAIL_WAIT: Duration = Duration::from_millis(3000);

#[derive(Deserialize)]
struct Cfg {
    port: Option<u16>,
    auth: Option<String>,
}

/**
Make a call to pandoc to convert between document types.

This is intended chiefly to convert "Github-flavored Markdown" to PDF
(and the defaults reflect that), but it can be used to make any kind
of conversion pandoc will make.
*/
async fn render(
    source: Vec<u8>,
    from_fmt: Option<&str>,
    to_fmt: Option<&str>,
) -> Result<Vec<u8>, String> {
    let from_opt = match from_fmt {
        Some(fmt) => fmt,
        None => "gfm",
    };
    let to_opt = match to_fmt {
        Some(fmt) => fmt,
        None => "pdf",
    };

    let mut child = Command::new("pandoc")
        .args(["-f", from_opt, "-t", to_opt])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Error spawning pandoc process: {}", &e))?;

    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or("Unable to get a handle on subprocess's stdin.".to_owned())?;
        stdin
            .write(&source)
            .await
            .map_err(|e| format!("Error writing to stdin of pandoc process: {}", &e))?;

        // We have intentionally scoped this block so that the subprocess's
        // stdin handle drops here, signalling EOF to the pandoc subprocess.
    }

    let output = child
        .wait_with_output()
        .await
        .map_err(|e| format!("Error getting output from pandoc process: {}", &e))?;

    if output.status.success() {
        Ok(output.stdout)
    } else {
        let err_output = String::from_utf8_lossy(&output.stderr).to_string();
        Err(err_output)
    }
}

fn authenticate(headers: &HeaderMap, auth: &str) -> bool {
    if let Some(val) = headers.get("authorization") {
        if let Ok(val) = val.to_str() {
            return val == auth;
        }
    }
    false
}

async fn handle(
    headers: HeaderMap,
    body: Option<BodyStream>,
    Extension(auth): Extension<Arc<String>>,
) -> Response {
    if !authenticate(&headers, &auth.as_str()) {
        tokio::time::sleep(AUTH_FAIL_WAIT).await;
        return (
            StatusCode::UNAUTHORIZED,
            "Invalid \"authorization:\" header value.".to_owned(),
        )
            .into_response();
    }

    let mut from_fmt: Option<&str> = None;
    let mut to_fmt: Option<&str> = None;

    if let Some(val) = headers.get("x-camp-from") {
        if let Ok(val) = val.to_str() {
            from_fmt = Some(val);
        }
    }
    if let Some(val) = headers.get("x-camp-to") {
        if let Ok(val) = val.to_str() {
            to_fmt = Some(val);
        }
    }

    let mut body = match body {
        None => {
            return (
                StatusCode::BAD_REQUEST,
                "Request requires a source document body.".to_owned(),
            )
                .into_response();
        }
        Some(body) => body,
    };

    let body_estimate = match body.size_hint() {
        (_, Some(high)) => high,
        (low, None) => low,
    };

    let mut source_buff: Vec<u8> = Vec::with_capacity(body_estimate);
    while let Some(chunk) = body.next().await {
        match chunk {
            Ok(bytes) => {
                source_buff.extend_from_slice(&bytes.slice(..));
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    format!("Error reading body: {}", &e),
                )
                    .into_response();
            }
        }
    }

    match render(source_buff, from_fmt, to_fmt).await {
        Ok(bytes) => (
            StatusCode::OK,
            [(
                HeaderName::from_static("content-type"),
                HeaderValue::from_static("application/pdf"),
            )],
            Bytes::from(bytes),
        )
            .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e).into_response(),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), String> {
    let cfg = {
        let cfg_str = std::fs::read_to_string(CFG_FILE)
            .map_err(|e| format!("Unable to read config file {:?}: {}", CFG_FILE, &e))?;
        let cfg: Cfg = toml::from_str(&cfg_str)
            .map_err(|e| format!("Unable to parse config file {:?}: {}", CFG_FILE, &e))?;
        cfg
    };
    let port_str = std::env::var("PORT").unwrap_or("".to_string());
    let port: u16 = port_str.parse().unwrap_or(cfg.port.unwrap_or(80));
    let auth_str = cfg.auth.unwrap_or(DEFAULT_AUTH.to_owned());
    println!("Listening on port {}\nwith auth str {:?}", port, &auth_str);

    let addr = SocketAddr::new("0.0.0.0".parse().unwrap(), port);
    let auth = Arc::new(auth_str);

    let app = Router::new()
        .route("/", post(handle))
        .layer(Extension(auth.clone()));

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();

    Ok(())
}

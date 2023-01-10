/*!
Mock Sendgrid endpoint for demonstration version.

Accepts all connections; writes all well-formed sendgrid request emails
to stdout.
*/

use std::net::{IpAddr, SocketAddr};

use axum::{
    Json, Router,
    http::StatusCode,
    response::IntoResponse,
    routing::post,
};
use serde::Deserialize;

const DEFAULT_PORT: u16 = 80;

/// For deserializing an email contact from the mock Sendgrid request.
#[derive(Deserialize)]
struct EmailId {
    email: String,
    name: String,
}

impl std::fmt::Display for EmailId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} <{}>", &self.name, &self.email)
    }
}

/// For deserializing the mock Sendgrid request.
#[derive(Deserialize)]
struct Email {
    to: Vec<EmailId>,
    from: EmailId,
    reply_to: EmailId,
    subject: String,
    body: String,
}

/// Responds to any well-formed mock Sendgrid request by logging it to stdout.
/// Responds to poorly-formed ones by... doing whatever axum does in that
/// case; I don't have to think about it.
async fn handle(email: Json<Email>) -> impl IntoResponse {
    println!();
    println!("to:");
    for id in email.to.iter() {
        println!("    {}", id);
    }
    println!(
        "from: {}\nreply to: {}\nsubject: {}\n{}",
        &email.from, &email.reply_to, &email. subject, &email.body
    );

    StatusCode::ACCEPTED
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let port: u16 = match std::env::var("PORT") {
        Ok(port_str) => match port_str.parse() {
            Ok(n) => n,
            Err(e) => {
                eprintln!("Unable to parse $PORT ({:?}): {}", &port_str, &e);
                DEFAULT_PORT
            },
        },
        Err(e) => {
            eprintln!("Unable to read $PORT: {}", &e);
            DEFAULT_PORT
        }
    };

    let addr = SocketAddr::new(IpAddr::from([0, 0, 0, 0]), port);
    println!("sendgrid_mock listening on {:?}", &addr);

    let router = Router::new()
        .fallback(post(handle));
    
    axum::Server::bind(&addr)
        .serve(router.into_make_service())
        .await
        .unwrap();
}
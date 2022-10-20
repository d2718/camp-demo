/*!
Here we go!
*/
use std::sync::Arc;

use axum::{
    http::StatusCode,
    middleware,
    response::{IntoResponse, Response},
    routing::{get, get_service, post},
    Extension, Form, Router,
};
use simplelog::{ColorChoice, TermLogger, TerminalMode};
use tokio::sync::RwLock;
use tower_http::services::fs::{ServeDir, ServeFile};

use camp::{config, config::Glob, inter, user::User};

async fn catchall_error_handler(e: std::io::Error) -> impl IntoResponse {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        format!("Unhandled internal error: {}", &e),
    )
}

async fn handle_login(
    Form(form): Form<inter::LoginData>,
    Extension(glob): Extension<Arc<RwLock<Glob>>>,
) -> Response {
    log::trace!("handle_login( {:?}, [ global state ]) called.", &form);

    let user = {
        let glob = glob.read().await;
        match glob.users.get(&form.uname) {
            Some(u) => u.clone(),
            None => {
                return inter::respond_bad_password(&form.uname);
            }
        }
    };

    match user {
        User::Admin(a) => inter::admin::login(a, form, glob.clone()).await,
        User::Boss(b) => inter::boss::login(b, form, glob.clone()).await,
        User::Teacher(t) => inter::teacher::login(t, form, glob.clone()).await,
        User::Student(s) => inter::student::login(s, form, glob.clone()).await,
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let log_cfg = simplelog::ConfigBuilder::new()
        .add_filter_allow_str("camp")
        .build();
    TermLogger::init(
        camp::log_level_from_env(),
        log_cfg,
        TerminalMode::Stdout,
        ColorChoice::Auto,
    )
    .unwrap();
    log::info!("Logging started.");
    log::info!("camp version {}", camp::VERSION);

    let args: Vec<String> = std::env::args().collect();

    let glob = config::load_configuration("config.toml").await.unwrap();
    let glob = Arc::new(RwLock::new(glob));

    let serve_root =
        get_service(ServeFile::new("data/index.html")).handle_error(catchall_error_handler);

    let serve_static = get_service(ServeDir::new("static")).handle_error(catchall_error_handler);

    let addr = glob.read().await.addr;
    let app = Router::new()
        .route("/boss", post(inter::boss::api))
        .route("/admin", post(inter::admin::api))
        .route("/teacher", post(inter::teacher::api))
        .layer(middleware::from_fn(inter::key_authenticate))
        .layer(middleware::from_fn(inter::request_identity))
        .route("/pwd", get(inter::password_reset))
        .route("/login", post(handle_login))
        .layer(Extension(glob.clone()))
        .nest("/static", serve_static)
        .route("/", serve_root);

    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

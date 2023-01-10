/*!
Populating the local "fake production" environment with sufficient data
to allow some experimentation.

Fake production data can be found in `crate_root/fakeprod_data`.
*/
use std::{fs::File, io::Read, path::Path};
use std::io::{self, ErrorKind};

use futures::stream::TryStreamExt;
use hyper::{Body, Client, Request};
use simplelog::{ColorChoice, TermLogger, TerminalMode};
use time::Date;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio_util::io::StreamReader;

use camp::*;
use camp::{
    config::{ConfigFile, Glob},
    course::Course,
    DATE_FMT,
    pace::Pace,
    user::{BaseUser, Role, User},
};

static CONFIG: &str      = "demo/config.toml";
static CAL_JSON: &str    = "demo/cal.json";
static DATES_JSON: &str  = "demo/dates.json";
static COURSE_DIR: &str  = "demo/courses";
static STAFF_CSV: &str   = "demo/staff.csv";
static STUDENT_CSV: &str = "demo/students.csv";
static GOALS_CSV: &str   = "demo/goals.csv";

static TEMP_TEACHER_UNAME: &str = "no";
static TEMP_TEACHER_NAME:  &str = "Nobody";
static TEMP_TEACHER_PWD:   &str = "nothing";

/**
CSV file format:

(Unlike all the other types of CSVs we use in production, this file
DOES have a header.)

```csv
role, uname, email, password, name
a,    root,  root@not.an.email,        toot
b,    boss,  boss@our.system.com,      bpwd
t,    jenny, jenny@camelotacademy.org, jpwd, Jenny Feaster
t,    irfan, irfan@camelotacademy.org, ipwd, Irfan Azam
# ... etc
```
*/
fn csv_file_to_staff<R: Read>(r: R) -> Result<(Vec<User>, Vec<String>), String> {
    log::trace!("csv_file_to_staff( ... ) called.");

    let mut csv_reader = csv::ReaderBuilder::new()
        .comment(Some(b'#'))
        .trim(csv::Trim::All)
        .flexible(true)
        .has_headers(true)
        .from_reader(r);

    let mut users: Vec<User> = Vec::new();
    let mut pwds: Vec<String> = Vec::new();

    for (n, res) in csv_reader.records().enumerate() {
        let rec = res.map_err(|e| format!("Error in CSV record {}: {}", &n, &e))?;
        let role = match rec.get(0).ok_or_else(|| format!("Line {}: no role.", &n))? {
            "a" | "A" => Role::Admin,
            "b" | "B" => Role::Boss,
            "t" | "T" => Role::Teacher,
            x => {
                return Err(format!("Line {}: unrecognized role: {:?}", &n, &x));
            }
        };

        let uname = rec
            .get(1)
            .ok_or_else(|| format!("Line {}: no uname.", &n))?
            .to_owned();
        let email = rec
            .get(2)
            .ok_or_else(|| format!("Line {}: no email.", &n))?
            .to_owned();
        let pwd = rec
            .get(3)
            .ok_or_else(|| format!("Line {}: no password.", &n))?
            .to_owned();

        let bu = BaseUser {
            uname,
            role,
            email,
            salt: String::new(),
        };
        let u = match role {
            Role::Admin => bu.into_admin(),
            Role::Boss => bu.into_boss(),
            Role::Teacher => {
                let name = rec
                    .get(4)
                    .ok_or_else(|| format!("Line {}: no name for teacher.", &n))?;
                bu.into_teacher(name.to_owned())
            }
            Role::Student => {
                return Err(format!("Line {} should not contain a student.", &n));
            }
        };

        users.push(u);
        pwds.push(pwd);
    }

    Ok((users, pwds))
}

/// Read courses from all ".mix" files in the specified directory.
fn read_course_dir<P: AsRef<Path>>(p: P) -> Result<Vec<Course>, String> {
    let p = p.as_ref();
    log::trace!("read_course_dir( {} ) called.", p.display());

    let mut courses: Vec<Course> = Vec::new();

    for res in std::fs::read_dir(p)
        .map_err(|e| format!("Error reading course dir {}: {}", p.display(), &e))?
    {
        let ent = match res {
            Ok(ent) => ent,
            Err(e) => {
                log::warn!("Error reading directory entry: {}", &e);
                continue;
            }
        };

        let path = ent.path();
        if path.extension() != Some("mix".as_ref()) {
            log::info!(
                "Skipping file without \".mix\" extension in cours dir: {}",
                &path.display()
            );
            continue;
        }

        let f = File::open(&path)
            .map_err(|e| format!("Error opening course file {}: {}", path.display(), &e))?;

        let course = Course::from_reader(f)
            .map_err(|e| format!("Error reading course file {}: {}", path.display(), &e))?;

        courses.push(course);
    }

    Ok(courses)
}

async fn read_key(uri: &str, uname: &str, pwd: &str) -> Result<String, String>  {
    let uri = format!("{}/login", uri);
    let body = format!("uname={}&password={}", uname, pwd);

    let client = Client::new();
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(body)).map_err(|e| format!(
            "Error generating body of request: {}", &e
        ))?;
    
    let resp = client.request(req).await.map_err(|e| format!(
        "Error sending request for key: {}", &e
    ))?;
    log::debug!("Login response: {}", resp.status());
    for (k, v) in resp.headers().iter() {
        let val = String::from_utf8_lossy(v.as_bytes());
        log::debug!("    {}: {}", k, &val);
    }
    let reader = StreamReader::new(
        resp.into_body()
        .map_err(|e| io::Error::new(ErrorKind::Other, e))
    );
    let reader = BufReader::new(reader);

    let mut key: Option<String> = None;
    let prefix = "    key: \"";
    let mut lines = reader.lines();
    while let Some(line) = lines.next_line().await.map_err(|e| format!(
        "Error reading from body of key response: {}", &e
    ))? {
        log::debug!("line: {}", &line);
        if let Some(chunk) = line.strip_prefix(prefix) {
            if let Some(chunk) = chunk.trim().strip_suffix('"') {
                key = Some(String::from(chunk));
                break;
            }
        }
    }
    let key = key.ok_or("Couldn't find key in login response from server.")?;

    Ok(key)
}

async fn load_dates_and_write_calendar<P: AsRef<Path>, Q: AsRef<Path>>(
    glob: &Glob,
    cal_path: P,
    dates_path: Q,
) {
    // Read and deserialize calendar file to dates.
    let p = cal_path.as_ref();
    let file_bytes = std::fs::read(p).expect(
        &format!("Unable to read calendar file: {:?}", p.display())
    );
    let date_strs: Vec<&str> = serde_json::from_slice(&file_bytes).expect(&format!(
        "Unable to deserialize contents of {:?} as JSON.", p.display()
    ));
    let dates: Vec<Date> = date_strs.into_iter()
        .map(|s| Date::parse(s, DATE_FMT).expect(&format!(
            "Unable to parse {:?} (from file {:?}) as Date.", s, p.display()
        )))
        .collect();
    
    // Set calendar dates in database.
    glob.data().read().await.set_calendar(&dates).await.map_err(|e| format!(
        "Unable to update calendar: {}", &e
    )).unwrap();

    // Read and deserialize special dates file.
    let p = dates_path.as_ref();
    let file_bytes = std::fs::read(p).expect(
        &format!("Unable to read dates file: {:?}", p.display())
    );
    let date_strs: Vec<Vec<&str>> = serde_json::from_slice(&file_bytes).expect(&format!(
        "Unable to deserialize contents of {:?} as JSON.", p.display()
    ));

    let data = glob.data();
    let store = data.read().await;
    for kvp in date_strs.into_iter() {
        let name = kvp[0];
        let day = Date::parse(kvp[1], DATE_FMT).expect(&format!(
            "Unable to parse {:?} date {:?} as Date.", name, kvp[1]
        ));
        store.set_date(name, &day).await.expect(&format!(
            "Error inserting {:?} date {} into database.", name, &day
        ));
    }
}

async fn force_reload(uri: &str, uname: &str, key: &str) {

    let client = Client::new();
    let uri = format!("{}/admin", uri);
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("x-camp-uname", uname)
        .header("x-camp-key", key)
        .header("x-camp-action", "refresh-all")
        .header("x-camp-request-id", 0)
        .body(Body::empty()).unwrap();
    
    let resp = client.request(req).await.unwrap();
    if resp.status().is_success() {
        println!("Sample data has been inserted into the database.");
    } else {
        let status = resp.status();
        let body = hyper::body::to_bytes(resp.into_body()).await.unwrap();
        let body_str = String::from_utf8_lossy(body.as_ref());
        println!(
            "Server returned status {} {:?}",
            status, status.canonical_reason()
        );
        println!("Response body:\n{}", &body_str);
    }

}

async fn autopace_students(
    glob: &mut Glob,
    uri: &str,
    admin_uname: &str,
    admin_key: &str,
) {
    println!("Inserting temporary teacher...");
    {
        let u = BaseUser {
            uname: TEMP_TEACHER_UNAME.into(),
            role: Role::Teacher,
            salt: "asdf".into(),
            email: "nobody@nowhere.not".into(),
        };
        let u = u.into_teacher(TEMP_TEACHER_NAME.into());
        glob.insert_user(&u).await.unwrap();
        glob.refresh_users().await.unwrap();
        glob.update_password(TEMP_TEACHER_UNAME, TEMP_TEACHER_PWD).await.unwrap();
    }

    force_reload(&uri, &admin_uname, &admin_key).await;
    let key = read_key(uri, TEMP_TEACHER_UNAME, TEMP_TEACHER_PWD).await.unwrap();

    println!("Pacing student calendars...");

    let unames: Vec<String> = glob.users.iter()
        .filter(|(_, u)| matches!(u, User::Student(_)))
        .map(|(uname, _)| uname.to_string())
        .collect();

    let client = Client::new();
    let uri = format!("{}/teacher", &uri);

    for (n, suname) in unames.iter().enumerate() {
        let req = Request::builder()
            .method("POST")
            .uri(&uri)
            .header("x-camp-uname", TEMP_TEACHER_UNAME)
            .header("x-camp-key", &key)
            .header("x-camp-action", "autopace")
            .header("x-camp-request-id", n)
            .body(Body::from(suname.clone())).unwrap();
        
        let resp = client.request(req).await.unwrap();
        if !resp.status().is_success() {
            eprintln!("Error autopacing student {:?}", suname);
            eprintln!("{:?}", &resp);
            let body_bytes = hyper::body::to_bytes(resp.into_body()).await.unwrap();
            eprintln!("{}", &String::from_utf8_lossy(&body_bytes));

        }
    }
    println!("Deleting temporary teacher...");
    glob.delete_user(TEMP_TEACHER_UNAME).await.unwrap();
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), UnifiedError> {
    let log_cfg = simplelog::ConfigBuilder::new()
        .add_filter_allow_str("demo_data")
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

    let mut glob = config::load_configuration(CONFIG).await?;
    let (uri, uname, pwd) = {
        let cf_bytes = std::fs::read(CONFIG)
            .expect(&format!("Error reading from {:?}", CONFIG));
        let cf: ConfigFile = toml::from_slice(&cf_bytes)
            .expect(&format!("Unable to deserialize contents of {:?}", CONFIG));
        let admin = cf.admin_uname.expect(&format!(
            "Must have admin_uname= option set in {:?}", CONFIG
        ));
        let pwd = cf.admin_password.expect(&format!(
            "Must have admin_password= option set in {:?}", CONFIG
        ));
        let uri = cf.uri.expect(&format!(
            "Must have uri= option set in {:?}", CONFIG
        ));
        (uri, admin, pwd)
    };
    let key = read_key(&uri, &uname, &pwd).await.unwrap();
    log::debug!("Key: {:?}", &key);

    {
        let courses = read_course_dir(COURSE_DIR)?;
        let data = glob.data();
        data.read().await.insert_courses(&courses).await?;
    }

    load_dates_and_write_calendar(&glob, CAL_JSON, DATES_JSON).await;

    let (users, pwds) = csv_file_to_staff(File::open(STAFF_CSV).unwrap())?;
    for u in users.iter() {
        glob.insert_user(u).await?;
    }
    glob.refresh_courses().await?;
    glob.refresh_users().await?;
    for (u, pwd) in users.iter().zip(pwds.iter()) {
        glob.update_password(u.uname(), pwd.as_str()).await?;
    }

    {
        let stud_csv = std::fs::read_to_string(STUDENT_CSV).unwrap();
        glob.upload_students(&stud_csv).await?;
    }
    glob.refresh_users().await?;

    log::info!(
        "Inserted {} Users and {} Courses.",
        &glob.users.len(),
        &glob.courses.len()
    );

    let mut n_g_ins: usize = 0;
    let paces = Pace::from_csv(File::open(GOALS_CSV).unwrap(), &glob)?;
    for p in paces.iter() {
        n_g_ins += glob.insert_goals(&p.goals).await?;
    }
    log::info!("Inserted {} Goals.", n_g_ins);
    force_reload(&uri, &uname, &key).await;
    autopace_students(&mut glob, &uri, &uname, &key).await;
    force_reload(&uri, &uname, &key).await;

    Ok(())
}

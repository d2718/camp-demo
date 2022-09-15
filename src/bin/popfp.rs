#![allow(dead_code)]
#![allow(unused_imports)]
/*!
Populating the local "fake production" environment with sufficient data
to allow some experimentation.

Fake production data can be found in `crate_root/fakeprod_data`.
*/
use std::{fs::File, io::Read, path::Path};

use simplelog::{ColorChoice, TermLogger, TerminalMode};

use camp::*;
use camp::{
    config::Glob,
    course::Course,
    pace::Pace,
    user::{BaseUser, Role, Student, Teacher, User},
};

static CONFIG: &str = "fakeprod_data/config.toml";
static COURSE_DIR: &str = "fakeprod_data/courses";
static STAFF_CSV: &str = "fakeprod_data/staff.csv";
static STUDENT_CSV: &str = "fakeprod_data/students.csv";
static GOALS_CSV: &str = "fakeprod_data/goals.csv";

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
            .map_err(|e| format!("Error opening course file {}: {}", p.display(), &e))?;

        let course = Course::from_reader(f)
            .map_err(|e| format!("Error reading course file {}: {}", p.display(), &e))?;

        courses.push(course);
    }

    Ok(courses)
}

#[cfg(feature = "fake")]
async fn nuke(glob: Glob) -> Result<(), UnifiedError> {
    log::info!("Attempting to nuke current database info.");
    glob.data().read().await.nuke_database().await?;
    glob.auth().read().await.nuke_database().await?;
    log::info!("Current data nuked.");

    Ok(())
}

#[cfg(feature = "fake")]
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), UnifiedError> {
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

    let glob = config::load_configuration(CONFIG).await?;
    nuke(glob).await?;
    let mut glob = config::load_configuration(CONFIG).await?;
    {
        let courses = read_course_dir(COURSE_DIR)?;
        let data = glob.data();
        data.read().await.insert_courses(&courses).await?;
    }
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

    Ok(())
}

#[cfg(not(feature = "fake"))]
fn main() {
    println!("World up.");
}

/*!
Structs to hold configuration data and global variables.

The main thing here is the `Glob` struct, which holds connections to both
the [`auth`] and [`store`](crate::store) databases, and therefore is in the best position
to moderate interactions with both kinds of data.
*/
use std::{
    collections::{HashMap, HashSet},
    fmt::Write,
    io::Cursor,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

use rand::{distributions, Rng};
use serde::Deserialize;
use time::Date;
use tokio::sync::RwLock;

use crate::{
    auth,
    auth::AuthResult,
    course::{Chapter, Course},
    inter,
    pace::{Goal, Pace, Source},
    store::Store,
    user::{Role, Student, User},
    UnifiedError,
};

// In general, when new users are added to the database, they are given
// randomly-generated passwords. The passwords will be drawn from
// these characters.
const DEFAULT_PASSWORD_CHARS: &str =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789!@#$%^&*()-=_+[]{};':|,.<>/?`~";

static BAD_CHARS_MSG: &str = r#"cannot contain any of the following characters: <, >, &, ""#;

/**
The characters <, >. &, " are disallowed in many types of string data because
they'll screw up HTML generation. This function checks for those characters.
*/
fn has_bad_chars(text: &str) -> bool {
    for b in text.as_bytes().iter() {
        match b {
            b'<' => {
                return true;
            }
            b'>' => {
                return true;
            }
            b'&' => {
                return true;
            }
            b'"' => {
                return true;
            }
            _ => {}
        }
    }
    false
}

/**
User names and Course symbols can only contain alphanumeric characters; this
function checks a string for characters outside these parameters.
*/
fn bad_uname(uname: &str) -> bool {
    for b in uname.as_bytes().iter() {
        if (*b >= b'a' && *b <= b'z') || (*b >= b'A' && *b <= b'Z') || (*b >= b'0' && *b <= b'9') {
            // These character ranges are okay.
        } else {
            return true;
        }
    }
    false
}

static BAD_UNAME_MSG: &str =
    "A uname can only contain alphanumeric ASCII characters: a-z, A-Z, or 0-9.";

/**
The purpose of this struct is to be deserialized directly from a TOML
configuration file.

These values will then get combined with some default values, and
massaged into data structures that are required for the system's
operation, but less amenable to being read directly from a textual
configuration file.

This struct and its members are only `pub` so that the configuration
documentation will show up with `cargo doc`.
*/
#[derive(Deserialize)]
pub struct ConfigFile {
    /// Base URI of the system, the one that should serve the login page.
    pub uri: Option<String>,
    /// Connection string for the authorization database [`auth::Db`]. See
    /// [`tokio_postgres::config::Config`] for the appropriate format(s).
    pub auth_db_connect_string: Option<String>,
    /// Connection string for the data database [`store::Store`](crate::store::Store).
    /// See again the `tokio::postgres` documentation.
    pub data_db_connect_string: Option<String>,
    /// User name of the default Admin user account who should be guaranteed
    /// to exist.
    pub admin_uname: Option<String>,
    /// Password of the default Admin user account who should be guaranteed to
    /// exist.
    ///
    /// While it should be impossible for this user to not exist while the
    /// system is running, this password can get changed through normal means.
    pub admin_password: Option<String>,
    /// Email address of the default Admin user account who should be
    /// guaranteed to exist.
    ///
    /// While it should be impossible for this user to not exist while the
    /// system is running, this email address can get changed through normal
    /// means.
    pub admin_email: Option<String>,
    /// Value of the `Authorization` header required in a Sendgrid request in
    /// order to send email.
    pub sendgrid_auth_string: String,
    /// Host to bind the TCP listening socket to.
    pub host: Option<String>,
    /// Port to bind the TCP listening socket to.
    ///
    /// This value may be overridden by the `PORT` environment variable.
    pub port: Option<u16>,
    /// Directory with [`handlebars`] templates.
    pub templates_dir: Option<String>,
    /*
    pub students_per_teacher: Option<usize>,
    pub goals_per_student: Option<usize>,
    */
}

/**
`Cfg` is an intermediate set of values between the `ConfigFile` and the `Glob`.

If I were more clever, it probably wouldn't need to exist.
*/
#[derive(Debug)]
pub struct Cfg {
    pub uri: String,
    pub auth_db_connect_string: String,
    pub data_db_connect_string: String,
    pub default_admin_uname: String,
    pub default_admin_password: String,
    pub default_admin_email: String,
    pub sendgrid_auth_string: String,
    pub addr: SocketAddr,
    pub templates_dir: PathBuf,
    /*
    pub students_per_teacher: usize,
    pub goals_per_student: usize,
    */
}

impl std::default::Default for Cfg {
    fn default() -> Self {
        Self {
            uri: "localhost:8001/".to_owned(),
            auth_db_connect_string:
                "host=localhost user=camp_test password='camp_test' dbname=camp_auth_test"
                    .to_owned(),
            data_db_connect_string:
                "host=localhost user=camp_test password='camp_test' dbname=camp_store_test"
                    .to_owned(),
            default_admin_uname: "root".to_owned(),
            default_admin_password: "toot".to_owned(),
            default_admin_email: "admin@camp.not.an.address".to_owned(),
            sendgrid_auth_string: "".to_owned(),
            addr: SocketAddr::new("0.0.0.0".parse().unwrap(), 8001),
            templates_dir: PathBuf::from("templates/"),
            /*
            students_per_teacher: 60,
            goals_per_student: 16,
            */
        }
    }
}

impl Cfg {
    #[allow(clippy::field_reassign_with_default)]
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, String> {
        let path = path.as_ref();
        let file_contents = std::fs::read_to_string(path)
            .map_err(|e| format!("Unable to read config file: {}", &e))?;
        let cf: ConfigFile = toml::from_str(&file_contents)
            .map_err(|e| format!("Unable to deserialize config file: {}", &e))?;

        let mut c = Self::default();
        c.sendgrid_auth_string = cf.sendgrid_auth_string;

        if let Some(s) = cf.uri {
            c.uri = s;
        }
        if let Some(s) = cf.auth_db_connect_string {
            c.auth_db_connect_string = s;
        }
        if let Some(s) = cf.data_db_connect_string {
            c.data_db_connect_string = s;
        }
        if let Some(s) = cf.admin_uname {
            c.default_admin_uname = s;
        }
        if let Some(s) = cf.admin_password {
            c.default_admin_password = s;
        }
        if let Some(s) = cf.admin_email {
            c.default_admin_email = s;
        }
        if let Some(s) = cf.host {
            c.addr.set_ip(
                s.parse()
                    .map_err(|e| format!("Error parsing {:?} as IP address: {}", &s, &e))?,
            );
        }
        if let Some(n) = cf.port {
            c.addr.set_port(n);
        }

        if let Ok(port_str) = std::env::var("PORT") {
            match port_str.parse::<u16>() {
                Ok(n) => {
                    log::info!("Using value of $PORT: {}", &n);
                    c.addr.set_port(n);
                }
                Err(e) => {
                    log::warn!(
                        "Unable to parse $PORT {:?}: {}; using default or configured value.",
                        &port_str,
                        &e
                    )
                }
            }
        }

        if let Some(s) = cf.templates_dir {
            c.templates_dir = PathBuf::from(&s);
        }

        Ok(c)
    }
}

/**
The `Glob` contains all the global variables and state the server process
and its handlers need to function.

It carries around a _lot_ of state: basically local copies of database
values for everything except pace goals. (It has definitely fallen prey to
feature creep and the "God Object" anti-pattern. Refactoring would definitely
involve breaking this up into more granular chunks of global data.)

As the `Glob` holds so much information (and in particular handles to both
databases), operations involving "checks" or data from multiple sources or
of multiple kinds are often most easily conducted "through" it. This is
reflected in its profusion of methods.
*/
pub struct Glob {
    auth: Arc<RwLock<auth::Db>>,
    data: Arc<RwLock<Store>>,
    pub uri: String,
    pub sendgrid_auth: String,
    pub calendar: Vec<Date>,
    pub dates: HashMap<String, Date>,
    pub courses: HashMap<i64, Course>,
    pub course_syms: HashMap<String, i64>,
    pub users: HashMap<String, User>,
    pub addr: SocketAddr,
    /*
    pub goals_per_student: usize,
    pub students_per_teacher: usize,
    */
    pub pwd_chars: Vec<char>,
}

impl<'a> Glob {
    /// Return a handle to the [`auth::Db`].
    pub fn auth(&self) -> Arc<RwLock<auth::Db>> {
        self.auth.clone()
    }
    /// Return a handle to the [`store::Store`](crate::store::Store).
    pub fn data(&self) -> Arc<RwLock<Store>> {
        self.data.clone()
    }

    /// Generate a random password (for inserting a new user).
    fn random_password(&self, length: usize) -> String {
        let dist = distributions::Slice::new(&self.pwd_chars).unwrap();
        let rng = rand::thread_rng();
        let new_pwd: String = rng.sample_iter(&dist).take(length).collect();
        new_pwd
    }

    /// Retrieve all `User` data from the database and replace the contents
    /// of the current `.users` map with it.
    pub async fn refresh_users(&mut self) -> Result<(), String> {
        log::trace!("Glob::refresh_users() called.");
        let new_users = self
            .data
            .read()
            .await
            .get_users()
            .await
            .map_err(|e| format!("Error retrieving users from Data DB: {}", &e))?;
        self.users = new_users;
        Ok(())
    }

    /// Retrieve all `Course` data from the database and replace the contents
    /// of the current `.courses` map with it.
    pub async fn refresh_courses(&mut self) -> Result<(), String> {
        log::trace!("Glob::refresh_courses() called.");
        let new_courses = self
            .data
            .read()
            .await
            .get_courses()
            .await
            .map_err(|e| format!("Error retrieving course information from Data DB: {}", &e))?;
        self.courses = new_courses;
        let new_sym_map: HashMap<String, i64> = self
            .courses
            .iter()
            .map(|(id, crs)| (crs.sym.clone(), *id))
            .collect();
        self.course_syms = new_sym_map;
        Ok(())
    }

    /// Refresh the internal list of instructional days from the values stored
    /// in the database.
    pub async fn refresh_calendar(&mut self) -> Result<(), String> {
        log::trace!("Glob::refresh_calendar() called.");
        let new_dates = self
            .data
            .read()
            .await
            .get_calendar()
            .await
            .map_err(|e| format!("Error retrieving calendar dates from Data DB: {}", &e))?;
        self.calendar = new_dates;
        self.calendar.sort();
        Ok(())
    }

    /// Refresh the HashMap of special dates with the values from the database.
    pub async fn refresh_dates(&mut self) -> Result<(), String> {
        log::trace!("Glob::refresh_dates() called.");
        let new_dates = self
            .data
            .read()
            .await
            .get_dates()
            .await
            .map_err(|e| format!("Error retrieving special dates from Data DB: {}", &e))?;
        self.dates = new_dates;
        Ok(())
    }

    /// Retrieve a reference to a given [`Course`] by its symbol.
    /// (This is slightly complicated because they are not indexed
    /// internally by course symbol.)
    pub fn course_by_sym(&self, sym: &str) -> Option<&Course> {
        match self.course_syms.get(sym) {
            Some(id) => self.courses.get(id),
            None => None,
        }
    }

    /**
    Check to see if any of a Course's data has prohibited characters.

    Because getting it right would complicate generation of HTML in certain
    places, Course symbols, Course titles, and Chapter titles may not
    contain the characters

    ```text
    < > & "
    ```
    */
    pub fn check_course_for_bad_chars(crs: &Course) -> Result<(), String> {
        if has_bad_chars(&crs.sym) {
            return Err(format!("Course symbols {}", BAD_CHARS_MSG));
        }
        if has_bad_chars(&crs.title) {
            return Err(format!("Course titles {}", BAD_CHARS_MSG));
        }

        for chp in crs.all_chapters() {
            if has_bad_chars(&chp.title) {
                return Err(format!("Chapter titles {}", BAD_CHARS_MSG));
            }
        }

        Ok(())
    }

    /// Check to see if a Chapter's title has "forbidden" characters.
    ///
    /// (See [`Glob::check_course_for_bad_chars`].)
    pub fn check_chapter_for_bad_chars(chp: &Chapter) -> Result<(), String> {
        if has_bad_chars(&chp.title) {
            return Err(format!("Chapter titles {}", BAD_CHARS_MSG));
        }
        Ok(())
    }

    /// Insert the given user into both the auth and the data databases.
    ///
    /// This takes advantage of the fact that it's necessary to insert into
    /// the data DB and get back a salt string before the user info can be
    /// inserted into the auth DB.
    pub async fn insert_user(&self, u: &User) -> Result<(), UnifiedError> {
        log::trace!("Glob::insert_user( {:?} ) called.", u);

        if bad_uname(u.uname()) {
            return Err(BAD_UNAME_MSG.to_string().into());
        }

        match u {
            User::Teacher(ref t) => {
                if has_bad_chars(&t.name) {
                    return Err(format!("Names {}", BAD_CHARS_MSG).into());
                }
            }
            User::Student(ref s) => {
                if has_bad_chars(&s.last) || has_bad_chars(&s.rest) {
                    return Err(format!("Names {}", BAD_CHARS_MSG).into());
                }
            }
            _ => { /* We don't need to check anything else. */ }
        }

        let data = self.data.read().await;
        let mut client = data.connect().await?;
        let t = client.transaction().await?;

        let salt = match u {
            User::Admin(base) => data.insert_admin(&t, &base.uname, &base.email).await?,
            User::Boss(base) => data.insert_boss(&t, &base.uname, &base.email).await?,
            User::Teacher(teach) => {
                data.insert_teacher(&t, &teach.base.uname, &teach.base.email, &teach.name)
                    .await?
            }
            User::Student(s) => {
                let mut studs = vec![s.clone()];
                data.insert_students(&t, &mut studs).await?;
                // .unwrap()ping is fine here, because we just ensured `studs`
                // was a vector of length exactly 1.
                studs.pop().unwrap().base.salt
            }
        };

        let new_password = self.random_password(32);

        {
            let auth = self.auth.read().await;
            let mut auth_client = auth.connect().await?;
            let auth_t = auth_client.transaction().await?;
            auth.add_user(&auth_t, u.uname(), &new_password, &salt)
                .await?;
            auth_t.commit().await?;
        }

        t.commit().await.map_err(|e| {
            format!(
            "Unable to commit transaction: {}\nWarning! Auth DB maybe out of sync with Data DB.", &e
        )
        })?;

        Ok(())
    }

    /**
    Insert multiple students at once, with data supplied in CSV format.

    For CSV file format, see [`Pace::from_csv`].
    */
    pub async fn upload_students(&self, csv_data: &str) -> Result<(), UnifiedError> {
        log::trace!(
            "Glob::upload_students( [ {} bytes of CSV body ] ) called.",
            &csv_data.len()
        );

        let mut reader = Cursor::new(csv_data);
        let mut students = Student::vec_from_csv_reader(&mut reader)?;
        {
            let mut not_teachers: Vec<(&str, &str, &str)> = Vec::new();
            for s in students.iter() {
                if bad_uname(&s.base.uname) {
                    return Err(BAD_UNAME_MSG.to_string().into());
                }
                if has_bad_chars(&s.last) || has_bad_chars(&s.rest) {
                    return Err(format!("Names {}", BAD_CHARS_MSG).into());
                }

                if let Some(User::Teacher(_)) = self.users.get(&s.teacher) {
                    /* This is the happy path. */
                } else {
                    not_teachers.push((&s.teacher, &s.last, &s.rest));
                }
            }

            if !not_teachers.is_empty() {
                let mut estr = String::from(
                    "You have assigned students to the following unames who are not teachers:\n",
                );
                for (t, last, rest) in not_teachers.iter() {
                    writeln!(&mut estr, "{} (assigned to {}, {})", t, last, rest).map_err(|e| {
                        format!(
                            "Error generating error message: {}\n(Task failed successfully, lol.)",
                            &e
                        )
                    })?;
                }
                return Err(UnifiedError::String(estr));
            }
        }

        let data = self.data.read().await;
        let mut data_client = data.connect().await?;
        let data_t = data_client.transaction().await?;

        let n_studs = data.insert_students(&data_t, &mut students).await?;
        log::trace!("Inserted {} Students into store.", &n_studs);

        let passwords: Vec<String> = students.iter().map(|_| self.random_password(32)).collect();
        let pword_refs: Vec<&str> = passwords.iter().map(|s| s.as_str()).collect();
        let mut uname_refs: Vec<&str> = Vec::with_capacity(students.len());
        let mut salt_refs: Vec<&str> = Vec::with_capacity(students.len());
        for s in students.iter() {
            uname_refs.push(&s.base.uname);
            salt_refs.push(&s.base.salt);
        }

        {
            let auth = self.auth.read().await;
            let mut auth_client = auth.connect().await?;
            let auth_t = auth_client.transaction().await?;

            auth.add_users(&auth_t, &uname_refs, &pword_refs, &salt_refs)
                .await?;

            auth_t.commit().await?;
        }

        data_t.commit().await.map_err(|e| {
            format!(
            "Unable to commit transaction: {}\nWarning! Auth DB maybe out of sync with Data DB.", &e
        )
        })?;

        Ok(())
    }

    /// Update the user data associated with `u.uname()` with the other data in `u`.
    pub async fn update_user(&self, u: &User) -> Result<(), UnifiedError> {
        log::trace!("Glob::update_user( {:?} ) called.", u);

        match u {
            User::Teacher(ref t) => {
                if has_bad_chars(&t.name) {
                    return Err(format!("Names {}", BAD_CHARS_MSG).into());
                }
            }
            User::Student(ref s) => {
                if has_bad_chars(&s.last) || has_bad_chars(&s.rest) {
                    return Err(format!("Names {}", BAD_CHARS_MSG).into());
                }
            }
            _ => { /* We don't need to check anything else. */ }
        }

        let data = self.data.read().await;
        let mut client = data.connect().await?;
        let t = client.transaction().await?;

        match u {
            User::Admin(_) => {
                data.update_admin(&t, u.uname(), u.email()).await?;
            }
            User::Boss(_) => {
                data.update_boss(&t, u.uname(), u.email()).await?;
            }
            User::Teacher(teach) => {
                data.update_teacher(&t, &teach.base.uname, &teach.base.email, &teach.name)
                    .await?;
            }
            User::Student(s) => {
                /*  Here we have to replace several of the fields of `s` from
                the value stored in `self.users` because the "Admin" user
                doesn't have access to them, and the values passed from the
                Admin page will not be correct. */
                let old_u = match self.users.get(&s.base.uname) {
                    Some(ou) => match ou {
                        User::Student(ous) => ous,
                        x => {
                            return Err(format!(
                                "{:?} is not a Student ({}).",
                                &s.base.uname,
                                &x.role()
                            )
                            .into());
                        }
                    },
                    None => {
                        return Err(
                            format!("{:?} is not a User in the database.", &s.base.uname).into(),
                        );
                    }
                };
                let mut s = s.clone();
                s.fall_exam = old_u.fall_exam.clone();
                s.spring_exam = old_u.spring_exam.clone();
                s.fall_exam_fraction = old_u.fall_exam_fraction;
                s.spring_exam_fraction = old_u.spring_exam_fraction;
                s.fall_notices = old_u.fall_notices;
                s.spring_notices = old_u.spring_notices;

                data.update_student(&t, &s).await?;
            }
        }

        t.commit().await?;

        Ok(())
    }

    /// Delete from the database all information associated with user name `uname`.
    pub async fn delete_user(&self, uname: &str) -> Result<(), UnifiedError> {
        log::trace!("Glob::delete_user( {:?} ) called.", uname);

        {
            let u = match self.users.get(uname) {
                None => {
                    return Err(UnifiedError::String(format!("No User {:?}.", uname)));
                }
                Some(u) => u,
            };

            if u.role() == Role::Teacher {
                let studs = self.get_students_by_teacher(u.uname());
                if !studs.is_empty() {
                    let mut estr = format!(
                        "The following Students are still assigned to Teacher {:?}\n",
                        u.uname()
                    );
                    for kid in studs.iter() {
                        estr.push_str(kid.uname());
                        estr.push('\n');
                    }
                    return Err(UnifiedError::String(estr));
                }
            }
        }

        let data = self.data.read().await;
        let mut data_client = data.connect().await?;
        let data_t = data_client.transaction().await?;

        data.delete_user(&data_t, uname).await?;
        {
            let auth = self.auth.read().await;
            let mut auth_client = auth.connect().await?;
            let auth_t = auth_client.transaction().await?;
            auth.delete_users(&auth_t, &[uname]).await?;
            auth_t.commit().await?;
        }

        if let Err(e) = data_t.commit().await {
            return Err(format!(
                "Unable to commit transaction: {}\nWarning! Auth DB maybe out of sync with Data DB.", &e
            ).into());
        }

        Ok(())
    }

    /// Set user `uname` to authenticate with the given `new_password`.
    pub async fn update_password(
        &self,
        uname: &str,
        new_password: &str,
    ) -> Result<(), UnifiedError> {
        log::trace!("Glob::update_password( {:?}, ... ) called.", uname);

        let u = self
            .users
            .get(uname)
            .ok_or_else(|| format!("There is no user with uname {:?}.", uname))?;

        self.auth
            .read()
            .await
            .set_password(uname, new_password, u.salt())
            .await?;
        Ok(())
    }

    /// Return all [`User::Student`]s who have the given teacher.
    pub fn get_students_by_teacher(&'a self, teacher_uname: &'_ str) -> Vec<&'a User> {
        log::trace!(
            "Glob::get_students_by_teacher( {:?} ) called.",
            teacher_uname
        );

        let mut stud_refs: Vec<&User> = Vec::new();
        for (_, u) in self.users.iter() {
            if let User::Student(ref s) = u {
                if s.teacher == teacher_uname {
                    stud_refs.push(u);
                }
            }
        }

        stud_refs
    }

    /**
    Delete the Chapter (from the database) with the given `id`.

    This will fail if any Students currently have the given Chapter as a Goal.
    */
    pub async fn delete_chapter(&self, id: i64) -> Result<(), UnifiedError> {
        log::trace!("Glob::delete_chapter( {:?} ) called.", &id);

        let data = self.data();
        let data_read = data.read().await;
        let mut client = data_read.connect().await?;
        let t = client.transaction().await?;

        let rows = t
            .query(
                "WITH ch_data AS (
                SELECT
                    chapters.course AS crs_n,
                    chapters.sequence AS ch_n,
                    courses.sym AS sym,
                    courses.title AS crs,
                    courses.book AS book,
                    chapters.title AS chp
                FROM chapters
                INNER JOIN courses ON
                    courses.id = chapters.course
                WHERE chapters.id = $1
            )
            SELECT
                ch_data.crs_n, goals.sym, ch_data.ch_n, goals.uname,
                ch_data.crs, ch_data.chp, ch_data.book
            FROM goals INNER JOIN ch_data ON goals.sym = ch_data.sym",
                &[&id],
            )
            .await?;

        if !rows.is_empty() {
            let row = &rows[0];
            log::debug!("{:?}", row);
            let sym: String = row.try_get("sym")?;
            let seq: i16 = row.try_get("seq")?;
            let title: String = row.try_get("crs")?;
            let chapter: String = row.try_get("chp")?;
            let book: String = match row.try_get("book")? {
                Some(s) => s,
                None => String::from("[ no listed book ]"),
            };
            let mut unames: HashSet<String> = HashSet::with_capacity(rows.len());
            for row in rows.iter() {
                let uname: String = row.try_get("uname")?;
                unames.insert(uname);
            }

            let mut estr = format!(
                "Chapter ({:?}, {:?}) ({}, {} from {}) cannot be deleted because the following users have that Chapter as a Goal:\n",
                &sym, &seq, &title, &chapter, &book
            );
            for uname in unames.iter() {
                if let Some(User::Student(ref s)) = self.users.get(uname.as_str()) {
                    writeln!(&mut estr, "{} ({}, {})", uname, &s.last, &s.rest)
                        .map_err(|e| format!("Error generating error message: {}", &e))?;
                }
            }

            return Err(estr.into());
        }

        t.execute("DELETE FROM chapters WHERE id = $1", &[&id])
            .await?;

        t.commit().await.map_err(|e| {
            format!(
                "Error commiting transaction to delete Chapter w/id {}: {}",
                &id, &e
            )
        })?;

        Ok(())
    }

    /**
    Delete from the database the Course with the given `sym`bol, along with
    all of its Chapters.

    Will fail if any Students have Chapters from the given course as Goals.
    */
    pub async fn delete_course(&self, sym: &str) -> Result<(usize, usize), UnifiedError> {
        log::trace!("Glob::delete_course( {:?} ) called.", sym);

        let data = self.data();
        let data_read = data.read().await;
        let mut client = data_read.connect().await?;
        let t = client.transaction().await?;

        let rows = t
            .query("SELECT DISTINCT uname FROM goals WHERE sym = $1", &[&sym])
            .await?;

        if !rows.is_empty() {
            let crs = self
                .course_by_sym(sym)
                .ok_or_else(|| format!("There is no course with symbol {:?}.", sym))?;
            let mut estr = format!(
                "The Course {:?} ({} from {}) cannot be deleted because the following users have Goals from that Course:\n",
                sym, &crs.title, &crs.book
            );
            for row in rows.iter() {
                let uname: &str = row.try_get("uname")?;
                if let Some(User::Student(ref s)) = self.users.get(uname) {
                    writeln!(&mut estr, "{} ({}, {})", uname, &s.last, &s.rest)
                        .map_err(|e| format!("Error generating error message: {}", &e))?;
                }
            }

            return Err(estr.into());
        }

        let tup = data_read.delete_course(&t, sym).await?;

        match t.commit().await {
            Ok(_) => Ok(tup),
            Err(e) => Err(e.into()),
        }
    }

    /// Insert the given slice of Goals into the database.
    pub async fn insert_goals(&self, goals: &[Goal]) -> Result<usize, UnifiedError> {
        log::trace!("Glob::insert_goals( [ {} Goals ] ) called.", &goals.len());

        // First we want to check the unames courses on all the goals and
        // ensure those exist before we start trying to insert. This will
        // allow us to produce a better error message for the user.
        {
            let mut unk_users: HashSet<String> = HashSet::new();
            let mut unk_courses: HashSet<String> = HashSet::new();
            for g in goals.iter() {
                match self.users.get(&g.uname) {
                    Some(User::Student(_)) => { /* This is what we hope is true! */ }
                    _ => {
                        unk_users.insert(g.uname.clone());
                    }
                }
                match g.source {
                    Source::Book(ref bch) => {
                        if self.course_syms.get(&bch.sym).is_none() {
                            unk_courses.insert(bch.sym.clone());
                        }
                    }
                    _ => {
                        return Err("Custom Courses not yet supported.".to_owned().into());
                    }
                }
            }

            if !(unk_users.is_empty() && unk_courses.is_empty()) {
                let mut estr = String::new();
                if !unk_users.is_empty() {
                    writeln!(
                        &mut estr,
                        "The following user names do not belong to known students:"
                    )
                    .map_err(|e| format!("Error preparing error message: {}!!!", &e))?;
                    for uname in unk_users.iter() {
                        writeln!(&mut estr, "{}", uname)
                            .map_err(|e| format!("Error preparing error message: {}!!!", &e))?;
                    }
                }
                if !unk_courses.is_empty() {
                    writeln!(
                        &mut estr,
                        "The following symbols do not belong to known courses:"
                    )
                    .map_err(|e| format!("Error preparing error message: {}!!!", &e))?;
                    for sym in unk_courses.iter() {
                        writeln!(&mut estr, "{}", sym)
                            .map_err(|e| format!("Error preparing error message: {}!!!", &e))?;
                    }
                }

                return Err(estr.into());
            }
        }

        let n_inserted = self.data.read().await.insert_goals(goals).await?;
        Ok(n_inserted)
    }

    /// Return the [`Pace`] calendar data for the Student with the given `uname`.
    pub async fn get_pace_by_student(&self, uname: &str) -> Result<Pace, UnifiedError> {
        log::trace!("Glob::get_pace_by_student( {:?} ) called.", uname);

        let stud = match self.users.get(uname) {
            Some(User::Student(s)) => s.clone(),
            _ => {
                return Err(format!("{:?} is not a Student in the database.", uname).into());
            }
        };
        let teach = match self.users.get(&stud.teacher) {
            Some(User::Teacher(t)) => t.clone(),
            _ => {
                return Err(format!(
                    "{:?} has teacher {:?}, but {:?} is not a teacher.",
                    &stud.base.uname, &stud.teacher, &stud.teacher
                )
                .into());
            }
        };

        let goals = self.data.read().await.get_goals_by_student(uname).await?;

        let p = Pace::new(stud, teach, goals, self)?;
        Ok(p)
    }

    /// Get [`Pace`]s for all Students who have the Teacher with the given `uname`.
    pub async fn get_paces_by_teacher(&self, tuname: &str) -> Result<Vec<Pace>, UnifiedError> {
        log::trace!("Glob::get_paces_by_teacher( {:?} ) called.", tuname);

        let teach = match self.users.get(tuname) {
            Some(User::Teacher(t)) => t.clone(),
            _ => {
                return Err(format!("{:?} is not a Teacher in the database.", tuname).into());
            }
        };

        let students = self.get_students_by_teacher(tuname);

        let mut goals = self.data.read().await.get_goals_by_teacher(tuname).await?;

        let mut goal_map: HashMap<String, Vec<Goal>> = HashMap::with_capacity(students.len());

        for g in goals.drain(..) {
            if let Some(v) = goal_map.get_mut(&g.uname) {
                (*v).push(g)
            } else {
                let uname = g.uname.clone();
                let v = vec![g];
                goal_map.insert(uname, v);
            }
        }

        for s in students {
            if goal_map.get(s.uname()).is_none() {
                goal_map.insert(s.uname().to_string(), vec![]);
            }
        }

        let mut cals: Vec<Pace> = Vec::with_capacity(goal_map.len());
        for (uname, v) in goal_map.drain() {
            let s = match self.users.get(&uname) {
                Some(User::Student(s)) => s.clone(),
                x => {
                    log::error!(
                        "Vector of goals belonging to {:?}, but this uname belongs not to a Student in the database ({:?}).",
                        &uname, &x
                    );
                    continue;
                }
            };

            let p = match Pace::new(s, teach.clone(), v, self) {
                Ok(p) => p,
                Err(e) => {
                    log::error!("Error generating Pace calendar for {:?}: {}", &uname, &e);
                    continue;
                }
            };

            cals.push(p);
        }

        Ok(cals)
    }

    pub async fn get_reports_archive_by_teacher(
        &self,
        tuname: &str
    ) -> Result<Vec<u8>, UnifiedError> {
        use std::io::Write;
        use tokio_postgres::types::{ToSql, Type};
        use zip::{CompressionMethod, write::FileOptions, ZipWriter};
        log::trace!(
            "Glob::get_reports_archive_by_teacher( {:?} ) called.",
            tuname
        );

        /*
        Okay, Praise Be To Cthulhu, this is disgusting.

        First of all, the whole idea of this function is Not The Right Thing.
        The Right Thing would have been to implement a function that returns an
        async Stream of PDF files on the `Store` struct, and push this whole
        encoding of a ZIP archive into the `inter::boss` module. But I don't
        think I have the time and experience to learn how to do that in a
        not-horribly-brittle way, so you have this, in a sort shove-one-half-
        of-the-abstraction-down-a-layer-and-one-half-of-the-abstraction-up-
        a-layer meeting-in-the-middle compromise of which I am not proud.

        Also, the implementation itself is the kind of disgusting, labyrinthine
        thing I find myself writing when trying to be asynchronously clever.
        */

        let stud_refs = self.get_students_by_teacher(tuname);
        let params: Vec<[&(dyn ToSql + Sync); 1]> = stud_refs.iter()
            .map(|u| match u {
                User::Student(s) => Some(s),
                _ => None,
            }).filter(|s| s.is_some())
            .map(|s| {
                let p: [&(dyn ToSql + Sync); 1] = [&s.unwrap().base.uname];
                p
            }).collect();

        if params.is_empty() {
            return Err(format!(
                "Teacher {:?} doesn't have any reports finalized.", tuname
            ).into());
        }
        let file_buff: Vec<u8> = Vec::new();
        let zip_opts = FileOptions::default()
            .compression_method(CompressionMethod::Stored);
        let mut zip = ZipWriter::new(std::io::Cursor::new(file_buff));
        let data = self.data();
        let reader = data.read().await;
        let mut client = reader.connect().await?;
        let t = client.transaction().await?;
        let stmt = t.prepare_typed(
            "SELECT doc FROM reports WHERE uname = $1", &[Type::TEXT]
        ).await?;

        let mut uname_n: usize = 0;
        let mut fut = t.query_opt(&stmt, &params[uname_n]);
        uname_n += 1;
        while uname_n < params.len() {
            if let Ok(Some(row)) = fut.await {
                fut = t.query_opt(&stmt, &params[uname_n]);
                if let Ok(doc) = row.try_get("doc") {
                    zip.start_file(
                        format!("{}.pdf", stud_refs[uname_n-1].uname()),
                        zip_opts
                    ).map_err(|e| format!(
                        "Error starting write of {}.pdf to archive: {}",
                        stud_refs[uname_n-1].uname(), &e
                    ))?;
                    if let Err(e) = zip.write(doc) {
                        return Err(format!(
                            "Error writing {}.pdf to archive: {}",
                            stud_refs[uname_n-1].uname(), &e
                        ).into());
                    }
                }
            } else {
                fut = t.query_opt(&stmt, &params[uname_n]);
            }
            uname_n += 1;
        }
        if let Ok(Some(row)) = fut.await {
            if let Ok(doc) = row.try_get("doc") {
                zip.start_file(
                    format!("{}.pdf", stud_refs.last().unwrap().uname()),
                    zip_opts
                ).map_err(|e| format!(
                    "Error starting write of {}.pdf to archive: {}",
                    stud_refs[uname_n-1].uname(), &e
                ))?;
                if let Err(e) = zip.write(doc) {
                    return Err(format!(
                        "Error writing {}.pdf to archive: {}",
                        stud_refs.last().unwrap().uname(), &e
                    ).into());
                }
            }
        }

        match zip.finish() {
            Ok(cursor) => Ok(cursor.into_inner()),
            Err(e) => Err(format!(
                "Error finalizing archive: {}", &e
            ).into())
        }
    }

    /**
    Delete all Goals and Students.

    This is meant to clear the database out between academic years.
    */
    pub async fn yearly_data_nuke(&mut self) -> Result<(), UnifiedError> {
        log::trace!("Glob::yearly_data_nuke() called.");

        let data_arc = self.data();
        let data = data_arc.read().await;
        let mut client = data.connect().await?;
        let t = client.transaction().await?;

        let unames = data.delete_students(&t).await?;
        let uname_refs: Vec<&str> = unames.iter().map(|s| s.as_str()).collect();

        let auth_arc = self.auth();
        let auth = auth_arc.read().await;
        let mut auth_client = auth.connect().await?;
        let auth_t = auth_client.transaction().await?;

        auth.delete_users(&auth_t, &uname_refs).await?;

        t.commit().await?;

        auth_t.commit().await.map_err(|e| format!(
            "Error removing tranche of {} users from the Auth DB ({}); Auth and Data DBs may be out of synch. The database may need manual attention from someone who understands databases. In any case, it is recommended to log back in before you continue.",
            &uname_refs.len(), &e
        ).into())
    }
}

async fn insert_default_admin_into_data_db(cfg: &Cfg, data: &Store) -> Result<User, UnifiedError> {
    {
        let mut client = data.connect().await?;
        let t = client.transaction().await?;
        data.insert_admin(&t, &cfg.default_admin_uname, &cfg.default_admin_password)
            .await?;
        t.commit().await?;
    }

    match data.get_user_by_uname(&cfg.default_admin_uname).await {
        Err(e) => Err(format!(
            "Error attempting to retrieve newly-inserted default Admin user: {}",
            &e
        )
        .into()),
        Ok(None) => Err(
            "Newly-inserted Admin still not present in Data DB for some reason."
                .to_owned()
                .into(),
        ),
        Ok(Some(u)) => Ok(u),
    }
}

async fn insert_default_admin_into_auth_db(
    cfg: &Cfg,
    u: &User,
    auth: &auth::Db,
) -> Result<(), UnifiedError> {
    let mut client = auth.connect().await?;
    let t = client.transaction().await?;
    auth.add_user(&t, u.uname(), &cfg.default_admin_password, u.salt())
        .await?;
    t.commit().await?;

    Ok(())
}

/// Loads system configuration and ensures all appropriate database tables
/// exist.
///
/// Also assures existence of default admin.
pub async fn load_configuration<P: AsRef<Path>>(path: P) -> Result<Glob, UnifiedError> {
    let cfg = Cfg::from_file(path.as_ref())?;
    log::info!("Configuration file read:\n{:#?}", &cfg);

    log::trace!("Checking state of auth DB...");
    let auth_db = auth::Db::new(cfg.auth_db_connect_string.clone());
    if let Err(e) = auth_db.ensure_db_schema().await {
        let estr = format!("Unable to ensure state of auth DB: {}", &e);
        return Err(estr.into());
    }
    log::trace!("...auth DB okay.");
    let n_old_keys = auth_db.cull_old_keys().await?;
    log::info!("Removed {} expired keys from Auth DB.", &n_old_keys);

    log::trace!("Checking state of data DB...");
    let data_db = Store::new(cfg.data_db_connect_string.clone());
    if let Err(e) = data_db.ensure_db_schema().await {
        let estr = format!("Unable to ensure state of data DB: {}", &e);
        return Err(estr.into());
    }
    log::trace!("...data DB okay.");

    log::trace!("Checking existence of default Admin in data DB...");
    let default_admin = match data_db.get_user_by_uname(&cfg.default_admin_uname).await {
        Err(e) => {
            let estr = format!(
                "Error attempting to check existence of default Admin ({}) in data DB: {}",
                &cfg.default_admin_uname, &e
            );
            return Err(estr.into());
        }
        Ok(None) => {
            log::info!(
                "Default Admin ({}) doesn't exist in data DB; inserting.",
                &cfg.default_admin_uname
            );

            let u = insert_default_admin_into_data_db(&cfg, &data_db)
                .await
                .map_err(|e| {
                    format!(
                        "Error attempting to insert default Admin user into Data DB: {}",
                        &e
                    )
                })?;
            u
        }
        Ok(Some(u)) => u,
    };
    log::trace!("Default admin OK in data DB.");

    log::trace!("Checking existence of default Admin in auth DB...");
    match auth_db
        .check_password(
            default_admin.uname(),
            &cfg.default_admin_password,
            default_admin.salt(),
        )
        .await
    {
        Err(e) => {
            let estr = format!(
                "Error checking existence of default Admin in auth DB: {}",
                &e
            );
            return Err(estr.into());
        }
        Ok(AuthResult::BadPassword) => {
            log::warn!(
                "Default Admin ({}) not using default password.",
                default_admin.uname()
            );
        }
        Ok(AuthResult::NoSuchUser) => {
            log::info!(
                "Default Admin ({}) doesn't exist in auth DB; inserting.",
                default_admin.uname()
            );
            insert_default_admin_into_auth_db(&cfg, &default_admin, &auth_db)
                .await
                .map_err(|e| {
                    format!(
                        "Error attempting to insert default Admin into Auth DB: {}",
                        &e
                    )
                })?;
            log::trace!("Default Admin inserted into auth DB.");
        }
        Ok(AuthResult::Ok) => {
            log::trace!("Default Admin password check OK.");
        }
        Ok(x) => {
            let estr = format!(
                "Default Admin password check resulted in {:?}, which just doesn't make sense.",
                &x
            );
            return Err(estr.into());
        }
    }
    log::trace!("Default Admin OK in auth DB.");

    let mut glob = Glob {
        uri: cfg.uri,
        auth: Arc::new(RwLock::new(auth_db)),
        data: Arc::new(RwLock::new(data_db)),
        sendgrid_auth: cfg.sendgrid_auth_string,
        dates: HashMap::new(),
        calendar: Vec::new(),
        courses: HashMap::new(),
        course_syms: HashMap::new(),
        users: HashMap::new(),
        addr: cfg.addr,
        /*
        goals_per_student: cfg.goals_per_student,
        students_per_teacher: cfg.students_per_teacher,
        */
        pwd_chars: DEFAULT_PASSWORD_CHARS.chars().collect(),
    };

    glob.refresh_courses().await?;
    log::info!("Retrieved {} courses from data DB.", glob.courses.len());

    glob.refresh_users().await?;
    log::info!("Retrieved {} users from data DB.", glob.users.len());

    glob.refresh_calendar().await?;
    log::info!(
        "Retrieved {} instructional days from data DB.",
        glob.calendar.len()
    );

    glob.refresh_dates().await?;
    log::info!("Retrieved {} special dates from data DB.", glob.dates.len());
    log::debug!("special dates:\n{:#?}\n", &glob.dates);

    inter::init(&cfg.templates_dir)?;

    Ok(glob)
}

#[cfg(test)]
mod tests {
    use crate::pace::{Pace, Source};
    use crate::tests::ensure_logging;
    use crate::*;

    use serial_test::serial;

    static CONFIG: &str = "fakeprod_data/config.toml";

    #[tokio::test]
    #[serial]
    async fn get_one_pace() -> Result<(), UnifiedError> {
        ensure_logging();

        let glob = config::load_configuration(CONFIG).await?;

        let p = glob.get_pace_by_student("eparker").await?;
        println!("{:#?}", &p);

        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn autopace() -> Result<(), UnifiedError> {
        ensure_logging();

        let glob = config::load_configuration(CONFIG).await?;

        let mut p: Pace = glob.get_pace_by_student("wholt").await?;
        p.autopace(&glob.calendar)?;
        for g in p.goals.iter() {
            let source = match &g.source {
                Source::Book(src) => src,
                _ => panic!("No custom chapters!"),
            };

            let crs = glob.course_by_sym(&source.sym).unwrap();
            let chp = crs.chapter(source.seq).unwrap();
            let datestr = match g.due {
                None => "None".to_string(),
                Some(d) => format!("{}", &d),
            };
            println!("{}: {} {} {:?}", &g.id, &crs.title, &chp.title, &datestr);
        }

        Ok(())
    }
}

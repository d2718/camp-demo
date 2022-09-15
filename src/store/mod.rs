/*!
Database interaction module.

TODO:
  * Better `.map_err()` annotations.

*/
use std::fmt::Write;

use rand::{distributions, Rng};
use tokio_postgres::{Client, NoTls};

mod cal;
mod courses;
mod goals;
mod users;

const DEFAULT_SALT_LENGTH: usize = 4;
const DEFAULT_SALT_CHARS: &str = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

static SCHEMA: &[(&str, &str, &str)] = &[
    // Three tables of course info: courses, chapters, and custom "chapters".
    (
        "SELECT FROM information_schema.tables WHERE table_name = 'courses'",
        "CREATE TABLE courses (
            id    BIGSERIAL PRIMARY KEY,
            sym   TEXT UNIQUE NOT NULL,
            title TEXT NOT NULL,
            book  TEXT,
            level REAL
        )",
        "DROP TABLE courses",
    ),
    (
        "SELECT FROM information_schema.tables WHERE table_name = 'chapters'",
        "CREATE TABLE chapters (
            id          BIGSERIAL PRIMARY KEY,
            course      BIGINT REFERENCES courses(id),
            sequence    SMALLINT,
            title       TEXT,   /* default is generated 'Chapter N' title */
            subject     TEXT,   /* default is blank */
            weight      REAL    /* default is 1.0 */
        )",
        "DROP TABLE chapters",
    ),
    (
        "SELECT FROM information_schema.tables WHERE table_name = 'custom_chapters'",
        "CREATE TABLE custom_chapters (
            id      BIGSERIAL PRIMARY KEY,
            uname   TEXT,   /* REFERENCES user(uname), when 'users' table available */
            title   TEXT NOT NULL,
            weight  REAL    /* default should be 1.0 */
        )",
        "DROP TABLE custom_chapters",
    ),
    (
        "SELECT FROM information_schema.tables WHERE table_name = 'users'",
        "CREATE TABLE users (
            uname TEXT PRIMARY KEY,
            role  TEXT NOT NULL,
            salt  TEXT,
            email TEXT
        )",
        "DROP TABLE users",
    ),
    (
        "SELECT FROM information_schema.tables WHERE table_name = 'teachers'",
        "CREATE TABLE teachers (
            uname TEXT UNIQUE REFERENCES users(uname),
            name  TEXT
        )",
        "DROP TABLE teachers",
    ),
    (
        "SELECT FROM information_schema.tables WHERE table_name = 'students'",
        "CREATE TABLE students (
            uname   TEXT UNIQUE REFERENCES users(uname),
            last    TEXT,
            rest    TEXT,
            teacher TEXT REFERENCES teachers(uname),
            parent  TEXT,     /* parent email address */
            fall_exam TEXT,
            spring_exam TEXT,
            fall_exam_fraction REAL,
            spring_exam_fraction REAL,
            fall_notices SMALLINT,
            spring_notices SMALLINT
        )",
        "DROP TABLE students",
    ),
    (
        "SELECT FROM information_schema.tables WHERE table_name = 'calendar'",
        "CREATE TABLE calendar ( day DATE UNIQUE NOT NULL )",
        "DROP TABLE calendar",
    ),
    (
        "SELECT FROM information_schema.tables WHERE table_name = 'dates'",
        "CREATE TABLE dates (
            name TEXT PRIMARY KEY,
            day DATE NOT NULL
        )",
        "DROP TABLE dates",
    ),
    (
        "SELECT FROM information_schema.tables WHERE table_name = 'custom_chapters'",
        "CREATE TABLE custom_chapters (
            id BIGSERIAL PRIMARY KEY,
            uname   TEXT REFERENCES user(uname),
            title   TEXT NOT NULL,
            weight  REAL
        )",
        "DROP TABLE custom_chapters",
    ),
    (
        "SELECT FROM information_schema.tables WHERE table_name = 'goals'",
        "CREATE TABLE goals (
            id          BIGSERIAL PRIMARY KEY,
            uname       TEXT REFERENCES students(uname),
            sym         TEXT REFERENCES courses(sym),
            seq         SMALLINT,
            custom      BIGINT REFERENCES custom_chapters(id),
            review      BOOL,
            incomplete  BOOL,
            due         DATE,
            done        DATE,
            tries       SMALLINT,
            score       TEXT
        )",
        "DROP TABLE goals",
    ),
];

/**
Errors returned by [`Store`] methods. Usually these are just wrapped
[`tokio_postgres`] errors (with possibly some additional context).
*/
#[derive(Debug, PartialEq)]
pub struct DbError(String);

impl DbError {
    /// Prepend some contextual `annotation` for the error.
    fn annotate(self, annotation: &str) -> Self {
        let s = format!("{}: {}", annotation, &self.0);
        Self(s)
    }

    pub fn display(&self) -> &str {
        &self.0
    }
}

impl From<tokio_postgres::error::Error> for DbError {
    fn from(e: tokio_postgres::error::Error) -> DbError {
        let mut s = format!("Data DB: {}", &e);
        if let Some(dbe) = e.as_db_error() {
            write!(&mut s, "; {}", dbe).unwrap();
        }
        DbError(s)
    }
}

impl From<String> for DbError {
    fn from(s: String) -> DbError {
        DbError(s)
    }
}

impl std::fmt::Display for DbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", &self.0)
    }
}

impl From<DbError> for String {
    fn from(val: DbError) -> Self {
        val.0
    }
}

/**
Endpoint for interacting with the underlying Postgres store.

As it stands, each "interaction" with the database will open (and when
finished close) a new connection. This is generally regarded as less
performant and involves more out-of-band data exchange than reusing
connections or using connection pools, but it is _way simpler_ to implement,
so unless and until there's a burning need to change it, it stays
inefficient but simple.

Some methods take as one of their arguments an [`&Transaction`](Transaction). These are
meant to be used in operations that may require multiple queries or
intermediate steps. For example, adding a new user to the system requires
adding an account with the user name to the system's `store::Db` and get a
salt value with which to hash their password when adding that same user name
to the `auth::Db`. If any step along the way fails, the entirety of both
transactions should be rolled back; this is made effortlessly simple if the
caller holds on to the [`Transaction`] in use.

See, for example, the source of
[`Glob::insert_user`](crate::config::Glob::insert_user)
for an example of this.
*/
pub struct Store {
    connection_string: String,
    salt_chars: Vec<char>,
    salt_length: usize,
}

impl Store {
    pub fn new(connection_string: String) -> Self {
        log::trace!("Store::new( {:?} ) called.", &connection_string);

        let salt_chars: Vec<char> = DEFAULT_SALT_CHARS.chars().collect();
        let salt_length = DEFAULT_SALT_LENGTH;

        Self {
            connection_string,
            salt_chars,
            salt_length,
        }
    }

    /// Set characters to use when generating user salt strings.
    ///
    /// Will quietly do nothing if `new_chars` has zero length.
    pub fn set_salt_chars(&mut self, new_chars: &str) {
        if !new_chars.is_empty() {
            self.salt_chars = new_chars.chars().collect();
        }
    }

    /// Set the length of salt strings to generate.
    ///
    /// Will quietly do nothing if set to zero.
    pub fn set_salt_length(&mut self, new_length: usize) {
        if new_length > 0 {
            self.salt_length = new_length;
        }
    }

    /// Generate a new user salt based on the current values of
    /// self.salt_chars and self.salt_length.
    fn generate_salt(&self) -> String {
        // self.salt_chars should never have zero length.
        let dist = distributions::Slice::new(&self.salt_chars).unwrap();
        let rng = rand::thread_rng();
        let new_salt: String = rng.sample_iter(&dist).take(self.salt_length).collect();
        new_salt
    }

    /**
    Return a connection to the underlying Postgres store.

    This connection should only ever be used to instantiate a
    [`Transaction`] for use in one of the `Store` methods that requires one:

    ```
    let mut client = my_store.connect().await?;
    let t = client.transaction().await?;
    my_store.delete_course(&t, "xa1x")?;
    ```

    */
    pub async fn connect(&self) -> Result<Client, DbError> {
        log::trace!(
            "Store::connect() called w/connection string {:?}",
            &self.connection_string
        );

        match tokio_postgres::connect(&self.connection_string, NoTls).await {
            Ok((client, connection)) => {
                log::trace!("    ...connection successful.");
                tokio::spawn(async move {
                    if let Err(e) = connection.await {
                        log::error!("Data DB connection error: {}", &e);
                    } else {
                        log::trace!("tokio connection runtime drops.");
                    }
                });
                Ok(client)
            }
            Err(e) => {
                let dberr = DbError::from(e);
                log::trace!("    ...connection failed: {:?}", &dberr);
                Err(dberr.annotate("Unable to connect"))
            }
        }
    }

    /**
    Ensure that the underlying Postgres store contains all the necessary
    tables.

    This should be called when the container starts up, but is also useful
    in setting up testing environments.
    */
    pub async fn ensure_db_schema(&self) -> Result<(), DbError> {
        log::trace!("Store::ensure_db_schema() called.");

        let mut client = self.connect().await?;
        let t = client
            .transaction()
            .await
            .map_err(|e| DbError::from(e).annotate("Data DB unable to begin transaction"))?;

        for (test_stmt, create_stmt, _) in SCHEMA.iter() {
            if t.query_opt(test_stmt.to_owned(), &[]).await?.is_none() {
                log::info!(
                    "{:?} returned no results; attempting to insert table.",
                    test_stmt
                );
                t.execute(create_stmt.to_owned(), &[]).await?;
            }
        }

        t.commit()
            .await
            .map_err(|e| DbError::from(e).annotate("Error committing transaction"))
    }

    /**
    Drop all database tables to fully reset database state.

    This is only meant for cleanup after testing. It is advisable to look at
    the ERROR level log output when testing to ensure this method did its job.
    */
    #[cfg(any(test, feature = "fake"))]
    pub async fn nuke_database(&self) -> Result<(), DbError> {
        log::trace!("Store::nuke_database() called.");

        let client = self.connect().await?;

        for (_, _, drop_stmt) in SCHEMA.iter().rev() {
            if let Err(e) = client.execute(drop_stmt.to_owned(), &[]).await {
                let err = DbError::from(e);
                log::error!("Error dropping: {:?}: {}", &drop_stmt, &err.display());
            }
        }

        log::trace!("    ....nuking comlete.");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    /*!
    These tests assume you have a Postgres instance running on your local
    machine with resources named according to what you see in the
    `static TEST_CONNECTION &str`:

    ```text
    user: camp_test
    password: camp_test

    with write access to:

    database: camp_store_test
    ```
    */
    use super::*;
    use crate::tests::ensure_logging;

    use serial_test::serial;

    pub static TEST_CONNECTION: &str =
        "host=localhost user=camp_test password='camp_test' dbname=camp_store_test";

    /**
    This function is for getting the database back in a blank slate state if
    a test panics partway through and leaves it munged.

    ```bash
    cargo test reset_store -- --ignored
    ```
    */
    #[tokio::test]
    #[ignore]
    #[serial]
    async fn reset_store() {
        ensure_logging();
        let db = Store::new(TEST_CONNECTION.to_owned());
        db.nuke_database().await.unwrap();
    }

    #[tokio::test]
    #[serial]
    async fn create_store() {
        ensure_logging();

        let db = Store::new(TEST_CONNECTION.to_owned());
        db.ensure_db_schema().await.unwrap();
        db.nuke_database().await.unwrap();
    }
}

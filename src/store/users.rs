/*
`Store` methods et. al. for dealing with the different kinds of users.

```sql
CREATE TABLE users (
    uname TEXT PRIMARY KEY,
    role  TEXT,      /* one of { 'admin', 'boss', 'teacher', 'student' } */
    salt  TEXT,
    email TEXT
);

CREATE TABLE teachers (
    uname TEXT REFERENCES users(uname),
    name  TEXT
);

CREATE TABLE students (
    uname   TEXT REFERENCES users(uname),
    last    TEXT,
    rest    TEXT,
    teacher TEXT REFERENCES teachers(uname),
    parent  TEXT,    /* parent email address */
    fall_exam   TEXT,
    spring_exam TEXT,
    fall_exam_fraction  REAL,
    spring_exam_fraction REAL,
    fall_notices   SMALLINT,
    spring_notices SMALLINT
);

```
*/
use std::collections::HashMap;
use std::fmt::Write;

use futures::stream::{FuturesUnordered, StreamExt};
use tokio_postgres::{
    types::{ToSql, Type},
    Row, Transaction,
};

use super::{DbError, Store};
use crate::blank_string_means_none;
use crate::user::*;

/**
The `TeacherSidecar` struct is to hold the contents of records queried from
the 'teachers' database table until they can be combined into a `Teacher`
struct.
*/
#[derive(Debug)]
struct TeacherSidecar {
    uname: String,
    name: String,
}

/**
The `StudentSidecar` struct is to hold the contents of records queried from
the 'students' database table until they can be combined into a `Student`
struct.
*/
#[derive(Debug)]
struct StudentSidecar {
    uname: String,
    last: String,
    rest: String,
    teacher: String,
    parent: String,
    fall_exam: Option<String>,
    spring_exam: Option<String>,
    fall_exam_fraction: f32,
    spring_exam_fraction: f32,
    fall_notices: i16,
    spring_notices: i16,
}

/// Turn a row queried from the 'users' table in to a `BaseUser.
fn base_user_from_row(row: &Row) -> Result<BaseUser, DbError> {
    log::trace!("base_user_from_row( {:?} ) called.", row);

    let role_str: &str = row.try_get("role")?;
    let bu = BaseUser {
        uname: row.try_get("uname")?,
        role: role_str.parse()?,
        salt: row.try_get("salt")?,
        email: row.try_get("email")?,
    };

    log::trace!("    ...base_user_from_row() returning {:?}", &bu);
    Ok(bu)
}

/**
Store the data from a row queried from the 'teachers' table in a
`TeacherSidecar`.

This should then be almost immediately combined with a `BaseUser` to
become a `Teacher`.
*/
fn teacher_from_row(row: &Row) -> Result<TeacherSidecar, DbError> {
    log::trace!("teacher_from_row( {:?} ) called.", row);

    let t = TeacherSidecar {
        uname: row.try_get("uname")?,
        name: row.try_get("name")?,
    };

    log::trace!("    ...teacher_from_row() returning {:?}", &t);
    Ok(t)
}

/**
Store the data from a row queried from the 'students' table in a
`StudentSidecar`.

This should them be almost immediately combined with a `BaseUser` to
become a `Student`.
*/
fn student_from_row(row: &Row) -> Result<StudentSidecar, DbError> {
    log::trace!("student_from_row( {:?} ) called.", row);

    let teacher: Option<String> = row.try_get("teacher")?;
    let teacher = match teacher {
        Some(uname) => uname,
        None => String::new(),
    };

    let s = StudentSidecar {
        uname: row.try_get("uname")?,
        last: row.try_get("last")?,
        rest: row.try_get("rest")?,
        teacher,
        parent: row.try_get("parent")?,
        fall_exam_fraction: row.try_get("fall_exam_fraction")?,
        spring_exam_fraction: row.try_get("spring_exam_fraction")?,
        fall_notices: row.try_get("fall_notices")?,
        spring_notices: row.try_get("spring_notices")?,
        fall_exam: match row.try_get("fall_exam") {
            Ok(x) => blank_string_means_none(x),
            Err(_) => None,
        },
        spring_exam: match row.try_get("spring_exam") {
            Ok(x) => blank_string_means_none(x),
            Err(_) => None,
        },
    };

    log::trace!("    ...student_from_row() returning {:?}", &s);
    Ok(s)
}

/**
Return the role of extant user `uname`, if he exists.

This function is used when inserting new users to mainly to ensure good error
messaging when a username is already in use.
*/
async fn check_existing_user_role(
    t: &Transaction<'_>,
    uname: &str,
) -> Result<Option<Role>, DbError> {
    log::trace!("check_existing_user_role( T, {:?} ) called.", uname);

    match t
        .query_opt("SELECT role FROM users WHERE uname = $1", &[&uname])
        .await
        .map_err(|e| DbError(format!("{}", &e)).annotate("Error querying for preexisting uname"))?
    {
        None => Ok(None),
        Some(row) => {
            let role_str: &str = row.try_get("role").map_err(|e| {
                DbError(format!("{}", &e)).annotate("Error getting role of preexisting uname")
            })?;
            let role: Role = role_str.parse().map_err(|e: String| {
                DbError(e).annotate("Error parsing role of preexisting uname")
            })?;
            Ok(Some(role))
        }
    }
}

impl Store {
    /**
    Deletes a user from the database, regardless of role.

    It's not clever; it tries to shotgun delete both student and teacher
    records for a given `uname` before deleting the entry from the `users`
    table. I haven't tested it, but I think this is probably faster than
    "the right thing": querying the role associated with the `uname` and
    performing an appropriate additional delete if necessary.
    */
    pub async fn delete_user(&self, t: &Transaction<'_>, uname: &str) -> Result<(), DbError> {
        log::trace!("Store::delete_user( {:?} ) called.", uname);

        /*
        JFC the type annotations here.

        This obnoxious way of passing parameters to the two following SQL
        DELETE statements is necessary to satisfy the borrow checker. Sorry.
        I absolutely invite you to make this suck less if you can.
        */
        let params: [&(dyn ToSql + Sync); 1] = [&uname];

        tokio::try_join!(
            t.execute("DELETE FROM completion WHERE uname = $1", &params[..]),
            t.execute("DELETE FROM drafts WHERE uname = $1", &params[..]),
            t.execute("DELETE FROM facts WHERE uname = $1", &params[..]),
            t.execute(
                "DELETE FROM nmr
                    WHERE id in
                    (SELECT id FROM goals WHERE uname = $1)",
                &params[..]
            ),
            t.execute("DELETE FROM reports WHERE uname = $1", &params[..]),
            t.execute("DELETE FROM social WHERE uname = $1", &params[..]),
        )?;

        let n_goals = self.delete_goals_by_student(t, uname).await?;
        log::trace!("Deleted {} Goals.", &n_goals);

        let (s_del_res, t_del_res) = tokio::join!(
            t.execute("DELETE FROM students WHERE uname = $1", &params[..]),
            t.execute("DELETE FROM teachers WHERE uname = $1", &params[..]),
        );

        match s_del_res {
            Err(e) => {
                return Err(e.into());
            }
            Ok(0) => {}
            Ok(1) => {
                log::trace!("{} student record deleted.", uname);
            }
            Ok(n) => {
                log::warn!(
                    "Deleting single student {} record affected {} rows.",
                    uname,
                    &n
                );
            }
        }
        match t_del_res {
            Err(e) => {
                return Err(e.into());
            }
            Ok(0) => {}
            Ok(1) => {
                log::trace!("{} teacher record deleted.", uname);
            }
            Ok(n) => {
                log::warn!(
                    "Deleting single teacher {} record affected {} rows.",
                    uname,
                    &n
                );
            }
        }

        let n = t
            .execute("DELETE FROM users WHERE uname = $1", &[&uname])
            .await?;

        if n == 0 {
            Err(DbError(format!("There is no user with uname {:?}.", uname)))
        } else {
            Ok(())
        }
    }

    /// Inserts the `user::BaseUser` information into the `users` table in the
    /// database and return the newly-generated salt.
    ///
    /// This is used by the `Store::insert_xxx()` methods to insert this part
    /// of the information. It also calls `check_existing_user_role()` and
    /// throws a propagable error if the given uname already exists.
    async fn insert_base_user(
        &self,
        t: &Transaction<'_>,
        uname: &str,
        email: &str,
        role: Role,
    ) -> Result<String, DbError> {
        log::trace!(
            "insert_base_user( T, {:?}, {:?}, {} ) called.",
            uname,
            email,
            role
        );

        if let Some(role) = check_existing_user_role(t, uname).await? {
            return Err(DbError(format!(
                "User name {} already exists with role {}.",
                uname, &role
            )));
        }

        let salt = self.generate_salt();

        t.execute(
            "INSERT INTO users (uname, role, salt, email)
                VALUES ($1, $2, $3, $4)",
            &[&uname, &role.to_string(), &salt, &email],
        )
        .await?;

        Ok(salt)
    }

    async fn update_base_user(
        &self,
        t: &Transaction<'_>,
        uname: &str,
        email: &str,
    ) -> Result<(), DbError> {
        log::trace!("update_base_user( T, {:?}, {:?} ) called.", uname, email);

        let n_updated = t
            .execute(
                "UPDATE users SET email = $1 WHERE uname = $2",
                &[&email, &uname],
            )
            .await?;

        if n_updated == 0 {
            Err(DbError(format!("No extant user {:?}.", uname)))
        } else if n_updated > 1 {
            log::warn!(
                "Store::update_base_user( T, {:?} ... ) updated more than 1 record!",
                uname
            );
            Ok(())
        } else {
            Ok(())
        }
    }

    pub async fn insert_admin(
        &self,
        t: &Transaction<'_>,
        uname: &str,
        email: &str,
    ) -> Result<String, DbError> {
        log::trace!("Store::insert_admin( {:?},{:?} ) called.", uname, email);

        let salt = self.insert_base_user(t, uname, email, Role::Admin).await?;

        log::trace!("Inserted Admin {:?} ({}).", uname, email);
        Ok(salt)
    }

    pub async fn update_admin(
        &self,
        t: &Transaction<'_>,
        uname: &str,
        email: &str,
    ) -> Result<(), DbError> {
        log::trace!("update_admin( {:?}, {:?} ) called.", uname, email);

        self.update_base_user(t, uname, email).await?;
        Ok(())
    }

    pub async fn insert_boss(
        &self,
        t: &Transaction<'_>,
        uname: &str,
        email: &str,
    ) -> Result<String, DbError> {
        log::trace!("Store::insert_boss( {:?}, {:?} ) called.", uname, email);

        let salt = self.insert_base_user(t, uname, email, Role::Boss).await?;

        log::trace!("Inserted Boss {:?} ({})", uname, email);
        Ok(salt)
    }

    pub async fn update_boss(
        &self,
        t: &Transaction<'_>,
        uname: &str,
        email: &str,
    ) -> Result<(), DbError> {
        log::trace!("update_admin( {:?}, {:?} ) called.", uname, email);

        self.update_base_user(t, uname, email).await?;
        Ok(())
    }

    pub async fn insert_teacher(
        &self,
        t: &Transaction<'_>,
        uname: &str,
        email: &str,
        name: &str,
    ) -> Result<String, DbError> {
        log::trace!(
            "Store::insert_teacher( {:?}, {:?}, {:?} ) called.",
            uname,
            email,
            name
        );

        let salt = self
            .insert_base_user(t, uname, email, Role::Teacher)
            .await?;

        t.execute(
            "INSERT INTO teachers (uname, name)
                VALUES ($1, $2)",
            &[&uname, &name],
        )
        .await?;

        log::trace!("Inserted Teacher {:?}, ({}, {})", uname, name, email);
        Ok(salt)
    }

    pub async fn update_teacher(
        &self,
        t: &Transaction<'_>,
        uname: &str,
        email: &str,
        name: &str,
    ) -> Result<(), DbError> {
        log::trace!(
            "Store::update_teacher( {:?}, {:?}, {:?} ) called.",
            uname,
            email,
            name
        );

        self.update_base_user(t, uname, email).await?;

        let n_updated = t
            .execute(
                "UPDATE teachers SET name = $1 WHERE uname = $2",
                &[&name, &uname],
            )
            .await?;

        if n_updated == 0 {
            return Err(DbError(format!(
                "{:?} has no entry in the 'teachers' table.",
                uname
            )));
        } else if n_updated > 1 {
            log::warn!(
                "User {:?} has {} entries in the 'teachers' table.",
                uname,
                &n_updated
            );
        }
        Ok(())
    }

    /// Insert the slice of supplied students into the database. On success,
    /// the Student objects salts are set.
    pub async fn insert_students(
        &self,
        t: &Transaction<'_>,
        students: &mut [Student],
    ) -> Result<usize, DbError> {
        log::trace!(
            "Store::insert_students( [ {} students ] ) called.",
            students.len()
        );

        let new_unames: Vec<&str> = students.iter().map(|s| s.base.uname.as_str()).collect();

        let preexisting_uname_query = t
            .prepare_typed(
                "SELECT uname, role FROM users WHERE uname = ANY($1)",
                &[Type::TEXT_ARRAY],
            )
            .await?;

        // Check to see if any of the new students have unames already in use
        // and return an informative error if so.
        let preexisting_uname_rows = t.query(&preexisting_uname_query, &[&new_unames]).await?;
        if !preexisting_uname_rows.is_empty() {
            /* Find the length of the longest uname; it will be used to format
            our error message. This finds the maximum length _in bytes_ (and
            not characters), but this is almost undoubtedly fine here.

            Also, unwrapping is okay, because there's guaranteed to be at
            least one item in the iterator, and usizes have total order. */
            let uname_len = new_unames.iter().map(|uname| uname.len()).max().unwrap();
            let mut estr =
                String::from("Database already contains users with the following unames:\n");
            for row in preexisting_uname_rows.iter() {
                let uname: &str = row.try_get("uname")?;
                let role: &str = row.try_get("role")?;
                writeln!(&mut estr, "{:width$} ({})", uname, role, width = uname_len).map_err(
                    |e| format!("There was an error preparing an error message: {}", &e),
                )?;
            }
            return Err(DbError(estr));
        }

        let (buiq, stiq) = tokio::join!(
            t.prepare_typed(
                "INSERT INTO users (uname, role, salt, email)
                    VALUES ($1, $2, $3, $4)",
                &[Type::TEXT, Type::TEXT, Type::TEXT, Type::TEXT]
            ),
            t.prepare_typed(
                "INSERT INTO students (
                    uname, last, rest, teacher, parent,
                    fall_exam, spring_exam,
                    fall_exam_fraction, spring_exam_fraction,
                    fall_notices, spring_notices
                )
                    VALUES (
                        $1, $2, $3, $4, $5,
                        $6, $7, $8, $9, $10, $11
                    )",
                &[
                    Type::TEXT,
                    Type::TEXT,
                    Type::TEXT,
                    Type::TEXT,
                    Type::TEXT,
                    Type::TEXT,
                    Type::TEXT,
                    Type::FLOAT4,
                    Type::FLOAT4,
                    Type::INT2,
                    Type::INT2
                ]
            ),
        );
        let (base_user_insert_query, student_table_insert_query) = (buiq?, stiq?);

        /*
        This next block is terrible and confusing.

        I want to run a bunch of database inserts concurrently. The
        parameters referenced in the insert statements, though, must
        be in a slice of references. These slices need to be bound
        _oustide_ the async function call that's being passed into
        `FuturesUnordered`.

        The `Student`s all exist in a slice that's been passed to
        this function, so we can refer to those unames and emails.

        We create a `String` holding the role (`"Student"`) each of
        these students will be assigned.

        We create a vector of salt strings we can reference.

        Finally we create a vector of four-element arrays (`pvec`).
        Each array holds references to the four parameters we are
        passing to the insert function to insert the corresponding
        student:
          * a reference to the `Student.base.uname`
          * a reference to the String holding the text "role"
          * a reference to one of the salts
          * a reference to the `Student.base.email`

        A reference to this array (making it a slice), will then be
        passed as the "parameters" to the insert statement.

        Phew.

        We are also putting it in its own scope, so `inserts` will drop.
        */
        let mut n_base_inserted: u64 = 0;
        let mut salts: Vec<String> = std::iter::repeat(())
            .take(students.len())
            .map(|_| self.generate_salt())
            .collect();
        {
            let student_role = Role::Student.to_string();

            let pvec: Vec<[&(dyn ToSql + Sync); 4]> = students
                .iter()
                .enumerate()
                .map(|(n, s)| {
                    let p: [&(dyn ToSql + Sync); 4] =
                        [&s.base.uname, &student_role, &salts[n], &s.base.email];
                    p
                })
                .collect();

            let mut inserts = FuturesUnordered::new();
            for params in pvec.iter() {
                inserts.push(t.execute(&base_user_insert_query, params));
            }

            while let Some(res) = inserts.next().await {
                match res {
                    Ok(_) => {
                        n_base_inserted += 1;
                    }
                    Err(e) => {
                        let estr = format!("Error inserting base user into database: {}", &e);
                        return Err(DbError(estr));
                    }
                }
            }
        }

        /*
        We're about to do a similar thing here. See the previous massive
        comment block if you're confused.
        */
        let mut n_stud_inserted: u64 = 0;
        {
            let pvec: Vec<[&(dyn ToSql + Sync); 11]> = students
                .iter()
                .map(|s| {
                    let p: [&(dyn ToSql + Sync); 11] = [
                        &s.base.uname,
                        &s.last,
                        &s.rest,
                        &s.teacher,
                        &s.parent,
                        &s.fall_exam,
                        &s.spring_exam,
                        &s.fall_exam_fraction,
                        &s.spring_exam_fraction,
                        &s.fall_notices,
                        &s.spring_notices,
                    ];
                    p
                })
                .collect();

            let mut inserts = FuturesUnordered::new();
            for params in pvec.iter() {
                inserts.push(t.execute(&student_table_insert_query, params));
            }

            while let Some(res) = inserts.next().await {
                match res {
                    Ok(_) => {
                        n_stud_inserted += 1;
                    }
                    Err(e) => {
                        let estr =
                            format!("Error inserting into students table in database: {}", &e);
                        return Err(DbError(estr));
                    }
                }
            }
        }

        for (stud, salt) in students.iter_mut().zip(salts.drain(..)) {
            stud.base.salt = salt;
        }

        log::trace!(
            "Inserted {} base users and {} student table rows.",
            &n_base_inserted,
            &n_stud_inserted
        );
        Ok(n_stud_inserted as usize)
    }

    pub async fn update_student(&self, t: &Transaction<'_>, u: &Student) -> Result<(), DbError> {
        log::trace!("Store::update_student( [ {:?} ] ) called.", &u.base.uname);

        self.update_base_user(t, &u.base.uname, &u.base.email)
            .await?;

        let teacher = match u.teacher.trim() {
            "" => None,
            x => Some(String::from(x)),
        };

        let n_updated = t
            .execute(
                "UPDATE students SET
                last = $1, rest = $2, teacher = $3, parent = $4,
                fall_exam = $5, spring_exam = $6,
                fall_exam_fraction = $7, spring_exam_fraction = $8,
                fall_notices = $9, spring_notices = $10
            WHERE uname = $11",
                &[
                    &u.last,
                    &u.rest,
                    &teacher,
                    &u.parent,
                    &u.fall_exam,
                    &u.spring_exam,
                    &u.fall_exam_fraction,
                    &u.spring_exam_fraction,
                    &u.fall_notices,
                    &u.spring_notices,
                    &u.base.uname,
                ],
            )
            .await?;

        if n_updated == 0 {
            return Err(DbError(format!(
                "{:?} has no entry in the 'students' table.",
                &u.base.uname
            )));
        } else if n_updated > 1 {
            log::warn!(
                "User {:?} has {} entries in the 'students' table.",
                &u.base.uname,
                &n_updated
            );
        }

        Ok(())
    }

    async fn get_base_users(t: &Transaction<'_>) -> Result<HashMap<String, BaseUser>, DbError> {
        log::trace!("Store::get_base_users( &T ) called.");

        let rows = t.query("SELECT * FROM users", &[]).await?;
        let mut map: HashMap<String, BaseUser> = HashMap::with_capacity(rows.len());

        for row in rows.iter() {
            let u = base_user_from_row(row)?;
            map.insert(u.uname.clone(), u);
        }

        Ok(map)
    }

    async fn get_teacher_sidecars(t: &Transaction<'_>) -> Result<Vec<TeacherSidecar>, DbError> {
        log::trace!("Store::get_teacher_sidecars( &T ) called.");

        let rows = t.query("SELECT * FROM teachers", &[]).await?;
        let mut teachers: Vec<TeacherSidecar> = Vec::with_capacity(rows.len());
        for row in rows.iter() {
            teachers.push(teacher_from_row(row)?);
        }

        log::trace!(
            "    ...Store::get_teacher_sidecars() returns {} Teachers.",
            &teachers.len()
        );
        Ok(teachers)
    }

    async fn get_student_sidecars(t: &Transaction<'_>) -> Result<Vec<StudentSidecar>, DbError> {
        log::trace!("Store::get_student_sidecars( &T ) called.");

        let rows = t.query("SELECT * FROM students", &[]).await?;
        let mut students: Vec<StudentSidecar> = Vec::with_capacity(rows.len());
        for row in rows.iter() {
            students.push(student_from_row(row)?);
        }

        log::trace!(
            "    ...Store::get_student_sidecars() returns {} Students.",
            &students.len()
        );
        Ok(students)
    }

    pub async fn get_users(&self) -> Result<HashMap<String, User>, DbError> {
        log::trace!("Store::get_users() called.");

        let mut client = self.connect().await?;
        let t = client.transaction().await?;

        let (base_res, teach_res, stud_res) = tokio::join!(
            Store::get_base_users(&t),
            Store::get_teacher_sidecars(&t),
            Store::get_student_sidecars(&t),
        );
        t.commit().await?;

        let (mut base_map, mut teach_vec, mut stud_vec) = (base_res?, teach_res?, stud_res?);
        let mut user_map: HashMap<String, User> = HashMap::with_capacity(base_map.len());

        for t in teach_vec.drain(..) {
            let base = base_map.remove(&t.uname).ok_or_else(|| {
                log::error!(
                    "Teacher {:?} has no corresponding BaseUser in database.",
                    &t.uname
                );

                format!(
"Teacher with uname {:?} has no corresponding entry in the database 'users' table.
This absolutely shouldn't be able to happen, but here we are.",
                        &t.uname
                    )
            })?;
            user_map.insert(base.uname.clone(), base.into_teacher(t.name));
        }

        for s in stud_vec.drain(..) {
            let base = base_map.remove(&s.uname).ok_or_else(|| {
                log::error!(
                    "Student {:?} has no corresponding BaseUser in database.",
                    &s.uname
                );

                format!(
"Student with uname {:?} has no corresponding entry in the database 'users' table.
This absolutely shouldn't be able to happen, but here we are.",
                        &s.uname
                    )
            })?;
            user_map.insert(
                base.uname.clone(),
                base.into_student(
                    s.last,
                    s.rest,
                    s.teacher,
                    s.parent,
                    s.fall_exam,
                    s.spring_exam,
                    s.fall_exam_fraction,
                    s.spring_exam_fraction,
                    s.fall_notices,
                    s.spring_notices,
                ),
            );
        }

        for (_, base) in base_map.drain() {
            let u: User = match base.role {
                Role::Admin => base.into_admin(),
                Role::Boss => base.into_boss(),
                x => {
                    log::error!(
                        "BaseUser {:?} has role of {}, but no corresponding sidecar in the appropriate table.",
                        &base.uname, &x
                    );
                    let estr = format!(
"User {:?} has a record in the 'users' table with role {}, but no corresponding
sidecar entry in the appropriate table for that role.
This absolutely shouldn't be able to happen, but here we are.",
                        &base.uname, &base.role
                    );
                    return Err(DbError(estr));
                }
            };

            user_map.insert(u.uname().to_string(), u);
        }

        log::trace!(
            "    ...Store::get_users() returns {} Users.",
            &user_map.len()
        );
        Ok(user_map)
    }

    async fn get_base_user_by_uname(
        t: &Transaction<'_>,
        uname: &str,
    ) -> Result<Option<BaseUser>, DbError> {
        match t
            .query_opt("SELECT * FROM users WHERE uname = $1", &[&uname])
            .await?
        {
            None => Ok(None),
            Some(row) => Ok(Some(base_user_from_row(&row)?)),
        }
    }

    async fn try_get_teacher_sidecar(
        t: &Transaction<'_>,
        uname: &str,
    ) -> Result<Option<TeacherSidecar>, DbError> {
        match t
            .query_opt("SELECT * FROM teachers WHERE uname = $1", &[&uname])
            .await?
        {
            None => Ok(None),
            Some(row) => Ok(Some(teacher_from_row(&row)?)),
        }
    }

    async fn try_get_student_sidecar(
        t: &Transaction<'_>,
        uname: &str,
    ) -> Result<Option<StudentSidecar>, DbError> {
        match t
            .query_opt("SELECT * FROM students WHERE uname = $1", &[&uname])
            .await?
        {
            None => Ok(None),
            Some(row) => Ok(Some(student_from_row(&row)?)),
        }
    }

    pub async fn get_user_by_uname(&self, uname: &str) -> Result<Option<User>, DbError> {
        log::trace!("Store::get_user_by_uname( {:?} ) called.", uname);

        let mut client = self.connect().await?;
        let t = client.transaction().await?;

        let base = match Store::get_base_user_by_uname(&t, uname).await? {
            None => {
                return Ok(None);
            }
            Some(bu) => bu,
        };

        let u = match base.role {
            Role::Admin => base.into_admin(),
            Role::Boss => base.into_boss(),
            Role::Teacher => match Store::try_get_teacher_sidecar(&t, uname).await? {
                None => {
                    log::error!(
"BaseUser {:?} has 'user' entry with role {}, but no corresponding sidecar in the appropriate table.",
                        &base.uname, &base.role
                    );
                    let estr = format!(
"User {:?} has a record in the 'users' table with role {}, but no corresponding
sidecar entry in the appropriate table for that role.
This absolutely shouldn't be able to happen, but here we are.",
                        &base.uname, &base.role
                    );
                    return Err(DbError(estr));
                }
                Some(t) => base.into_teacher(t.name),
            },
            Role::Student => match Store::try_get_student_sidecar(&t, uname).await? {
                None => {
                    log::error!(
"BaseUser {:?} has 'user' entry with role {}, but no corresponding sidecar in the appropriate table.",
                    &base.uname, &base.role
                    );
                    let estr = format!(
"User {:?} has a record in the 'users' table with role {}, but no corresponding
sidecar entry in the appropriate table for that role.
This absolutely shouldn't be able to happen, but here we are.",
                        &base.uname, &base.role
                    );
                    return Err(DbError(estr));
                }
                Some(s) => base.into_student(
                    s.last,
                    s.rest,
                    s.teacher,
                    s.parent,
                    s.fall_exam,
                    s.spring_exam,
                    s.fall_exam_fraction,
                    s.spring_exam_fraction,
                    s.fall_notices,
                    s.spring_notices,
                ),
            },
        };

        log::trace!("    ...Store::get_user_by_uname() returns {:?}", &u);
        Ok(Some(u))
    }

    /**
    Delete all Student-oriented data: everything from the `goals` table, all
    the `students` sidecar data, all the `users` with role `student`.

    This is the inter-academic-year housecleaning function. It should return
    a Vec of usernames that have been deleted, so they can be removed from the
    auth database.
    */
    pub async fn delete_students(&self, t: &Transaction<'_>) -> Result<Vec<String>, DbError> {
        log::trace!("Store::delete_students() called.");

        tokio::try_join!(
            t.execute("DELETE FROM completion", &[]),
            t.execute("DELETE FROM drafts", &[]),
            t.execute("DELETE FROM facts", &[]),
            t.execute("DELETE FROM nmr", &[]),
            t.execute("DELETE FROM reports", &[]),
            t.execute("DELETE FROM social", &[]),
        )?;
            t.execute("DELETE FROM goals", &[]).await?;
            t.execute("DELETE FROM students", &[]).await?;
        let uname_rows = t
            .query(
                "DELETE FROM users WHERE role = 'Student'
            RETURNING uname",
                &[],
            )
            .await?;

        let mut unames: Vec<String> = Vec::new();
        for row in uname_rows.iter() {
            unames.push(row.try_get("uname")?);
        }

        Ok(unames)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use serial_test::serial;

    use crate::store::tests::TEST_CONNECTION;
    use crate::tests::ensure_logging;
    use crate::UnifiedError;

    fn same_students(a: &Student, b: &Student) -> bool {
        if &a.base.uname != &b.base.uname {
            return false;
        }
        if &a.base.role != &b.base.role {
            return false;
        }
        if &a.base.email != &b.base.email {
            return false;
        }
        if &a.last != &b.last {
            return false;
        }
        if &a.rest != &b.rest {
            return false;
        }
        if &a.teacher != &b.teacher {
            return false;
        }
        if &a.parent != &b.parent {
            return false;
        }
        if &a.fall_exam != &b.fall_exam {
            return false;
        }
        if &a.spring_exam != &b.spring_exam {
            return false;
        }
        if &a.fall_exam_fraction != &b.spring_exam_fraction {
            return false;
        }
        if &a.spring_exam_fraction != &b.spring_exam_fraction {
            return false;
        }
        if &a.fall_notices != &b.fall_notices {
            return false;
        }
        if &a.spring_notices != &b.spring_notices {
            return false;
        }
        true
    }

    static ADMINS: &[(&str, &str)] = &[
        ("admin", "thelma@camelotacademy.org"),
        ("dan", "dan@camelotacademy.org"),
    ];

    static BOSSES: &[(&str, &str)] = &[
        ("boss", "boss@camelotacademy.org"),
        ("tdg", "thelma@camelotacademy.org"),
    ];

    static TEACHERS: &[(&str, &str, &str)] = &[
        ("berro", "berro@camelotacademy.org", "Mr Berro"),
        ("jenny", "jenny@camelotacademy.org", "Ms Jenny"),
        ("irfan", "irfan@camelotacademy.org", "Mr Irfan"),
    ];

    static STUDENTS_CSV: &str = "#uname, last, rest, email, parent, teacher
    frog, Frog, Frederick, fred.frog@gmail.com, ferd.frog@gmail.com, berro
    zack, Milk, Zachary, milktruck@gmail.com, handsome.dave@gmail.com, jenny
    ghill, Hill, Griffin, g.wilder.hill@gmail.com, dan@camelotacademy.org, berro
    edriver, Driver, Elaine E., ee.driver@gmail.com, arol.parker@gmail.com, irfan";

    #[tokio::test]
    #[serial]
    async fn insert_users() -> Result<(), UnifiedError> {
        ensure_logging();

        let db = Store::new(TEST_CONNECTION.to_owned());
        db.ensure_db_schema().await?;

        let mut client = db.connect().await?;
        let t = client.transaction().await?;

        for (uname, email) in ADMINS.iter() {
            db.insert_admin(&t, uname, email).await.unwrap();
        }
        for (uname, email) in BOSSES.iter() {
            db.insert_boss(&t, uname, email).await.unwrap();
        }
        for (uname, email, name) in TEACHERS.iter() {
            db.insert_teacher(&t, uname, email, name).await.unwrap();
        }

        let mut studs =
            Student::vec_from_csv_reader(std::io::Cursor::new(STUDENTS_CSV.as_bytes())).unwrap();
        assert_eq!(
            db.insert_students(&t, &mut studs).await.unwrap(),
            studs.len()
        );

        t.commit().await?;

        let mut umap = db.get_users().await.unwrap();

        let t = client.transaction().await?;

        for (uname, email) in ADMINS.iter() {
            let u = umap.remove(*uname).unwrap();
            assert_eq!(
                (*uname, *email, Role::Admin),
                (u.uname(), u.email(), u.role())
            );
            db.delete_user(&t, uname).await?;
        }
        for (uname, email) in BOSSES.iter() {
            let u = umap.remove(*uname).unwrap();
            assert_eq!(
                (*uname, *email, Role::Boss),
                (u.uname(), u.email(), u.role())
            );
            db.delete_user(&t, uname).await?;
        }

        for (uname, email, _) in TEACHERS.iter() {
            let u = umap.remove(*uname).unwrap();
            assert_eq!(
                (*uname, *email, Role::Teacher),
                (u.uname(), u.email(), u.role())
            );
        }

        for stud in studs.drain(..) {
            let s = match umap.remove(&stud.base.uname).unwrap() {
                User::Student(s) => s,
                x @ _ => panic!("Expected User::Student, got {:?}", &x),
            };
            assert!(same_students(&stud, &s));
            db.delete_user(&t, &stud.base.uname).await.unwrap();
        }

        for (uname, _, _) in TEACHERS.iter() {
            db.delete_user(&t, uname).await.unwrap();
        }

        t.commit().await?;

        db.nuke_database().await?;
        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn alter_users() -> Result<(), UnifiedError> {
        ensure_logging();

        const NEW_EMAIL: &str = "new@nowhere.org";
        const NEW_NAME: &str = "Teachy McTeacherson";

        let db = Store::new(TEST_CONNECTION.to_owned());
        db.ensure_db_schema().await.unwrap();
        let mut client = db.connect().await?;
        let t = client.transaction().await?;

        db.insert_boss(&t, BOSSES[0].0, BOSSES[0].1).await?;
        db.insert_teacher(&t, TEACHERS[0].0, TEACHERS[0].1, TEACHERS[0].2)
            .await?;
        t.commit().await?;

        let mut umap = db.get_users().await?;

        let u = umap.remove(BOSSES[0].0).unwrap();
        assert_eq!(
            (BOSSES[0].0, BOSSES[0].1, Role::Boss),
            (u.uname(), u.email(), u.role())
        );
        let t = client.transaction().await?;
        db.update_boss(&t, u.uname(), NEW_EMAIL).await?;

        let u = umap.remove(TEACHERS[0].0).unwrap();
        assert_eq!(
            (TEACHERS[0].0, TEACHERS[0].1, Role::Teacher),
            (u.uname(), u.email(), u.role())
        );
        if let User::Teacher(teach) = u {
            assert_eq!(TEACHERS[0].2, &teach.name);
            db.update_teacher(&t, &teach.base.uname, NEW_EMAIL, NEW_NAME)
                .await?;
        } else {
            panic!("User is not a teacher.");
        }
        t.commit().await?;

        umap = db.get_users().await?;

        let u = umap.remove(BOSSES[0].0).unwrap();
        assert_eq!(
            (BOSSES[0].0, NEW_EMAIL, Role::Boss),
            (u.uname(), u.email(), u.role())
        );

        let u = umap.remove(TEACHERS[0].0).unwrap();
        assert_eq!(
            (TEACHERS[0].0, NEW_EMAIL, Role::Teacher),
            (u.uname(), u.email(), u.role())
        );
        if let User::Teacher(t) = u {
            assert_eq!(NEW_NAME, &t.name);
        } else {
            panic!("User is not a teacher.");
        }

        db.nuke_database().await?;
        Ok(())
    }
}

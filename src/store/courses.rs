/*
`Store` methods et. al. for dealing with `Course` and
`Chapter` storage, update, and retrieval.

```sql

CREATE TABLE courses (
    id    SERIAL PRIMARY KEY,
    sym   TEXT UNIQUE NOT NULL,
    book  TEXT,
    title TEXT NOT NULL,
    level REAL
);

CREATE TABLE chapters (
    id       SERIAL PRIMARY KEY,
    course   INTEGER REFERENCES courses(id),
    sequence SMALLINT,
    title    TEXT,      /* NULL should give default-generated title */
    subject  TEXT,      /* NULL should just be a blank */
    weight   REAL       /* NULL should give default value of 1.0 */
);

CREATE TABLE custom_chapters (
    id    BIGSERIAL PRIMARY KEY,
    uname REFERENCES user(uname),   /* username of creator */
    title TEXT NOT NULL,
    weight REAL     /* NULL should give default value of 1.0 */
);
```
*/
use std::collections::HashMap;
use std::fmt::Write;

use tokio_postgres::{types::Type, Row, Transaction};

use super::{DbError, Store};
use crate::course::{Chapter, Course};

fn chapter_from_row(row: &Row) -> Result<Chapter, DbError> {
    Ok(Chapter {
        id: row.try_get("id")?,
        course_id: row.try_get("course")?,
        seq: row.try_get("sequence")?,
        title: row.try_get("title")?,
        subject: match row.try_get("subject") {
            Ok(x) => Some(x),
            Err(_) => None,
        },
        weight: row.try_get("weight")?,
    })
}

fn course_from_row(row: &Row) -> Result<Course, DbError> {
    Ok(Course::new(
        row.try_get("id")?,
        row.try_get("sym")?,
        row.try_get("book")?,
        row.try_get("title")?,
        row.try_get("level")?,
    ))
}

impl Store {
    /// Attempt to insert multiple courses into the database simultaneously.
    pub async fn insert_courses(&self, courses: &[Course]) -> Result<(usize, usize), DbError> {
        log::trace!(
            "Store::insert_courses( [ {} courses ] ) called.",
            courses.len()
        );

        let new_symbols: Vec<&str> = courses.iter().map(|c| c.sym.as_str()).collect();

        let mut client = self.connect().await?;
        let t = client.transaction().await?;
        let preexisting_sym_query = t
            .prepare_typed(
                "SELECT sym, title FROM courses WHERE sym = ANY($1)",
                &[Type::TEXT_ARRAY],
            )
            .await?;

        // Check to see if any of our new courses are duplicating `sym`bols
        // already in use and return with an informative error if so.
        let preexisting_sym_rows = t.query(&preexisting_sym_query, &[&new_symbols]).await?;
        if !preexisting_sym_rows.is_empty() {
            // This finds its maximum length in _bytes_, not _characters_, but
            // that's almost undoubtedly okay in this context.
            //
            // Also, unwrapping is fine here, because there's guaranteed to be
            // at least one member of `preexisting_sym_rows`, so `.max()` should
            // return `Some(n)` instead of `None`.
            let sym_len = new_symbols.iter().map(|sym| sym.len()).max().unwrap();
            let mut estr =
                String::from("Database already contains courses with the following symbols:\n");
            for row in preexisting_sym_rows.iter() {
                let sym: &str = row.try_get("sym")?;
                let title: &str = row.try_get("title")?;
                write!(&mut estr, "{:width$}  ({})", sym, title, width = sym_len).unwrap();
            }
            return Err(DbError(estr));
        }

        let insert_course_query = t
            .prepare_typed(
                "INSERT INTO courses (sym, book, title, level)
                VALUES ($1, $2, $3, $4)
                RETURNING id",
                &[Type::TEXT, Type::TEXT, Type::TEXT, Type::FLOAT4],
            )
            .await?;
        let insert_chapter_query = t
            .prepare_typed(
                "INSERT INTO chapters
                (course, sequence, title, subject, weight)
                VALUES ($1, $2, $3, $4, $5)",
                &[Type::INT8, Type::INT2, Type::TEXT, Type::TEXT, Type::FLOAT4],
            )
            .await?;

        let mut n_courses: usize = 0;
        let mut n_chapters: u64 = 0;

        // TODO: Swtich this section to use concurrent insertion, like with
        //       FuturesUnordered or somthing.
        for crs in courses.iter() {
            let row = t
                .query_one(
                    &insert_course_query,
                    &[&crs.sym, &crs.book, &crs.title, &crs.level],
                )
                .await?;
            let id: i64 = row.try_get("id")?;
            n_courses += 1;

            for ch in crs.all_chapters() {
                let n = t
                    .execute(
                        &insert_chapter_query,
                        &[&id, &ch.seq, &ch.title, &ch.subject, &ch.weight],
                    )
                    .await?;
                n_chapters += n;
            }
        }

        t.commit().await?;

        Ok((n_courses, n_chapters as usize))
    }

    /// Update the stored data on the course with symbol `c.sym` with the
    /// rest of the information in `c`.
    pub async fn update_course(&self, c: &Course) -> Result<(), DbError> {
        log::trace!("Store::update_course( {:?} ) called.", c);

        let client = self.connect().await?;

        client
            .execute(
                "UPDATE courses SET
                book = $1, title = $2, level = $3
                WHERE sym = $4",
                &[&c.book, &c.title, &c.level, &c.sym],
            )
            .await?;

        Ok(())
    }

    /// Insert the given collection of chapters into the database.
    pub async fn insert_chapters(&self, chapters: &[Chapter]) -> Result<usize, DbError> {
        log::trace!(
            "Store::insert_chapter( [ {} chapters ] ) called.",
            chapters.len()
        );

        let mut client = self.connect().await?;
        let t = client.transaction().await?;

        let insert_chapter_query = t
            .prepare_typed(
                "INSERT INTO chapters
                (course, sequence, title, subject, weight)
                VALUES ($1, $2, $3, $4, $5)",
                &[Type::INT8, Type::INT2, Type::TEXT, Type::TEXT, Type::FLOAT4],
            )
            .await?;

        let mut n_chapters: u64 = 0;

        // TODO: Switch this section to use concurrent insertion, like with
        //       FuturesUnordered.
        for ch in chapters.iter() {
            let n = t
                .execute(
                    &insert_chapter_query,
                    &[&ch.course_id, &ch.seq, &ch.title, &ch.subject, &ch.weight],
                )
                .await?;
            n_chapters += n;
        }

        t.commit().await?;

        Ok(n_chapters as usize)
    }

    /**
    Delete the chapter with the given `id` from the database.
    
    This will fail if there are still any [`Goal`](crate::pace::Goal)s that
    use this chatper, and the error message will probably be inexplicable to
    the user. This function is therefore guarded by
    [`Glob::delete_chapter`](crate::config::Glob::delete_chapter),
    which should be used instead.
    */
    pub async fn delete_chapter(&self, id: i64) -> Result<(), DbError> {
        log::trace!("Store::delete_chapter( {} ) called.", &id);

        let client = self.connect().await?;
        match client
            .execute("DELETE FROM chapters WHERE id = $1", &[&id])
            .await
        {
            Err(e) => {
                return Err(e.into());
            }
            Ok(0) => {
                return Err(DbError(format!("No Chapter with id {}.", &id)));
            }
            Ok(1) => {
                log::trace!("1 chapter record deleted.");
            }
            Ok(n) => {
                log::warn!("Deleting single chapter w/id {} affected {} rows.", &id, &n);
            }
        }

        Ok(())
    }

    /**
    Delete the course with the given `sym`bol and all its chapters from the
    database.

    This will fail if there are any assigned [`Goal`](crate::pace::Goal)s
    that use any of this course's chapters, and the error message will
    probably be inexplicable to the user. This function is therefore guarded
    by [`Glob::delete_course`](crate::config::Glob::delete_course), which
    should be used instead.
    */
    pub async fn delete_course(
        &self,
        t: &Transaction<'_>,
        sym: &str,
    ) -> Result<(usize, usize), DbError> {
        log::trace!("Store::delete_course( {:?} ) called.", sym);

        let n_chapters = t
            .execute(
                "DELETE FROM chapters
                WHERE course IN (
                    SELECT id FROM courses
                    WHERE sym = $1
                )",
                &[&sym],
            )
            .await?;

        let n_courses = t
            .execute(
                "DELETE FROM courses
                WHERE sym = $1",
                &[&sym],
            )
            .await?;

        Ok((n_courses as usize, n_chapters as usize))
    }

    /// Update the chapter in the database with the id of `ch.id` with the
    /// rest of the information in `ch`.
    pub async fn update_chapter(&self, ch: &Chapter) -> Result<(), DbError> {
        log::trace!("Store::update_chapter( {:?} ) called.", ch);

        let client = self.connect().await?;

        client
            .execute(
                "UPDATE chapters SET
                sequence = $1, title = $2, subject = $3, weight = $4
                WHERE id = $5",
                &[&ch.seq, &ch.title, &ch.subject, &ch.weight, &ch.id],
            )
            .await?;

        Ok(())
    }

    /// Retrieve the course with the given `sym`bol and wrap it up
    /// in a [`Course`] struct.
    pub async fn get_course_by_sym(&self, sym: &str) -> Result<Option<Course>, DbError> {
        log::trace!("Store::get_course_by_sym( {:?} ) called.", sym);

        let client = self.connect().await?;

        let row = match client
            .query_opt("SELECT * FROM courses WHERE sym = $1", &[&sym])
            .await?
        {
            None => {
                return Ok(None);
            }
            Some(row) => row,
        };

        let crs = Course::new(
            row.try_get("id")?,
            row.try_get("sym")?,
            row.try_get("book")?,
            row.try_get("title")?,
            row.try_get("level")?,
        );

        let rows = client
            .query(
                "SELECT * FROM chapters WHERE course = $1
                ORDER BY sequence",
                &[&crs.id],
            )
            .await?;
        let mut chapters: Vec<Chapter> = Vec::with_capacity(rows.len());
        for row in rows.iter() {
            match chapter_from_row(row) {
                Ok(ch) => {
                    chapters.push(ch);
                }
                Err(e) => {
                    return Err(e.annotate("Unable to generate Chapter from Data DB row"));
                }
            }
        }

        Ok(Some(crs.with_chapters(chapters)))
    }

    /// Return a HashMap of all courses in the database.
    pub async fn get_courses(&self) -> Result<HashMap<i64, Course>, DbError> {
        let mut client = self.connect().await?;
        let t = client.transaction().await?;

        let course_rows = t.query("SELECT * FROM courses", &[]).await?;
        let mut course_map: HashMap<i64, Course> = HashMap::with_capacity(course_rows.len());
        let mut vec_map: HashMap<i64, Vec<Chapter>> = HashMap::with_capacity(course_rows.len());
        for row in course_rows.iter() {
            let crs = course_from_row(row)?;
            vec_map.insert(crs.id, Vec::new());
            course_map.insert(crs.id, crs);
        }

        let chapter_rows = t
            .query(
                "SELECT * from chapters
                ORDER BY sequence",
                &[],
            )
            .await?;
        for row in chapter_rows.iter() {
            let ch = chapter_from_row(row)?;
            vec_map.get_mut(&ch.course_id).unwrap().push(ch);
        }

        for (id, chaps) in vec_map.drain() {
            let crs = course_map.remove(&id).unwrap();
            course_map.insert(id, crs.with_chapters(chaps));
        }

        Ok(course_map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs::File;

    use float_cmp::approx_eq;
    use serial_test::serial;

    use crate::store::tests::TEST_CONNECTION;
    use crate::tests::ensure_logging;

    fn same_chapters(a: &Chapter, b: &Chapter) -> bool {
        if a.seq != b.seq {
            return false;
        }
        if !approx_eq!(f32, a.weight, b.weight) {
            return false;
        }
        if &a.title != &b.title {
            return false;
        }
        if &a.subject != &b.subject {
            return false;
        }
        true
    }

    fn same_courses(a: &Course, b: &Course) -> bool {
        if !approx_eq!(f32, a.level, b.level) {
            return false;
        }
        if &a.sym != &b.sym {
            return false;
        }
        if &a.title != &b.title {
            return false;
        }
        if &a.book != &b.book {
            return false;
        }

        for (x, y) in a.all_chapters().zip(b.all_chapters()) {
            if !same_chapters(x, y) {
                return false;
            }
        }

        true
    }

    #[tokio::test]
    #[serial]
    async fn insert_course() {
        ensure_logging();

        let cpc = Course::from_reader(File::open("test/good_course_0.mix").unwrap()).unwrap();
        let hdg = Course::from_reader(File::open("test/good_course_2.mix").unwrap()).unwrap();
        let tot_chp = cpc.all_chapters().count() + hdg.all_chapters().count();

        let course_vec = vec![cpc, hdg];

        let db = Store::new(TEST_CONNECTION.to_owned());
        db.ensure_db_schema().await.unwrap();

        let (n_crs, n_chp) = db.insert_courses(&course_vec).await.unwrap();
        assert_eq!((n_crs, n_chp), (2, tot_chp));

        assert!(db.insert_courses(&course_vec[0..1]).await.is_err());

        let new_cpc = db.get_course_by_sym("pc").await.unwrap().unwrap();
        assert!(same_courses(&course_vec[0], &new_cpc));
        assert!(!same_courses(&course_vec[1], &new_cpc));

        db.nuke_database().await.unwrap();
    }

    #[tokio::test]
    #[serial]
    async fn get_all_courses() {
        ensure_logging();

        let course_files: &[&str] = &[
            "test/good_course_0.mix",
            // "test/good_course_1.mix", // same as test/good_course_0.mix
            "test/good_course_2.mix",
            "test/good_course_3.mix",
        ];

        let loaded_courses: Vec<Course> = course_files
            .iter()
            .map(|fname| Course::from_reader(File::open(fname).unwrap()).unwrap())
            .collect();

        let db = Store::new(TEST_CONNECTION.to_owned());
        db.ensure_db_schema().await.unwrap();

        let (n_crs, n_chap) = db.insert_courses(&loaded_courses).await.unwrap();
        log::trace!("Loaded {} courses, {} chapters.", &n_crs, &n_chap);

        let course_map = db.get_courses().await.unwrap();
        let mut sym_map: HashMap<String, i64> = HashMap::with_capacity(course_map.len());
        for (id, crs) in course_map.iter() {
            sym_map.insert(crs.sym.to_owned(), *id);
        }

        for lcrs in loaded_courses.iter() {
            assert!(same_courses(
                lcrs,
                course_map.get(sym_map.get(&lcrs.sym).unwrap()).unwrap()
            ));
        }

        db.nuke_database().await.unwrap();
    }
}

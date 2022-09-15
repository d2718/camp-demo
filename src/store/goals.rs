/*!
`Store` methods et. al. for dealing with `Goal` insertion, update,
and retrieval.

```sql
CREATE TABLE goals (
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
    score   TEXT
);
```
*/
use futures::stream::{FuturesUnordered, StreamExt};
use tokio_postgres::{types::ToSql, types::Type, Row, Transaction};

use super::{DbError, Store};
use crate::pace::{BookCh, Goal, Source};

fn goal_from_row(row: &Row) -> Result<Goal, DbError> {
    let bkch = BookCh {
        sym: row.try_get("sym")?,
        seq: row.try_get("seq")?,
        // Gets set in the `Pace` constructor.
        level: 0.0,
    };

    Ok(Goal {
        id: row.try_get("id")?,
        uname: row.try_get("uname")?,
        source: Source::Book(bkch),
        review: row.try_get("review")?,
        incomplete: row.try_get("incomplete")?,
        due: row.try_get("due")?,
        done: row.try_get("done")?,
        tries: row.try_get("tries")?,
        // Gets set in the `Pace` constructor.
        weight: 0.0,
        score: row.try_get("score")?,
    })
}

impl Store {
    /**
    Insert the supplied [`Goal`]s into the database.

    [`Glob::insert_goals`](crate::config::Glob::insert_goals) calls this method
    and supplies better error messages; it should be used instead.
    */
    pub async fn insert_goals(&self, goals: &[Goal]) -> Result<usize, DbError> {
        log::trace!("Store::insert_goals( [ {} goals ] ) called.", &goals.len());

        // Make copies of all the book `Source`s, and throw an error on custom
        // ones because we don't support those yet.
        for g in goals.iter() {
            if let Source::Custom(_) = &g.source {
                return Err(DbError("Custom Sources are unsupported.".to_owned()));
            }
        }
        let sources: Vec<BookCh> = goals
            .iter()
            .map(|g| match g.source {
                Source::Book(ref bch) => bch.clone(),
                _ => panic!("We just checked, and there shouldn't be any Custom Sources."),
            })
            .collect();

        let mut client = self.connect().await?;
        let t = client.transaction().await?;

        let insert_stmt = t
            .prepare_typed(
                "INSERT INTO goals (
                uname, sym, seq, review, incomplete,
                due, done
            )
            VALUES (
                $1, $2, $3, $4, $5,
                $6, $7
            )",
                &[
                    Type::TEXT,
                    Type::TEXT,
                    Type::INT2,
                    Type::BOOL,
                    Type::BOOL,
                    Type::DATE,
                    Type::DATE,
                ],
            )
            .await?;

        let pvec: Vec<[&(dyn ToSql + Sync); 7]> = goals
            .iter()
            .zip(sources.iter())
            .map(|(g, src)| {
                let p: [&(dyn ToSql + Sync); 7] = [
                    &g.uname,
                    &src.sym,
                    &src.seq,
                    &g.review,
                    &g.incomplete,
                    &g.due,
                    &g.done,
                ];
                p
            })
            .collect();

        let mut n_inserted: u64 = 0;
        {
            let mut inserts = FuturesUnordered::new();
            for params in pvec.iter() {
                inserts.push(t.execute(&insert_stmt, params));
            }

            while let Some(res) = inserts.next().await {
                match res {
                    Ok(_) => {
                        n_inserted += 1;
                    }
                    Err(e) => {
                        let estr = format!("Error inserting Goal into database: {}", &e);
                        return Err(DbError(estr));
                    }
                }
            }
        }

        t.commit().await?;

        Ok(n_inserted as usize)
    }

    /// Insert a single [`Goal`].
    pub async fn insert_one_goal(&self, g: &Goal) -> Result<(), DbError> {
        log::trace!("Store::insert_one_goal( {:?} ) called.", g);

        let src = match &g.source {
            Source::Book(bch) => bch,
            _ => {
                return Err(DbError("Custom sources not yet supported.".to_owned()));
            }
        };

        let client = self.connect().await?;

        client
            .execute(
                "INSERT INTO goals (
                uname, sym, seq, review, incomplete,
                due, done
            )
            VALUES (
                $1, $2, $3, $4, $5,
                $6, $7
            )",
                &[
                    &g.uname,
                    &src.sym,
                    &src.seq,
                    &g.review,
                    &g.incomplete,
                    &g.due,
                    &g.done,
                ],
            )
            .await?;

        Ok(())
    }

    /// Update the goal in the database with the `id` of  `g.id` with the
    /// rest of the information in `g`.
    pub async fn update_goal(&self, g: &Goal) -> Result<(), DbError> {
        log::trace!("Store_update_goal( {:?} ) called.", g);

        let src = match &g.source {
            Source::Book(bch) => bch,
            _ => {
                return Err(DbError("Custom sources not yet supported.".to_owned()));
            }
        };

        let client = self.connect().await?;

        client
            .execute(
                "UPDATE goals SET
                sym = $1, seq = $2, review = $3, incomplete = $4,
                due = $5, done = $6, tries = $7, score = $8
            WHERE id = $9",
                &[
                    &src.sym,
                    &src.seq,
                    &g.review,
                    &g.incomplete,
                    &g.due,
                    &g.done,
                    &g.tries,
                    &g.score,
                    &g.id,
                ],
            )
            .await?;

        Ok(())
    }

    /**
    Update the due dates of the goals in the databases with `id`s that match
    those in `goals` with the due dates from the `Goal`s in `goals.

    This function only affects the due dates of the goals in question; it is
    used when autopacing a student's calendar.
    */
    pub async fn update_due_dates(&self, goals: &[Goal]) -> Result<usize, DbError> {
        log::trace!("Store::update_goals( [ {} goals] ) called.", &goals.len());

        let mut client = self.connect().await?;
        let t = client.transaction().await?;

        let update_stmt = t
            .prepare_typed(
                "UPDATE goals SET due = $1 WHERE id = $2",
                &[Type::DATE, Type::INT8],
            )
            .await?;

        let pvec: Vec<[&(dyn ToSql + Sync); 2]> = goals
            .iter()
            .map(|g| {
                let p: [&(dyn ToSql + Sync); 2] = [&g.due, &g.id];
                p
            })
            .collect();

        let mut n_changed: u64 = 0;
        {
            let mut inserts = FuturesUnordered::new();
            for params in pvec.iter() {
                inserts.push(t.execute(&update_stmt, params));
            }

            while let Some(res) = inserts.next().await {
                match res {
                    Ok(n) => {
                        n_changed += n;
                    }
                    Err(e) => {
                        let estr = format!("Error updating goal: {}", &e);
                        return Err(DbError(estr));
                    }
                }
            }
        }
        t.commit().await?;

        Ok(n_changed as usize)
    }

    /// Delete the goal with the given `id` from the database.
    pub async fn delete_goal(&self, id: i64) -> Result<String, DbError> {
        log::trace!("Store::delete_goal( {} ) called.", &id);

        let client = self.connect().await?;

        let row = client
            .query_one("DELETE FROM goals WHERE id = $1 RETURNING uname", &[&id])
            .await?;

        let uname: String = row.try_get("uname")?;

        Ok(uname)
    }

    /// Fetch all of a student's pace goals and wrap them in a vector of
    /// [`Goal`]s.
    pub async fn get_goals_by_student(&self, uname: &str) -> Result<Vec<Goal>, DbError> {
        log::trace!("Store::get_goals_by_student( {:?} ) called.", uname);

        let client = self.connect().await?;

        let rows = client
            .query("SELECT * FROM goals WHERE uname = $1", &[&uname])
            .await?;

        let mut goals: Vec<Goal> = Vec::with_capacity(rows.len());
        for row in rows.iter() {
            match goal_from_row(row) {
                Ok(g) => {
                    goals.push(g);
                }
                Err(e) => {
                    return Err(DbError(format!(
                        "Unable to read Goal from database: {}",
                        &e
                    )));
                }
            }
        }

        Ok(goals)
    }

    /// Delete all of a student's pace goals.
    pub async fn delete_goals_by_student(
        &self,
        t: &Transaction<'_>,
        uname: &str,
    ) -> Result<usize, DbError> {
        log::trace!("Store::delete_goals_by_student( {:?} ) called.", uname);

        let n_goals = t
            .execute("DELETE FROM goals WHERE uname = $1", &[&uname])
            .await?;

        Ok(n_goals as usize)
    }

    /// Retrieve all of the goals of students who have the given teacher.
    ///
    /// This is used, among other things, to fetch data for the teacher's
    /// view.
    pub async fn get_goals_by_teacher(&self, tuname: &str) -> Result<Vec<Goal>, DbError> {
        log::trace!("Store::get_goals_by_teacher( {:?} ) called.", tuname);

        let client = self.connect().await?;

        let rows = client
            .query(
                "SELECT
                id, goals.uname, sym, seq, custom, review, incomplete,
                due, done, tries, score
            FROM
                goals INNER JOIN students ON goals.uname = students.uname
            WHERE
                students.teacher = $1",
                &[&tuname],
            )
            .await?;

        let mut goals: Vec<Goal> = Vec::with_capacity(rows.len());
        for row in rows.iter() {
            match goal_from_row(row) {
                Ok(g) => {
                    goals.push(g);
                }
                Err(e) => {
                    log::warn!("Fetching goals for teacher {:?}: {}.", tuname, &e);
                }
            }
        }

        Ok(goals)
    }
}

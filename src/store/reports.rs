/*!
All the extra monstrosity demanded for report writing.

CREATE TABLE nmr (
    id      BIGINT PRIMARY KEY REFERENCES goals(id),
    status  TEXT    /* one of { NULL, 'M', 'R' } */
);

CREATE TABLE facts (
    uname   TEXT REFERENCES students(uname),
    add     TEXT,
    sub     TEXT,
    mul     TEXT,
    div     TEXT
);

CREATE TABLE social (
    uname   TEXT REFERENCES students(uname),
    term    TEXT,   /* one of { 'Fall', 'Spring', 'Summer' } */
    trait   TEXT,
    score   TEXT    /* 1- (worst) to 3+ (best) */
);

CREATE TABLE completion (
    uname   TEXT REFERENCES students(uname),
    term    TEXT,   /* one of { 'Fall', 'Spring', 'Summer' } */
    courses TEXT,
    year    INT
);

CREATE TABLE drafts (
    uname   TEXT REFERENCES students(uname),
    term    TEXT,
    draft   TEXT
);

CREATE TABLE reports (
    uname   TEXT REFERENCES students(uname),
    term    TEXT,
    doc     bytea
);
*/
use std::{
    collections::HashMap,
    fmt::Debug,
    str::FromStr,
};

use futures::{
    stream::{FuturesUnordered, StreamExt},
    try_join,
};
use tokio_postgres::{
    types::{ToSql, Type},
    Row, Transaction,
};

use super::{DbError, Store};
use crate::{
    blank_string_means_none,
    hist::HistEntry,
    pace::Term, report::*,
};

fn row2mastery(row: &Row) -> Result<Mastery, DbError> {
    let status: Option<&str> = row.try_get("status")?;

    let m = Mastery {
        id: row.try_get("id")?,
        status: MasteryStatus::try_from(blank_string_means_none(status))?,
    };

    Ok(m)
}

impl Store {
    pub async fn set_mastery(t: &Transaction<'_>, stati: &[Mastery]) -> Result<usize, DbError> {
        log::trace!("Store::set_mastery( [ &T ], {:?} ) called.", &stati);

        let update_statement = t
            .prepare_typed(
                "INSERT INTO nmr (id, status)
                VALUES ($1, $2)
                ON CONFLICT ON CONSTRAINT nmr_pkey
                DO UPDATE SET status = $2",
                &[Type::INT8, Type::TEXT],
            )
            .await?;

        let mastery_strs: Vec<Option<&str>> = stati.iter().map(|m| m.status.as_sql()).collect();

        let mut n_set: usize = 0;
        {
            let data_refs: Vec<[&(dyn ToSql + Sync); 2]> = stati
                .iter()
                .enumerate()
                .map(|(n, m)| {
                    let p: [&(dyn ToSql + Sync); 2] = [&m.id, &mastery_strs[n]];
                    p
                })
                .collect();

            let mut inserts = FuturesUnordered::new();
            for params in data_refs.iter() {
                inserts.push(t.execute(&update_statement, &params[..]));
            }

            while let Some(res) = inserts.next().await {
                match res {
                    Ok(_) => {
                        n_set += 1;
                    }
                    Err(e) => {
                        let estr = format!("Error updating Goal mastery status: {}", &e);
                        return Err(DbError(estr));
                    }
                }
            }
        }

        Ok(n_set)
    }

    pub async fn get_mastery(t: &Transaction<'_>, uname: &str) -> Result<Vec<Mastery>, DbError> {
        log::trace!("Store::get_mastery( [ &T ], {:?} ) called.", uname);

        let rows = t
            .query(
                "SELECT goals.id, status FROM nmr
                INNER JOIN goals ON nmr.id = goals.id
            WHERE goals.uname = $1",
                &[&uname],
            )
            .await?;

        let mut masteries: Vec<Mastery> = Vec::with_capacity(rows.len());
        for row in rows.iter() {
            let m =
                row2mastery(row).map_err(|e| e.annotate("Error reading Mastery from DB row"))?;
            masteries.push(m)
        }

        Ok(masteries)
    }

    pub async fn get_facts(t: &Transaction<'_>, uname: &str) -> Result<FactSet, DbError> {
        log::trace!("Store::get_facts( [ &T ], {:?} ) called.", uname);

        let opt = t
            .query_opt(
                "SELECT add, sub, mul, div FROM facts
                WHERE uname = $1",
                &[&uname],
            )
            .await?;

        match opt {
            Some(row) => {
                let add: &str = row.try_get("add")?;
                let sub: &str = row.try_get("sub")?;
                let mul: &str = row.try_get("mul")?;
                let div: &str = row.try_get("div")?;

                Ok(FactSet {
                    add: add.into(),
                    sub: sub.into(),
                    mul: mul.into(),
                    div: div.into(),
                })
            }
            None => Ok(FactSet::default()),
        }
    }

    pub async fn set_facts(
        t: &Transaction<'_>,
        uname: &str,
        facts: &FactSet,
    ) -> Result<(), DbError> {
        log::trace!(
            "Store::set_facts( [ &T ], {:?}, {:?} ) called.",
            uname,
            facts
        );

        let opt = t
            .query_opt("SELECT FROM facts WHERE uname = $1", &[&uname])
            .await?;

        let params: [&(dyn ToSql + Sync); 5] = [
            &facts.add.as_str(),
            &facts.sub.as_str(),
            &facts.mul.as_str(),
            &facts.div.as_str(),
            &uname,
        ];

        match opt {
            Some(_row) => {
                t.execute(
                    "UPDATE facts SET
                        add = $1, sub = $2, mul = $3, div = $4
                        WHERE uname = $5",
                    &params,
                )
                .await?;
            }
            None => {
                t.execute(
                    "INSERT INTO facts (add, sub, mul, div, uname)
                    VALUES ($1, $2, $3, $4, $5)",
                    &params,
                )
                .await?;
            }
        }

        Ok(())
    }

    pub async fn set_social(
        t: &Transaction<'_>,
        uname: &str,
        term: Term,
        traits: &HashMap<String, String>,
    ) -> Result<(), DbError> {
        log::trace!(
            "Store::set_social( [ &T ], {:?}, {:?}, [ &HashMap ] called. data:\n{:?}",
            uname,
            &term,
            traits
        );

        t.execute(
            "DELETE FROM social
                WHERE uname = $1 AND term = $2",
            &[&uname, &term.as_str()],
        )
        .await?;

        let insert_stmt = t
            .prepare_typed(
                "INSERT INTO social (uname, term, trait, score)
                VALUES ($1, $2, $3, $4)",
                &[Type::TEXT, Type::TEXT, Type::TEXT, Type::TEXT],
            )
            .await?;

        let term = term.as_str();

        {
            let params: Vec<[&(dyn ToSql + Sync); 4]> = traits
                .iter()
                .map(|(k, v)| {
                    let p: [&(dyn ToSql + Sync); 4] = [&uname, &term, k, v];
                    p
                })
                .collect();

            let mut inserts = FuturesUnordered::new();
            for param in params.iter() {
                inserts.push(t.execute(&insert_stmt, param));
            }

            while let Some(res) = inserts.next().await {
                if let Err(e) = res {
                    let estr = format!(
                        "Error writing social/emotional/behavioral goal to DB: {}",
                        &e
                    );
                    return Err(DbError(estr));
                }
            }
        }

        Ok(())
    }

    pub async fn get_social(
        t: &Transaction<'_>,
        uname: &str,
        term: Term,
    ) -> Result<HashMap<String, String>, DbError> {
        log::trace!(
            "Store::get_social( [ &T ], {:?}, {:?} ) called.",
            uname,
            &term
        );

        let rows = t
            .query(
                "SELECT trait, score FROM social
                WHERE uname = $1 AND term = $2",
                &[&uname, &term.as_str()],
            )
            .await?;

        let mut map: HashMap<String, String> = HashMap::with_capacity(rows.len());
        for row in rows.iter() {
            let thing: String = row.try_get("trait")?;
            let score: String = row.try_get("score")?;
            map.insert(thing, score);
        }

        Ok(map)
    }

    pub async fn set_completion<S>(
        t: &Transaction<'_>,
        uname: &str,
        year: i32,
        term: Term,
        courses: &[S],
    ) -> Result<(), DbError>
    where S: AsRef<str> + ToSql + Debug + Sync
    {
        log::trace!(
            "Store::set_completion( [ &T ], {:?}, {:?}, {} {:?} ) called.",
            uname,
            &term,
            year,
            courses,
        );

        t.execute(
            "DELETE FROM completion
                WHERE uname = $1 AND term = $2 AND year = $3",
            &[&uname, &term.as_str(), &year]
        ).await.map_err(|e| format!(
            "error clearing old completion values: {}", &e
        ))?;

        let insert_statement = t.prepare_typed(
            "INSERT INTO completion (uname, term, courses, year)
            VALUES ($1, $2, $3, $4)",
            &[Type::TEXT, Type::TEXT, Type::TEXT, Type::INT4]
        ).await.map_err(|e| format!(
            "error preparing completion insertion statement: {}", &e
        ))?;

        for crs in courses.iter() {
            t.execute(
                &insert_statement,
                &[&uname, &term.as_str(), &crs, &year]
            ).await.map_err(|e| format!(
                "error inserting completed course {:?}: {}", crs, &e
            ))?;
        }

        Ok(())
    }

    pub async fn add_completion(
        t: &Transaction<'_>,
        uname: &str,
        year: i32,
        term: Term,
        course: &str
    ) -> Result<(), DbError>
    {
        log::trace!(
            "Store::add_completion( {:?}, {:?}, {:?}, {:?} ) called.",
            uname, &year, &term, course
        );

        t.execute(
            "INSERT INTO completion (uname, year, term, courses)
            VALUES ($1, $2, $3, $4)",
            &[&uname, &year, &term.as_str(), &course]
        ).await.map_err(|e| format!(
            "error inserting course {:?} for uname for term {:?} {}-{}: {}",
            course, &term, year, year+1, &e
        ))?;

        Ok(())
    }

    pub async fn delete_completion(
        t: &Transaction<'_>,
        uname: &str,
        course: &str
    ) -> Result<(), DbError> {
        log::trace!(
            "Store::delete_completion( {:?}, {:?} ) called.",
            uname, course
        );

        t.execute(
            "DELETE FROM completion WHERE uname = $1 AND courses = $2",
            &[&uname, &course]
        ).await.map_err(|e| format!(
            "error deleting course {:?} for {:?}: {}",
            course, uname, &e
        ))?;

        Ok(())
    }

    pub async fn get_completion(
        t: &Transaction<'_>,
        uname: &str,
        year: i32,
        term: Term,
    ) -> Result<Vec<String>, DbError> {
        log::trace!(
            "Store::get_completion( [ &T ], {:?}, {:?} ) called.",
            uname,
            &term
        );

        let rows = t.query(
            "SELECT courses FROM completion
            WHERE uname = $1 AND term = $2 AND year = $3",
            &[&uname, &term.as_str(), &year]
        ).await?;

        let mut courses: Vec<String> = Vec::with_capacity(rows.len());

        for row in rows.iter() {
            let sym: String = row.try_get("courses")?;
            courses.push(sym);
        }

        Ok(courses)
    }

    pub async fn get_completion_history(
        &self,
        uname: &str
    ) -> Result<Vec<HistEntry>, DbError> {
        log::trace!(
            "Store::get_completion_history( {:?} ) called.", uname
        );

        let client = self.connect().await?;
        let rows = client.query(
            "SELECT year, term, courses FROM completion
                WHERE uname = $1",
            &[&uname]
        ).await?;

        let mut hists: Vec<HistEntry> = Vec::with_capacity(rows.len());
        for row in rows.iter() {
            let term_str: &str = row.try_get("term")?;
            let term = Term::from_str(term_str)?;
            let hist = HistEntry {
                sym: row.try_get("courses")?,
                year: row.try_get("year")?,
                term,
            };

            hists.push(hist);
        }
        hists.sort();

        Ok(hists)
    }

    pub async fn get_completion_histories_by_teacher(
        &self,
        tuname: &str
    ) -> Result<HashMap<String, Vec<HistEntry>>, DbError> {
        log::trace!(
            "Store::get_completion_histories_by_teacher( {:?} ) called.", tuname
        );

        let client = self.connect().await?;
        let rows = client.query(
            "SELECT completion.uname, completion.term,
                    completion.year, completion.courses
                FROM completion INNER JOIN students
                    ON completion.uname = students.uname
                WHERE students.teacher = $1",
            &[&tuname]
        ).await?;

        let mut map: HashMap<String, Vec<HistEntry>> = HashMap::new();
        for row in rows.iter() {
            let uname: String = row.try_get("uname")?;
            let term_str: &str = row.try_get("term")?;
            let term = Term::from_str(term_str)?;
            let hist = HistEntry {
                sym: row.try_get("courses")?,
                year: row.try_get("year")?,
                term,
            };

            map.entry(uname).or_default().push(hist);
        }

        for (_, hist) in map.iter_mut() {
            hist.sort();
        }

        Ok(map)
    }

    pub async fn get_all_completion_histories(&self)
    -> Result<HashMap<String, Vec<HistEntry>>, DbError> {
        log::trace!("Store::get_all_completion_histories() called.");

        let client = self.connect().await?;
        let rows = client.query(
            "SELECT uname, year, term, courses FROM completion",
            &[]
        ).await?;

        let mut map: HashMap<String, Vec<HistEntry>> = HashMap::new();
        for row in rows.iter() {
            let uname: String = row.try_get("uname")?;
            let term_str: &str = row.try_get("term")?;
            let term = Term::from_str(term_str)?;
            let hist = HistEntry {
                sym: row.try_get("courses")?,
                year: row.try_get("year")?,
                term,
            };

            map.entry(uname).or_default().push(hist);
        }

        for (_, hist) in map.iter_mut() {
            hist.sort();
        }

        Ok(map)
    }

    pub async fn set_report_sidecar(
            &self,
            sidecar: &ReportSidecar,
            year: i32
        ) -> Result<(), DbError> {
        log::trace!("Store::set_report_sidecar( {:?} ) called.", &sidecar.uname);

        let uname = &sidecar.uname;

        let mut client = self.connect().await?;
        let t = client.transaction().await?;

        let fact_set = match &sidecar.facts {
            Some(fs) => *fs,
            None => FactSet::default(),
        };

        if let Err(e) = tokio::try_join!(
            Store::set_facts(&t, uname, &fact_set),
            Store::set_social(&t, uname, Term::Fall, &sidecar.fall_social),
            Store::set_social(&t, uname, Term::Spring, &sidecar.spring_social),
            Store::set_completion(&t, uname, year, Term::Fall, &sidecar.fall_complete),
            Store::set_completion(&t, uname, year, Term::Spring, &sidecar.spring_complete),
            Store::set_completion(&t, uname, year, Term::Summer, &sidecar.summer_complete),
            Store::set_mastery(&t, &sidecar.mastery),
        ) {
            return Err(format!("Unable to write sidecar data to database: {}", &e).into());
        }

        t.commit().await.map_err(|e| e.into())
    }

    pub async fn get_report_sidecar(
            &self,
            uname: &str,
            year: i32
        ) -> Result<ReportSidecar, DbError> {
        log::trace!("Store::get_report_sidecar( {:?} ) called.", uname);

        let mut client = self.connect().await?;
        let t = client.transaction().await?;

        let (
            facts,
            fall_social,
            spring_social,
            fall_complete,
            spring_complete,
            summer_complete,
            mastery,
        ) = tokio::try_join!(
            Store::get_facts(&t, uname),
            Store::get_social(&t, uname, Term::Fall),
            Store::get_social(&t, uname, Term::Spring),
            Store::get_completion(&t, uname, year, Term::Fall),
            Store::get_completion(&t, uname, year, Term::Spring),
            Store::get_completion(&t, uname, year, Term::Summer),
            Store::get_mastery(&t, uname),
        )?;

        t.commit().await?;

        let car = ReportSidecar {
            uname: uname.to_string(),
            facts: Some(facts),
            fall_social,
            spring_social,
            mastery,
            fall_complete,
            spring_complete,
            summer_complete,
        };

        Ok(car)
    }

    pub async fn set_draft(
        t: &Transaction<'_>,
        uname: &str,
        term: Term,
        text: &str,
    ) -> Result<(), DbError> {
        log::trace!(
            "Store::set_draft( [ &T ], {:?}, {:?}, [ {} bytes of text ] ) called.",
            uname,
            &term,
            text.len()
        );

        let params: [&(dyn ToSql + Sync); 3] = [&uname, &term.as_str(), &text];

        try_join!(
            t.execute(
                "DELETE FROM drafts WHERE uname = $1 AND term = $2",
                &params[..2]
            ),
            t.execute(
                "INSERT INTO drafts (uname, term, draft)
                    VALUES ($1, $2, $3)",
                &params[..]
            ),
        )
        .map_err(|e| format!("Unable to clear old or set new draft text: {}", &e))?;

        Ok(())
    }

    pub async fn get_draft(
        t: &Transaction<'_>,
        uname: &str,
        term: Term,
    ) -> Result<Option<String>, DbError> {
        log::trace!(
            "Store::get_draft( [ &T ], {:?}, {:?} ) called.",
            uname,
            &term
        );

        let opt = match t
            .query_opt(
                "SELECT draft FROM drafts
                WHERE uname = $1 AND term = $2",
                &[&uname, &term.as_str()],
            )
            .await?
        {
            Some(row) => {
                let text: Option<&str> = row.try_get("draft")?;
                blank_string_means_none(text).map(|text| text.to_owned())
            }
            None => None,
        };

        Ok(opt)
    }

    pub async fn set_final(
        t: &Transaction<'_>,
        uname: &str,
        term: Term,
        pdf_bytes: &[u8],
    ) -> Result<(), DbError> {
        log::trace!(
            "Store::set_final( [ &T ], {:?}, {:?}, [ {} bytes of pdf ] ) called.",
            uname,
            &term,
            pdf_bytes.len()
        );

        let insert_stmt = t
            .prepare_typed(
                "INSERT INTO reports (uname, term, doc)
                    VALUES ($1, $2, $3)",
                &[Type::TEXT, Type::TEXT, Type::BYTEA],
            )
            .await?;
        let params: [&(dyn ToSql + Sync); 3] = [&uname, &term.as_str(), &pdf_bytes];

        t.execute(
            "DELETE FROM reports WHERE uname = $1 AND term = $2",
            &params[..2]
        ).await?;
        t.execute(&insert_stmt, &params[..]).await?;

        Ok(())
    }

    pub async fn get_final(
        t: &Transaction<'_>,
        uname: &str,
        term: Term,
    ) -> Result<Option<Vec<u8>>, DbError> {
        log::trace!(
            "Store::get_final( [ &T ], {:?}, {:?} ) called.",
            uname,
            &term.as_str()
        );

        let opt = match t
            .query_opt(
                "SELECT doc FROM reports WHERE uname = $1 AND term = $2",
                &[&uname, &term.as_str()],
            )
            .await?
        {
            Some(row) => {
                let bytes: Option<Vec<u8>> = row.try_get("doc")?;
                match bytes {
                    Some(bytez) => {
                        if bytez.is_empty() {
                            None
                        } else {
                            Some(bytez)
                        }
                    }
                    None => None,
                }
            }
            None => None,
        };

        Ok(opt)
    }

    pub async fn clear_final(
        &self,
        uname: &str,
        term: Term,
    ) -> Result<(), DbError> {
        log::trace!(
            "Store::clear_final( {:?}, {:?} ) called.",
            uname, &term.as_str()
        );

        let client = self.connect().await?;
        client.execute(
            "DELETE FROM reports WHERE uname = $1 AND term = $2",
            &[&uname, &term.as_str()],
        ).await?;

        Ok(())
    }

    /**
    Clear all sidecar student data for the year.

    Leaves completion data intact, because that's important to keep.
    */
    pub async fn yearly_clear_sidecars(t: &Transaction<'_>) -> Result<(), DbError> {
        log::trace!("Store::yearly_clear_sidecars( [ T ] ) called.");

        let _ = tokio::try_join!(
            t.execute("DELETE FROM nmr", &[]),
            t.execute("DELETE FROM facts", &[]),
            t.execute("DELETE FROM social", &[]),
            t.execute("DELETE FROM drafts", &[]),
            t.execute("DELETE FROM reports", &[]),
        )?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use serial_test::serial;

    use super::*;

    use crate::tests::ensure_logging;
    use crate::UnifiedError;

    static FAKEPROD: &str =
        "host=localhost user=camp_test password='camp_test' dbname=camp_store_fakeprod";
    static UNAME: &str = "zmilk";
    static SOCIAL_CATS: &[&str] = &[
        "Class Participation",
        "Leadership",
        "Time Management",
        "Behavior",
        "Social Skills",
        "Attention to Detail",
        "Organization",
        "Study Skills",
    ];

    fn social_map() -> HashMap<String, String> {
        SOCIAL_CATS
            .iter()
            .map(|cat| (String::from(*cat), format!("2")))
            .collect()
    }

    #[tokio::test]
    #[serial]
    async fn read_sidecar() -> Result<(), UnifiedError> {
        ensure_logging();

        let db = Store::new(FAKEPROD.to_owned());
        db.ensure_db_schema().await?;

        let sc = db.get_report_sidecar(UNAME).await?;

        log::info!("Debug:\n{:#?}", &sc);
        let sc_str: String =
            serde_json::to_string_pretty(&sc).map_err(|e| format!("Error JSONizing: {}", &e))?;
        log::info!("JSON (serde):\n{}", &sc_str);

        Ok(())
    }

    #[tokio::test]
    #[serial]
    async fn write_sidecar() -> Result<(), UnifiedError> {
        ensure_logging();

        let db = Store::new(FAKEPROD.to_owned());
        db.ensure_db_schema().await?;

        let facts = FactSet {
            add: FactStatus::Mastered,
            sub: FactStatus::Mastered,
            mul: FactStatus::Mastered,
            div: FactStatus::Not,
        };

        let mastery = vec![
            Mastery {
                id: 1,
                status: MasteryStatus::Retained,
            },
            Mastery {
                id: 2,
                status: MasteryStatus::Not,
            },
            Mastery {
                id: 3,
                status: MasteryStatus::Mastered,
            },
            Mastery {
                id: 4,
                status: MasteryStatus::Retained,
            },
        ];

        let sc = ReportSidecar {
            uname: UNAME.to_owned(),
            facts: Some(facts),
            fall_social: social_map(),
            spring_social: social_map(),
            fall_complete: "None".to_owned(),
            spring_complete: "".to_owned(),
            summer_complete: None,
            mastery,
        };

        db.set_report_sidecar(&sc).await?;

        Ok(())
    }
}

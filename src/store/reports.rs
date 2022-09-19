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
    courses TEXT
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
use std::collections::HashMap;

use futures::{
    stream::{FuturesUnordered, StreamExt},
    try_join,
};
use tokio_postgres::{
    Row,
    Transaction,
    types::{ToSql, Type},
};

use super::{DbError, Store};
use crate::{
    blank_string_means_none,
    pace::Term,
    report::*,
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
    pub async fn set_mastery(
        t: &Transaction<'_>,
        stati: &[Mastery]
    ) -> Result<usize, DbError> {
        log::trace!("Store::set_mastery( [ &T ], {:?} ) called.", &stati);

        let update_statement = t.prepare_typed(
            "INSERT INTO nmr (id, status)
                VALUES ($1, $2)\
                ON CONFLICT ON CONSTRAINT nmr_pkey
                DO UPDATE SET status = $2",
                &[Type::INT8, Type::TEXT]
        ).await?;

        let mastery_strs: Vec<Option<&str>> = stati.iter().map(|m| m.status.as_sql()).collect();

        let mut n_set: usize = 0;
        {
            let data_refs: Vec<[&(dyn ToSql + Sync); 2]> = stati.iter().enumerate()
                .map(|(n, m)| {
                    let p: [&(dyn ToSql + Sync); 2] =
                        [&m.id, &mastery_strs[n]];
                    p
                }).collect();
            
            let mut inserts = FuturesUnordered::new();
            for params in data_refs.iter() {
                inserts.push(
                    t.execute(&update_statement, &params[..])
                );
            }

            while let Some(res) = inserts.next().await {
                match res {
                    Ok(_) => { n_set += 1; },
                    Err(e) => {
                        let estr = format!("Error updating Goal mastery status: {}", &e);
                        return Err(DbError(estr));
                    },
                }
            }
        }

        Ok(n_set)
    }

    pub async fn get_mastery(
        t: &Transaction<'_>,
        uname: &str
    ) -> Result<Vec<Mastery>, DbError> {
        log::trace!("Store::get_mastery( [ &T ], {:?} ) called.", uname);

        let rows = t.query(
            "SELECT id, status FROM nmr
                INNER JOIN goals ON nmr.id = goals.id
            WHERE goals.uname = $1",
            &[&uname]
        ).await?;

        let mut masteries: Vec<Mastery> = Vec::with_capacity(rows.len());
        for row in rows.iter() {
            let m = row2mastery(&row).map_err(|e|
                e.annotate("Error reading Mastery from DB row"))?;
            masteries.push(m)
        }

        Ok(masteries)
    }

    pub async fn get_facts(
        t: &Transaction<'_>,
        uname: &str,
    ) -> Result<FactSet, DbError> {
        log::trace!("Store::get_facts( [ &T ], {:?} ) called.", uname);

        let opt = t.query_opt(
            "SELECT add, sub, mul, div FROM facts
                WHERE uname = $1",
            &[&uname]
        ).await?;

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
            },
            None => {
                Ok(FactSet::default())
            }
        }
    }

    pub async fn set_social(
        t: &Transaction<'_>,
        uname: &str,
        term: Term,
        traits: &HashMap<String, String>
    ) -> Result<(), DbError> {
        log::trace!(
            "Store::set_social( [ &T ], {:?}, {:?}, [ &HashMap ] called. data:\n{:?}",
            uname, &term, traits
        );

        t.execute(
            "DELETE FROM social
                WHERE uname = $1 AND term = $2",
            &[&uname, &term.as_str()]
        ).await?;

        let insert_stmt = t.prepare_typed(
            "INSERT INTO social (uname, term, trait, score)
                VALUES ($1, $2, $3, $4)",
            &[Type::TEXT, Type::TEXT, Type::TEXT, Type::TEXT]
        ).await?;

        let term = term.as_str();

        {
            let params: Vec<[&(dyn ToSql + Sync); 4]> = traits.iter()
                .map(|(k, v)| {
                    let p: [&(dyn ToSql + Sync); 4] =
                        [&uname, &term, k, v];
                    p
                }).collect();

            let mut inserts = FuturesUnordered::new();
            for param in params.iter() {
                inserts.push(t.execute(&insert_stmt, param));
            }

            while let Some(res) = inserts.next().await {
                if let Err(e) = res {
                    let estr = format!(
                        "Error writing social/emotional/behavioral goal to DB: {}", &e
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
        term: Term
    ) -> Result<HashMap<String, String>, DbError> {
        log::trace!("Store::get_social( [ &T ], {:?}, {:?} ) called.", uname, &term);

        let rows = t.query(
            "SELECT (trait, score) FROM social
                WHERE uname = $1 AND term = $2",
            &[&uname, &term.as_str()]
        ).await?;

        let mut map: HashMap<String, String> = HashMap::with_capacity(rows.len());
        for row in rows.iter() {
            let thing: String = row.try_get("trait")?;
            let score: String = row.try_get("score")?;
            map.insert(thing, score);
        };

        Ok(map)
    }

    pub async fn set_completion(
        t: &Transaction<'_>,
        uname: &str,
        term: Term,
        courses: &str
    ) -> Result<(), DbError> {
        log::trace!(
            "Store::set_completion( [ &T ], {:?}, {:?}, {:?} ) called.",
            uname, &term, courses
        );

        let params: [&(dyn ToSql + Sync); 3] = [&uname, &term.as_str(), &courses];

        try_join!(
            t.execute(
                "DELETE FROM completion WHERE uname = $1 AND term = $2",
                &params[..2]
            ),
            t.execute(
                "INSERT INTO completion (uname, term, courses)
                    VALUES ($1, $2, $3)",
                &params[..]
            )
        ).map_err(|e| format!(
            "Unable to clear old or set new completion value: {}", &e
        ))?;

        Ok(())
    }

    pub async fn get_completion(
        t: &Transaction<'_>,
        uname: &str,
        term: Term,
    ) -> Result<Option<String>, DbError> {
        log::trace!(
            "Store::get_completion( [ &T ], {:?}, {:?} ) called.",
            uname, &term
        );

        let opt = match t.query_opt(
            "SELECT courses FROM completion
                WHERE uname = $1 AND term = $2",
            &[&uname, &term.as_str()]
        ).await? {
            Some(row) => {
                let courses: Option<&str> = row.try_get("courses")?;
                match blank_string_means_none(courses) {
                    Some(cstr) => Some(cstr.to_owned()),
                    None => None,
                }
            },
            None => None
        };

        Ok(opt)
    }

    pub async fn set_draft(
        t: &Transaction<'_>,
        uname: &str,
        term: Term,
        text: &str
    ) -> Result<(), DbError> {
        log::trace!(
            "Store::set_draft( [ &T ], {:?}, {:?}, [ {} bytes of text ] ) called.",
            uname, &term, text.len()
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
        ).map_err(|e| format!(
            "Unable to clear old or set new draft text: {}", &e
        ))?;

        Ok(())
    }

    pub async fn get_draft(
        t: &Transaction<'_>,
        uname: &str,
        term: Term
    ) -> Result<Option<String>, DbError> {
        log::trace!(
            "Store::get_draft( [ &T ], {:?}, {:?} ) called.",
            uname, &term
        );

        let opt = match t.query_opt(
            "SELECT draft FROM drafts
                WHERE uname = $1 AND term = $2",
            &[&uname, &term.as_str()]
        ).await? {
            Some(row) => {
                let text: Option<&str> = row.try_get("draft")?;
                match blank_string_means_none(text) {
                    Some(text) => Some(text.to_owned()),
                    None => None,
                }
            },
            None => None,
        };

        Ok(opt)
    }

    pub async fn set_final(
        t: &Transaction<'_>,
        uname: &str,
        term: Term,
        pdf_bytes: &[u8]
    ) -> Result<(), DbError> {
        log::trace!(
            "Store::set_final( [ &T ], {:?}, {:?}, [ {} bytes of pdf ] ) called.",
            uname, &term, pdf_bytes.len()
        );

        let insert_stmt = t.prepare_typed(
            "INSERT INTO reports (uname, term, doc)
                    VALUES ($1, $2, $3)",
            &[Type::TEXT, Type::TEXT, Type::BYTEA]
        ).await?;
        let params: [&(dyn ToSql + Sync); 3] = [&uname, &term.as_str(), &pdf_bytes];

        try_join!(
            t.execute(
                "DELETE FROM reports WHERE uname = $1 AND term = $2",
                &params[..2]
            ),
            t.execute(&insert_stmt, &params[..]),
        ).map_err(|e| format!(
            "Unable to clear old report or set new one: {}", &e
        ))?;

        Ok(())
    }

    pub async fn get_final(
        t: &Transaction<'_>,
        uname: &str,
        term: Term
    ) -> Result<Option<Vec<u8>>, DbError> {
        log::trace!(
            "Store::get_final( [ &T ], {:?}, {:?} ) called.",
            uname, &term.as_str()
        );

        let opt = match t.query_opt(
            "SELECT doc FROM reports WHERE uname = $1 AND term = $2",
            &[&uname, &term.as_str()]
        ).await? {
            Some(row) => {
                let bytes: Option<Vec<u8>> = row.try_get("doc")?;
                match bytes {
                    Some(bytez) => {
                        if bytez.is_empty() {
                            None
                        } else {
                            Some(bytez)
                        }
                    },
                    None => None,
                }
            },
            None => None,
        };

        Ok(opt)
    }
}
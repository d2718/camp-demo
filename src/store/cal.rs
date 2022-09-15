/*!
Calendar-oriented methods.

Dates are represented by the `time::Date` struct.

```sql
CREATE TABLE calendar (
    day DATE UNIQUE NOT NULL
);
```

```sql
CREATE TABLE dates (
    name TEXT PRIMARY KEY,
    day DATE NOT NULL
);
```
*/
use std::collections::HashMap;

use futures::stream::{FuturesUnordered, StreamExt};
use time::Date;
use tokio_postgres::types::{ToSql, Type};

use super::{DbError, Store};

impl Store {
    /// Store this collection of dates as making up the "working days" of the
    /// current academic year.
    pub async fn set_calendar(&self, dates: &[Date]) -> Result<(usize, usize), DbError> {
        log::trace!("Store::insert_dates( {:?} ) called.", &dates);

        let mut client = self.connect().await?;
        let t = client.transaction().await?;

        let insert_statement = t
            .prepare_typed("INSERT INTO calendar (day) VALUES ($1)", &[Type::DATE])
            .await?;

        let n_deleted = t
            .execute("DELETE FROM calendar", &[])
            .await
            .map_err(|e| format!("Unable to clear old calendar: {}", &e))?;

        let mut n_inserted: u64 = 0;
        {
            let date_refs: Vec<[&(dyn ToSql + Sync); 1]> = dates
                .iter()
                .map(|d| {
                    let p: [&(dyn ToSql + Sync); 1] = [d];
                    p
                })
                .collect();

            let mut inserts = FuturesUnordered::new();
            for params in date_refs.iter() {
                inserts.push(t.execute(&insert_statement, &params[..]));
            }

            while let Some(res) = inserts.next().await {
                match res {
                    Ok(_) => {
                        n_inserted += 1;
                    }
                    Err(e) => {
                        let estr = format!("Error inserting date into calendar: {}", &e);
                        return Err(DbError(estr));
                    }
                }
            }
        }

        t.commit().await?;
        Ok((n_deleted as usize, n_inserted as usize))
    }

    /// Retrieve the collection of "working dates" from the current academic
    /// year as stored. They should be in chronological order.
    pub async fn get_calendar(&self) -> Result<Vec<Date>, DbError> {
        log::trace!("Store::get_calendar() called.");

        let client = self.connect().await?;
        let rows = client
            .query("SELECT day FROM calendar ORDER BY day", &[])
            .await
            .map_err(|e| format!("Error fetching calendar from Data DB: {}", &e))?;

        let mut dates: Vec<Date> = Vec::with_capacity(rows.len());
        for row in rows.iter() {
            let d: Date = row.try_get("day")?;
            dates.push(d);
        }

        Ok(dates)
    }

    /// Store a "special date".
    ///
    /// So far the only recognized special dates are "fall-end" and "spring-end"
    /// for denoting the ends of each semester.
    pub async fn set_date(&self, name: &str, day: &Date) -> Result<(), DbError> {
        log::trace!("Store::set_date( {:?}, {} ) called.", name, &day);

        let client = self.connect().await?;
        client
            .execute(
                "INSERT INTO dates (name, day)
                VALUES ($1, $2)
                ON CONFLICT ON CONSTRAINT dates_pkey
                DO UPDATE set day = $2",
                &[&name, &day],
            )
            .await
            .map_err(|e| {
                format!(
                    "Error inserting date {:?} ({}) into database: {}",
                    &name, &day, &e
                )
            })?;

        Ok(())
    }

    /// Delete a "special date" from the store.
    pub async fn delete_date(&self, name: &str) -> Result<(), DbError> {
        log::trace!("Store::delete_date( {:?} ) called.", &name);

        let client = self.connect().await?;
        let n_deleted = client
            .execute("DELETE FROM dates WHERE name = $1", &[&name])
            .await
            .map_err(|e| {
                log::error!("Error deleting date {:?} from database: {}", name, &e);
                format!("Unable to delete date from database: {}", &e)
            })?;

        match n_deleted {
            0 => Err(DbError(format!("No date with name {:?}.", name))),
            1 => Ok(()),
            n => {
                log::warn!(
                    "Deleting date {:?} deleted {} records, which shouldn't happen.",
                    name,
                    n
                );
                Ok(())
            }
        }
    }

    /// Retrieve all "special dates" as stored.
    pub async fn get_dates(&self) -> Result<HashMap<String, Date>, DbError> {
        log::trace!("Store::get_dates() called.");

        let client = self.connect().await?;
        let rows = client
            .query("SELECT name, day FROM dates", &[])
            .await
            .map_err(|e| format!("Error querying database for dates: {}", &e))?;

        let mut map: HashMap<String, Date> = HashMap::with_capacity(rows.len());
        for row in rows.iter() {
            let name: String = row.try_get("name").map_err(|e| {
                log::error!("Error getting 'name' from row {:?}: {}", &row, &e);
                "Error retrieving date name from data DB.".to_string()
            })?;
            let date: Date = row.try_get("day").map_err(|e| {
                log::error!("Error getting 'day' from row {:?}: {}", &row, &e);
                "Error retrieving date from data DB.".to_string()
            })?;

            map.insert(name, date);
        }

        Ok(map)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use time::{macros::format_description, Month};

    #[test]
    fn date_format() {
        let d = Date::from_calendar_date(2022, Month::August, 11).unwrap();

        println!("Display: \"{}\"", &d);

        let dfmtr = format_description!("[year]-[month]-[day]");
        let hween = Date::parse("2021-10-31", &dfmtr).unwrap();
        println!("{:?}, {}", &hween, &hween);
    }
}

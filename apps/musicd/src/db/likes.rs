use std::collections::{HashMap, HashSet};
use std::io;

use rusqlite::params;

use super::{Database, db_error};

impl Database {
    pub(crate) fn add_item_like(
        &self,
        item_kind: &str,
        item_id: &str,
        client_id: &str,
        liked_unix: i64,
    ) -> io::Result<bool> {
        let connection = self.connection()?;
        let changed = connection
            .execute(
                "INSERT OR IGNORE INTO item_likes
                 (item_kind, item_id, client_id, liked_unix)
                 VALUES (?1, ?2, ?3, ?4)",
                params![item_kind, item_id, client_id, liked_unix],
            )
            .map_err(db_error)?;
        Ok(changed > 0)
    }

    pub(crate) fn count_item_likes(&self, item_kind: &str, item_id: &str) -> io::Result<u64> {
        let connection = self.connection()?;
        let count = connection
            .query_row(
                "SELECT COUNT(*)
                 FROM item_likes
                 WHERE item_kind = ?1 AND item_id = ?2",
                params![item_kind, item_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(db_error)?;
        Ok(count.max(0) as u64)
    }

    pub(crate) fn item_like_counts(&self, item_kind: &str) -> io::Result<HashMap<String, u64>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT item_id, COUNT(*)
                 FROM item_likes
                 WHERE item_kind = ?1
                 GROUP BY item_id",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map([item_kind], |row| {
                let count = row.get::<_, i64>(1)?.max(0) as u64;
                Ok((row.get::<_, String>(0)?, count))
            })
            .map_err(db_error)?;

        let mut counts = HashMap::new();
        for row in rows {
            let (item_id, count) = row.map_err(db_error)?;
            counts.insert(item_id, count);
        }
        Ok(counts)
    }

    pub(crate) fn client_liked_item_ids(
        &self,
        item_kind: &str,
        client_id: Option<&str>,
    ) -> io::Result<HashSet<String>> {
        let Some(client_id) = client_id else {
            return Ok(HashSet::new());
        };
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT item_id
                 FROM item_likes
                 WHERE item_kind = ?1 AND client_id = ?2",
            )
            .map_err(db_error)?;
        let rows = statement
            .query_map(params![item_kind, client_id], |row| row.get::<_, String>(0))
            .map_err(db_error)?;

        let mut item_ids = HashSet::new();
        for row in rows {
            item_ids.insert(row.map_err(db_error)?);
        }
        Ok(item_ids)
    }
}

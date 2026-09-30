use std::io;

use rusqlite::{OptionalExtension, Row, params};

use super::{Database, db_error};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UserRecord {
    pub(crate) id: i64,
    pub(crate) username: String,
    pub(crate) password_hash: String,
    pub(crate) must_change_password: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WebSessionRecord {
    pub(crate) user_id: i64,
    pub(crate) username: String,
    pub(crate) must_change_password: bool,
    pub(crate) expires_unix: i64,
    pub(crate) last_seen_unix: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ApiTokenRecord {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) scope: String,
    pub(crate) created_unix: i64,
    pub(crate) last_used_unix: Option<i64>,
    pub(crate) revoked_unix: Option<i64>,
}

const API_TOKEN_COLUMNS: &str = "id, name, scope, created_unix, last_used_unix, revoked_unix";

fn api_token_from_row(row: &Row<'_>) -> rusqlite::Result<ApiTokenRecord> {
    Ok(ApiTokenRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        scope: row.get(2)?,
        created_unix: row.get(3)?,
        last_used_unix: row.get(4)?,
        revoked_unix: row.get(5)?,
    })
}

impl Database {
    /// Returns the stored value for `key`, first storing `candidate` if the
    /// key is unset. Used for server secrets that must survive restarts.
    pub(crate) fn app_state_value_or_insert(
        &self,
        key: &str,
        candidate: &str,
    ) -> io::Result<String> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT OR IGNORE INTO app_state (key, value) VALUES (?1, ?2)",
                params![key, candidate],
            )
            .map_err(db_error)?;
        connection
            .query_row("SELECT value FROM app_state WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .map_err(db_error)
    }

    pub(crate) fn count_users(&self) -> io::Result<i64> {
        let connection = self.connection()?;
        connection
            .query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))
            .map_err(db_error)
    }

    pub(crate) fn insert_user(
        &self,
        username: &str,
        password_hash: &str,
        must_change_password: bool,
        now_unix: i64,
    ) -> io::Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO users
                 (username, password_hash, must_change_password, created_unix, updated_unix)
                 VALUES (?1, ?2, ?3, ?4, ?4)",
                params![username, password_hash, must_change_password, now_unix],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn load_user_by_username(&self, username: &str) -> io::Result<Option<UserRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT id, username, password_hash, must_change_password
                 FROM users
                 WHERE username = ?1",
                [username],
                |row| {
                    Ok(UserRecord {
                        id: row.get(0)?,
                        username: row.get(1)?,
                        password_hash: row.get(2)?,
                        must_change_password: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(db_error)
    }

    pub(crate) fn load_user(&self, user_id: i64) -> io::Result<Option<UserRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT id, username, password_hash, must_change_password
                 FROM users
                 WHERE id = ?1",
                [user_id],
                |row| {
                    Ok(UserRecord {
                        id: row.get(0)?,
                        username: row.get(1)?,
                        password_hash: row.get(2)?,
                        must_change_password: row.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(db_error)
    }

    /// Stores a new password hash and clears the forced-change flag.
    pub(crate) fn update_user_password(
        &self,
        user_id: i64,
        password_hash: &str,
        now_unix: i64,
    ) -> io::Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "UPDATE users
                 SET password_hash = ?2, must_change_password = 0, updated_unix = ?3
                 WHERE id = ?1",
                params![user_id, password_hash, now_unix],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn insert_web_session(
        &self,
        token_hash: &str,
        user_id: i64,
        now_unix: i64,
        expires_unix: i64,
    ) -> io::Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "DELETE FROM web_sessions WHERE expires_unix <= ?1",
                [now_unix],
            )
            .map_err(db_error)?;
        connection
            .execute(
                "INSERT INTO web_sessions
                 (token_hash, user_id, created_unix, last_seen_unix, expires_unix)
                 VALUES (?1, ?2, ?3, ?3, ?4)",
                params![token_hash, user_id, now_unix, expires_unix],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn load_web_session(
        &self,
        token_hash: &str,
        now_unix: i64,
    ) -> io::Result<Option<WebSessionRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT s.user_id, u.username, u.must_change_password,
                        s.expires_unix, s.last_seen_unix
                 FROM web_sessions s
                 JOIN users u ON u.id = s.user_id
                 WHERE s.token_hash = ?1 AND s.expires_unix > ?2",
                params![token_hash, now_unix],
                |row| {
                    Ok(WebSessionRecord {
                        user_id: row.get(0)?,
                        username: row.get(1)?,
                        must_change_password: row.get(2)?,
                        expires_unix: row.get(3)?,
                        last_seen_unix: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(db_error)
    }

    pub(crate) fn touch_web_session(
        &self,
        token_hash: &str,
        now_unix: i64,
        expires_unix: i64,
    ) -> io::Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "UPDATE web_sessions
                 SET last_seen_unix = ?2, expires_unix = ?3
                 WHERE token_hash = ?1",
                params![token_hash, now_unix, expires_unix],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn delete_web_session(&self, token_hash: &str) -> io::Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "DELETE FROM web_sessions WHERE token_hash = ?1",
                [token_hash],
            )
            .map_err(db_error)?;
        Ok(())
    }

    /// Signs the user out everywhere except the session identified by `keep_token_hash`.
    pub(crate) fn delete_other_web_sessions(
        &self,
        user_id: i64,
        keep_token_hash: &str,
    ) -> io::Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "DELETE FROM web_sessions WHERE user_id = ?1 AND token_hash != ?2",
                params![user_id, keep_token_hash],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn insert_api_token(
        &self,
        id: &str,
        name: &str,
        token_hash: &str,
        scope: &str,
        now_unix: i64,
    ) -> io::Result<ApiTokenRecord> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO api_tokens (id, name, token_hash, scope, created_unix)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![id, name, token_hash, scope, now_unix],
            )
            .map_err(db_error)?;
        Ok(ApiTokenRecord {
            id: id.to_string(),
            name: name.to_string(),
            scope: scope.to_string(),
            created_unix: now_unix,
            last_used_unix: None,
            revoked_unix: None,
        })
    }

    /// Looks up an unrevoked token by the hash of its secret.
    pub(crate) fn load_active_api_token(
        &self,
        token_hash: &str,
    ) -> io::Result<Option<ApiTokenRecord>> {
        let connection = self.connection()?;
        connection
            .query_row(
                &format!(
                    "SELECT {API_TOKEN_COLUMNS}
                     FROM api_tokens
                     WHERE token_hash = ?1 AND revoked_unix IS NULL"
                ),
                [token_hash],
                api_token_from_row,
            )
            .optional()
            .map_err(db_error)
    }

    pub(crate) fn touch_api_token(&self, id: &str, now_unix: i64) -> io::Result<()> {
        let connection = self.connection()?;
        connection
            .execute(
                "UPDATE api_tokens SET last_used_unix = ?2 WHERE id = ?1",
                params![id, now_unix],
            )
            .map_err(db_error)?;
        Ok(())
    }

    pub(crate) fn list_api_tokens(&self) -> io::Result<Vec<ApiTokenRecord>> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(&format!(
                "SELECT {API_TOKEN_COLUMNS}
                 FROM api_tokens
                 ORDER BY revoked_unix IS NOT NULL, created_unix DESC"
            ))
            .map_err(db_error)?;
        let rows = statement
            .query_map([], api_token_from_row)
            .map_err(db_error)?;

        let mut tokens = Vec::new();
        for row in rows {
            tokens.push(row.map_err(db_error)?);
        }
        Ok(tokens)
    }

    /// Returns false when the token does not exist or was already revoked.
    pub(crate) fn revoke_api_token(&self, id: &str, now_unix: i64) -> io::Result<bool> {
        let connection = self.connection()?;
        let changed = connection
            .execute(
                "UPDATE api_tokens SET revoked_unix = ?2
                 WHERE id = ?1 AND revoked_unix IS NULL",
                params![id, now_unix],
            )
            .map_err(db_error)?;
        Ok(changed > 0)
    }
}

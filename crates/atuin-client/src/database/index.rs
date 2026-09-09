//! Narrow, borrowed history rows for building the daemon's search index.

use std::borrow::Cow;

use atuin_common::db;
use atuin_common::time::OffsetDateTimeExt;
use atuin_domain::record::UNKNOWN_USER;
use easy_cast::Conv;
use futures::TryStreamExt;
use sqlx::sqlite::SqliteRow;
use sqlx::{FromRow, Row};
use time::OffsetDateTime;

use super::Sqlite;
use crate::history::{AuthorKind, History, HistoryId, is_agent_author};

/// Only the history fields the search index consumes. Strings borrow the
/// decoded row or live history entry, except normalization of legacy origins.
#[derive(Debug, PartialEq, Eq)]
pub struct IndexHistoryRef<'a> {
    pub id: HistoryId,
    pub timestamp: OffsetDateTime,
    pub command: &'a str,
    pub cwd: &'a str,
    pub session: &'a str,
    pub cmd_origin: Cow<'a, str>,
    pub author: &'a str,
    pub shell: Option<&'a str>,
    pub author_kind: Option<AuthorKind>,
}

impl IndexHistoryRef<'_> {
    #[must_use]
    pub fn is_agent(&self) -> bool {
        is_agent_author(self.author, self.author_kind, &self.cmd_origin)
    }
}

impl<'a> From<&'a History> for IndexHistoryRef<'a> {
    fn from(history: &'a History) -> Self {
        Self {
            id: history.id,
            timestamp: history.timestamp,
            command: &history.command,
            cwd: &history.cwd,
            session: &history.session,
            cmd_origin: Cow::Borrowed(history.cmd_origin.as_str()),
            author: &history.author,
            shell: history.shell.as_deref(),
            author_kind: history.author_kind,
        }
    }
}

impl<'r> FromRow<'r, SqliteRow> for IndexHistoryRef<'r> {
    fn from_row(row: &'r SqliteRow) -> sqlx::Result<Self> {
        let hostname: &str = row.try_get("hostname")?;
        let author: Option<&str> = row.try_get("author").ok().flatten();
        let author = author
            .filter(|author| !author.trim().is_empty())
            .unwrap_or_else(|| hostname.split_once(':').map_or(hostname, |(_, user)| user));
        let author_kind: Option<i64> = row.try_get("author_kind").ok().flatten();
        let author_kind =
            author_kind.and_then(|kind| u8::try_from(kind).ok()).and_then(AuthorKind::from_repr);
        Ok(Self {
            id: row.try_get("id")?,
            timestamp: OffsetDateTime::from_unix_nanos_i64(row.try_get("timestamp")?),
            command: row.try_get("command")?,
            cwd: row.try_get("cwd")?,
            session: row.try_get("session")?,
            cmd_origin: if hostname.contains(':') {
                Cow::Borrowed(hostname)
            } else {
                Cow::Owned(format!("{hostname}:{UNKNOWN_USER}"))
            },
            author,
            shell: row.try_get("shell").ok().flatten(),
            author_kind,
        })
    }
}

impl Sqlite {
    /// Visit a narrow page of active history, ordered by descending ID.
    ///
    /// Each borrowed row is consumed before polling the next. The callback
    /// cannot retain row references, so SQLx values need not be kept for a whole
    /// page. Returns the last visited ID, or `None` when the scan is exhausted.
    pub async fn visit_index_history_page<F>(
        &self,
        before: Option<HistoryId>,
        page_size: usize,
        mut visit: F,
    ) -> sqlx::Result<Option<HistoryId>>
    where
        F: for<'r> FnMut(IndexHistoryRef<'r>),
    {
        const COLUMNS: &str =
            "id, timestamp, command, cwd, session, hostname, author, shell, author_kind";
        let sql = if before.is_some() {
            format!(
                "SELECT {COLUMNS} FROM history WHERE deleted_at IS NULL AND id < ? ORDER BY id \
                 DESC LIMIT ?"
            )
        } else {
            format!(
                "SELECT {COLUMNS} FROM history WHERE deleted_at IS NULL ORDER BY id DESC LIMIT ?"
            )
        };
        let mut query = db::query(sqlx::AssertSqlSafe(sql));
        if let Some(before) = before {
            query = query.bind(before);
        }
        let mut rows = query.bind(i64::conv(page_size)).fetch(self.sqlite.pool());
        let mut last_id = None;
        while let Some(row) = rows.try_next().await? {
            let history = IndexHistoryRef::from_row(&row)?;
            last_id = Some(history.id);
            visit(history);
        }
        Ok(last_id)
    }
}

#[cfg(test)]
mod tests {
    use rstest::{fixture, rstest};

    use super::*;

    #[fixture]
    async fn database() -> Sqlite {
        Sqlite::in_memory(std::time::Duration::from_secs(5)).await.unwrap()
    }

    #[rstest]
    #[case::human("host:user", Some("user"), None)]
    #[case::agent("host:user", Some("codex"), None)]
    #[case::username_named_agent("host:pi", None, None)]
    #[case::explicit_user("host:user", Some("codex"), Some(1))]
    #[case::explicit_agent("host:user", Some("user"), Some(2))]
    #[case::future_kind("host:user", Some("codex"), Some(999))]
    #[case::legacy_hostname("old-host", None, None)]
    #[case::legacy_agent_hostname("pi", None, None)]
    #[case::blank_author("host:user", Some("  "), None)]
    #[case::unknown_username("codex:unknown-user", Some("codex"), None)]
    #[case::empty_username("host:", None, None)]
    #[tokio::test]
    async fn matches_owned_history(
        #[future(awt)] database: Sqlite,
        #[case] hostname: &str,
        #[case] author: Option<&str>,
        #[case] kind: Option<i64>,
    ) {
        let history: History = History::import()
            .timestamp(OffsetDateTime::UNIX_EPOCH - time::Duration::nanoseconds(1))
            .command("echo café")
            .cwd("/work")
            .shell("bash")
            .build()
            .into();
        database.save(&history).await.unwrap();
        db::query("UPDATE history SET hostname = ?, author = ?, author_kind = ? WHERE id = ?")
            .bind(hostname)
            .bind(author)
            .bind(kind)
            .bind(history.id)
            .execute(database.sqlite.pool())
            .await
            .unwrap();
        let owned = database.load(history.id).await.unwrap().unwrap();
        let mut seen = 0;
        let last = database
            .visit_index_history_page(None, 1, |borrowed| {
                assert_eq!(borrowed, IndexHistoryRef::from(&owned));
                assert_eq!(borrowed.is_agent(), owned.is_agent());
                assert_eq!(matches!(borrowed.cmd_origin, Cow::Borrowed(_)), hostname.contains(':'));
                seen += 1;
            })
            .await
            .unwrap();
        assert_eq!(seen, 1);
        assert_eq!(last, Some(history.id));
        assert_eq!(
            database
                .visit_index_history_page(last, 1, |_| panic!("cursor repeated a row"))
                .await
                .unwrap(),
            None
        );
    }

    #[rstest]
    #[tokio::test]
    async fn text_borrows_the_sqlx_row(#[future(awt)] database: Sqlite) {
        let row = db::query(
            "SELECT '00000000000000000000000000000001' AS id, 0 AS timestamp,             'echo \
             café' AS command, '/work' AS cwd, 'session' AS session,             'host:user' AS \
             hostname, 'user' AS author, 'bash' AS shell, NULL AS author_kind",
        )
        .fetch_one(database.sqlite.pool())
        .await
        .unwrap();
        let borrowed = IndexHistoryRef::from_row(&row).unwrap();
        assert_eq!(borrowed.command.as_ptr(), row.get::<&str, _>("command").as_ptr());
        assert_eq!(borrowed.cwd.as_ptr(), row.get::<&str, _>("cwd").as_ptr());
        assert_eq!(borrowed.author.as_ptr(), row.get::<&str, _>("author").as_ptr());
        assert_eq!(borrowed.cmd_origin.as_ptr(), row.get::<&str, _>("hostname").as_ptr());
    }
}

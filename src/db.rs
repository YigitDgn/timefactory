use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{params, Connection};

pub struct UsageRow {
    pub process_name: String,
    pub seconds: i64,
}

pub struct MinuteRow {
    pub minute: u32,
    pub process_name: String,
    pub seconds: i64,
}

pub struct UsageDatabase {
    conn: Connection,
}

// Same location as the Qt build, so existing data is picked up as-is.
pub fn database_path() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("TimeFactory")
        .join("timefactory.db")
}

impl UsageDatabase {
    pub fn open() -> rusqlite::Result<Self> {
        Self::open_at(&database_path())
    }

    pub fn open_at(path: &Path) -> rusqlite::Result<Self> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }

        let conn = Connection::open(path)?;
        // The tracker thread writes while the UI reads: WAL lets both proceed.
        conn.busy_timeout(Duration::from_secs(2))?;
        conn.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()))?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> rusqlite::Result<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> rusqlite::Result<Self> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS usage_daily (
                date TEXT NOT NULL,
                process_name TEXT NOT NULL,
                seconds INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY(date, process_name)
            );
            CREATE TABLE IF NOT EXISTS usage_minutes (
                date TEXT NOT NULL,
                minute INTEGER NOT NULL,
                process_name TEXT NOT NULL,
                seconds INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY(date, minute, process_name)
            );
            CREATE TABLE IF NOT EXISTS idle_daily (
                date TEXT PRIMARY KEY,
                seconds INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS apps (
                process_name TEXT PRIMARY KEY,
                exe_path TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL
            )",
        )?;
        Ok(Self { conn })
    }

    /// Adds one tracked second for `process_name` on `date` (ISO yyyy-MM-dd)
    /// at `minute` of the day (0..1440).
    pub fn add_second(
        &mut self,
        date: &str,
        minute: u32,
        process_name: &str,
    ) -> rusqlite::Result<()> {
        let tx = self.conn.transaction()?;
        tx.prepare_cached(
            "INSERT INTO usage_daily (date, process_name, seconds) VALUES (?1, ?2, 1)
             ON CONFLICT(date, process_name) DO UPDATE SET seconds = seconds + 1",
        )?
        .execute(params![date, process_name])?;
        tx.prepare_cached(
            "INSERT INTO usage_minutes (date, minute, process_name, seconds) VALUES (?1, ?2, ?3, 1)
             ON CONFLICT(date, minute, process_name) DO UPDATE SET seconds = seconds + 1",
        )?
        .execute(params![date, minute, process_name])?;
        tx.commit()
    }

    pub fn add_idle_second(&self, date: &str) -> rusqlite::Result<()> {
        self.conn
            .prepare_cached(
                "INSERT INTO idle_daily (date, seconds) VALUES (?1, 1)
                 ON CONFLICT(date) DO UPDATE SET seconds = seconds + 1",
            )?
            .execute(params![date])?;
        Ok(())
    }

    pub fn set_app_path(&self, process_name: &str, exe_path: &str) -> rusqlite::Result<()> {
        self.conn
            .prepare_cached(
                "INSERT INTO apps (process_name, exe_path) VALUES (?1, ?2)
                 ON CONFLICT(process_name) DO UPDATE SET exe_path = excluded.exe_path",
            )?
            .execute(params![process_name, exe_path])?;
        Ok(())
    }

    pub fn app_paths(&self) -> rusqlite::Result<HashMap<String, String>> {
        let mut stmt = self
            .conn
            .prepare_cached("SELECT process_name, exe_path FROM apps")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect()
    }

    /// Per-app totals over the inclusive date range, largest first.
    pub fn usage_between(&self, from: &str, to: &str) -> rusqlite::Result<Vec<UsageRow>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT process_name, SUM(seconds) AS total FROM usage_daily
             WHERE date BETWEEN ?1 AND ?2
             GROUP BY process_name ORDER BY total DESC",
        )?;
        let rows = stmt.query_map(params![from, to], |r| {
            Ok(UsageRow {
                process_name: r.get(0)?,
                seconds: r.get(1)?,
            })
        })?;
        rows.collect()
    }

    /// Total tracked seconds per date over the inclusive range (dates without
    /// data are omitted).
    pub fn daily_totals(&self, from: &str, to: &str) -> rusqlite::Result<HashMap<String, i64>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT date, SUM(seconds) FROM usage_daily WHERE date BETWEEN ?1 AND ?2 GROUP BY date",
        )?;
        let rows = stmt.query_map(params![from, to], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect()
    }

    pub fn idle_between(&self, from: &str, to: &str) -> rusqlite::Result<i64> {
        self.conn.query_row(
            "SELECT COALESCE(SUM(seconds), 0) FROM idle_daily WHERE date BETWEEN ?1 AND ?2",
            params![from, to],
            |r| r.get(0),
        )
    }

    pub fn minutes_for_date(&self, date: &str) -> rusqlite::Result<Vec<MinuteRow>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT minute, process_name, seconds FROM usage_minutes WHERE date = ?1 ORDER BY minute",
        )?;
        let rows = stmt.query_map(params![date], |r| {
            Ok(MinuteRow {
                minute: r.get(0)?,
                process_name: r.get(1)?,
                seconds: r.get(2)?,
            })
        })?;
        rows.collect()
    }

    pub fn setting(&self, key: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .ok()
    }

    pub fn set_setting(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_second_updates_daily_and_minutes() {
        let mut db = UsageDatabase::open_in_memory().unwrap();
        db.add_second("2026-11-22", 600, "Chrome").unwrap();
        db.add_second("2026-11-22", 600, "Chrome").unwrap();
        db.add_second("2026-11-22", 601, "Code").unwrap();

        let usage = db.usage_between("2026-11-22", "2026-11-22").unwrap();
        assert_eq!(usage.len(), 2);
        assert_eq!(usage[0].process_name, "Chrome");
        assert_eq!(usage[0].seconds, 2);
        assert_eq!(usage[1].process_name, "Code");
        assert_eq!(usage[1].seconds, 1);

        let minutes = db.minutes_for_date("2026-11-22").unwrap();
        assert_eq!(minutes.len(), 2);
        assert_eq!(minutes[0].minute, 600);
        assert_eq!(minutes[0].process_name, "Chrome");
        assert_eq!(minutes[0].seconds, 2);
        assert_eq!(minutes[1].minute, 601);
        assert_eq!(minutes[1].process_name, "Code");
        assert_eq!(minutes[1].seconds, 1);
    }

    #[test]
    fn usage_between_and_daily_totals_filter_dates() {
        let mut db = UsageDatabase::open_in_memory().unwrap();
        db.add_second("2026-11-20", 10, "Chrome").unwrap();
        db.add_second("2026-11-21", 20, "Code").unwrap();
        db.add_second("2026-11-21", 21, "Code").unwrap();
        db.add_second("2026-11-22", 30, "Chrome").unwrap();

        let usage = db.usage_between("2026-11-20", "2026-11-21").unwrap();
        assert_eq!(usage.len(), 2);
        assert_eq!(usage[0].process_name, "Code");
        assert_eq!(usage[0].seconds, 2);
        assert_eq!(usage[1].process_name, "Chrome");
        assert_eq!(usage[1].seconds, 1);

        let totals = db.daily_totals("2026-11-19", "2026-11-21").unwrap();
        assert_eq!(totals.len(), 2);
        assert_eq!(totals.get("2026-11-19"), None);
        assert_eq!(totals.get("2026-11-20"), Some(&1));
        assert_eq!(totals.get("2026-11-21"), Some(&2));
    }

    #[test]
    fn idle_between_defaults_to_zero_and_sums_range() {
        let db = UsageDatabase::open_in_memory().unwrap();
        assert_eq!(db.idle_between("2026-11-20", "2026-11-22").unwrap(), 0);

        db.add_idle_second("2026-11-20").unwrap();
        db.add_idle_second("2026-11-20").unwrap();
        db.add_idle_second("2026-11-22").unwrap();

        assert_eq!(db.idle_between("2026-11-20", "2026-11-21").unwrap(), 2);
        assert_eq!(db.idle_between("2026-11-20", "2026-11-22").unwrap(), 3);
    }

    #[test]
    fn app_paths_and_settings_upsert() {
        let db = UsageDatabase::open_in_memory().unwrap();
        db.set_app_path("Chrome", "C:\\old\\chrome.exe").unwrap();
        db.set_app_path("Chrome", "C:\\new\\chrome.exe").unwrap();
        let paths = db.app_paths().unwrap();
        assert_eq!(
            paths.get("Chrome").map(String::as_str),
            Some("C:\\new\\chrome.exe")
        );

        assert_eq!(db.setting("theme"), None);
        db.set_setting("theme", "dark").unwrap();
        assert_eq!(db.setting("theme").as_deref(), Some("dark"));
        db.set_setting("theme", "light").unwrap();
        assert_eq!(db.setting("theme").as_deref(), Some("light"));
    }

    #[test]
    fn wal_file_persistence_survives_reopen() {
        let unique = format!(
            "timefactory-test-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let path = std::env::temp_dir().join(unique);

        {
            let mut db = UsageDatabase::open_at(&path).unwrap();
            db.add_second("2026-11-22", 720, "TimeFactoryTest").unwrap();
            db.set_setting("idle_minutes", "5").unwrap();
        }
        {
            let db = UsageDatabase::open_at(&path).unwrap();
            let usage = db.usage_between("2026-11-22", "2026-11-22").unwrap();
            assert_eq!(usage.len(), 1);
            assert_eq!(usage[0].process_name, "TimeFactoryTest");
            assert_eq!(usage[0].seconds, 1);
            assert_eq!(db.setting("idle_minutes").as_deref(), Some("5"));
        }

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }
}

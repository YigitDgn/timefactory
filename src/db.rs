use std::collections::HashMap;
use std::path::PathBuf;
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
        let path = database_path();
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }

        let conn = Connection::open(path)?;
        // The tracker thread writes while the UI reads: WAL lets both proceed.
        conn.busy_timeout(Duration::from_secs(2))?;
        conn.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()))?;
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

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use jiff::Zoned;

use crate::db::UsageDatabase;
use crate::tracker;

pub const DEFAULT_IDLE_MINUTES: u64 = 2;

/// What the tracker saw on its latest tick, for the UI.
#[derive(Default, Clone)]
pub struct LiveState {
    /// Foreground app right now (`None` if it's TimeFactory itself or unknown).
    pub active: Option<String>,
    /// Last foreground app other than TimeFactory.
    pub last_active: Option<String>,
    /// Every app with a visible window.
    pub open: HashSet<String>,
    pub idle: bool,
}

pub struct Shared {
    pub live: Mutex<LiveState>,
    pub paused: AtomicBool,
    pub idle_threshold_secs: AtomicU64,
}

impl Shared {
    pub fn new(idle_minutes: u64) -> Arc<Shared> {
        Arc::new(Shared {
            live: Mutex::new(LiveState::default()),
            paused: AtomicBool::new(false),
            idle_threshold_secs: AtomicU64::new(idle_minutes * 60),
        })
    }

    pub fn live(&self) -> LiveState {
        self.live.lock().map(|l| l.clone()).unwrap_or_default()
    }
}

/// Samples the foreground app once a second on a background thread, so
/// tracking continues while the window is hidden. Only the foreground app
/// accrues time; nothing is counted while paused, and time without keyboard
/// or mouse input beyond the idle threshold is counted as idle instead.
pub fn spawn(shared: Arc<Shared>) {
    thread::spawn(move || {
        let mut db = match UsageDatabase::open() {
            Ok(db) => db,
            Err(_) => return,
        };
        let mut paths_saved: HashSet<String> = HashSet::new();
        let mut next_tick = Instant::now();

        loop {
            next_tick += Duration::from_secs(1);
            tick(&shared, &mut db, &mut paths_saved);

            let now = Instant::now();
            if next_tick > now {
                thread::sleep(next_tick - now);
            } else {
                // Fell behind (e.g. after sleep/hibernate): don't try to catch up.
                next_tick = now;
            }
        }
    });
}

fn tick(shared: &Shared, db: &mut UsageDatabase, paths_saved: &mut HashSet<String>) {
    let now = Zoned::now();
    let date = now.date().to_string();
    let minute = now.hour() as u32 * 60 + now.minute() as u32;

    let open = tracker::open_display_names();
    let active = tracker::active_app();
    let idle = tracker::idle_seconds() >= shared.idle_threshold_secs.load(Ordering::Relaxed);

    apply_tick(shared, db, paths_saved, &date, minute, open, active, idle);
}

#[allow(clippy::too_many_arguments)]
fn apply_tick(
    shared: &Shared,
    db: &mut UsageDatabase,
    paths_saved: &mut HashSet<String>,
    date: &str,
    minute: u32,
    open: HashSet<String>,
    active: Option<(String, String)>,
    idle: bool,
) {
    if !shared.paused.load(Ordering::Relaxed) {
        if idle {
            let _ = db.add_idle_second(date);
        } else if let Some((name, path)) = &active {
            let _ = db.add_second(date, minute, name);
            // Remember where the exe lives so the UI can show its icon.
            if paths_saved.insert(name.clone()) {
                let _ = db.set_app_path(name, path);
            }
        }
    }

    if let Ok(mut live) = shared.live.lock() {
        let active = active.map(|(name, _)| name);
        if active.is_some() {
            live.last_active = active.clone();
        }
        live.active = active;
        live.open = open;
        live.idle = idle;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_tick_records_usage_and_caches_app_path_once() {
        let shared = Shared::new(DEFAULT_IDLE_MINUTES);
        let mut db = UsageDatabase::open_in_memory().unwrap();
        let mut saved = HashSet::new();

        apply_tick(
            &shared,
            &mut db,
            &mut saved,
            "2026-11-22",
            600,
            HashSet::from(["Chrome".into()]),
            Some(("Chrome".into(), "C:\\chrome.exe".into())),
            false,
        );
        // Second tick with a different path should not overwrite once cached in paths_saved.
        apply_tick(
            &shared,
            &mut db,
            &mut saved,
            "2026-11-22",
            600,
            HashSet::from(["Chrome".into()]),
            Some(("Chrome".into(), "D:\\other.exe".into())),
            false,
        );

        let rows = db.usage_between("2026-11-22", "2026-11-22").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].process_name, "Chrome");
        assert_eq!(rows[0].seconds, 2);
        assert_eq!(
            db.app_paths().unwrap().get("Chrome").map(String::as_str),
            Some("C:\\chrome.exe")
        );

        let live = shared.live();
        assert_eq!(live.active.as_deref(), Some("Chrome"));
        assert_eq!(live.last_active.as_deref(), Some("Chrome"));
        assert!(!live.idle);
    }

    #[test]
    fn paused_tick_skips_db_writes_but_updates_live_state() {
        let shared = Shared::new(DEFAULT_IDLE_MINUTES);
        shared.paused.store(true, Ordering::Relaxed);
        let mut db = UsageDatabase::open_in_memory().unwrap();
        let mut saved = HashSet::new();

        apply_tick(
            &shared,
            &mut db,
            &mut saved,
            "2026-11-22",
            605,
            HashSet::from(["Code".into()]),
            Some(("Code".into(), "C:\\code.exe".into())),
            false,
        );

        assert!(db
            .usage_between("2026-11-22", "2026-11-22")
            .unwrap()
            .is_empty());
        assert_eq!(db.idle_between("2026-11-22", "2026-11-22").unwrap(), 0);
        assert_eq!(shared.live().active.as_deref(), Some("Code"));
    }

    #[test]
    fn idle_tick_records_idle_only() {
        let shared = Shared::new(DEFAULT_IDLE_MINUTES);
        let mut db = UsageDatabase::open_in_memory().unwrap();
        let mut saved = HashSet::new();

        apply_tick(
            &shared,
            &mut db,
            &mut saved,
            "2026-11-22",
            610,
            HashSet::new(),
            Some(("Chrome".into(), "C:\\chrome.exe".into())),
            true,
        );

        assert!(db
            .usage_between("2026-11-22", "2026-11-22")
            .unwrap()
            .is_empty());
        assert_eq!(db.idle_between("2026-11-22", "2026-11-22").unwrap(), 1);
        assert!(shared.live().idle);
    }

    #[test]
    fn none_active_preserves_last_active() {
        let shared = Shared::new(DEFAULT_IDLE_MINUTES);
        let mut db = UsageDatabase::open_in_memory().unwrap();
        let mut saved = HashSet::new();

        apply_tick(
            &shared,
            &mut db,
            &mut saved,
            "2026-11-22",
            615,
            HashSet::new(),
            Some(("Firefox".into(), "C:\\firefox.exe".into())),
            false,
        );
        apply_tick(
            &shared,
            &mut db,
            &mut saved,
            "2026-11-22",
            616,
            HashSet::new(),
            None,
            false,
        );

        let live = shared.live();
        assert_eq!(live.active, None);
        assert_eq!(live.last_active.as_deref(), Some("Firefox"));
    }

    #[test]
    fn local_now_resolves_valid_date_and_minute() {
        let now = Zoned::now();
        assert_eq!(now.date().to_string().len(), 10);
        let minute = now.hour() as u32 * 60 + now.minute() as u32;
        assert!(minute < 1440);
    }
}

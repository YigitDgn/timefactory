use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use chrono::{Local, Timelike};

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
    let now = Local::now();
    let date = now.format("%Y-%m-%d").to_string();
    let minute = now.hour() * 60 + now.minute();

    let open = tracker::open_display_names();
    let active = tracker::active_app();
    let idle = tracker::idle_seconds() >= shared.idle_threshold_secs.load(Ordering::Relaxed);

    if !shared.paused.load(Ordering::Relaxed) {
        if idle {
            let _ = db.add_idle_second(&date);
        } else if let Some((name, path)) = &active {
            let _ = db.add_second(&date, minute, name);
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

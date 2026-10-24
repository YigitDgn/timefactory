mod format;
mod style;
mod widgets;

use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{Datelike, Days, Local, Months, NaiveDate};
use eframe::egui::{
    self, text::LayoutJob, vec2, Align, Color32, Frame, Layout, Margin, RichText, ScrollArea,
    Stroke, TextFormat, TextureHandle, TextureId, Ui,
};
use egui_phosphor::regular as icon;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winapi::shared::windef::HWND;

use crate::db::{database_path, MinuteRow, UsageDatabase, UsageRow};
use crate::theme::{self, ThemeMode};
use crate::tracking::{LiveState, Shared, DEFAULT_IDLE_MINUTES};
use crate::tray::{self, Tray};
use crate::winicon;
use format::{month_name, short_duration};
use style::{semibold, Palette, ACTIVE_COLOR, APP_COLORS, OPEN_COLOR, PAUSED_COLOR};
use widgets::{card, card_frame, empty_note, KpiValue};

const APP_TITLE: &str = "TimeFactory";
// Taken from Cargo.toml's `version` at compile time.
const VERSION_TEXT: &str = concat!("v", env!("CARGO_PKG_VERSION"));
const THEME_SETTING: &str = "theme";
pub const IDLE_SETTING: &str = "idle_minutes";
const IDLE_CHOICES: [u64; 7] = [1, 2, 3, 5, 10, 15, 30];
const TOP_APPS: usize = 6;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Day,
    Week,
    Month,
    Settings,
}

const NAV: [(Page, &str, &str); 4] = [
    (Page::Day, icon::HOUSE, "Bugün"),
    (Page::Week, icon::CHART_BAR, "Haftalık"),
    (Page::Month, icon::CALENDAR_BLANK, "Aylık"),
    (Page::Settings, icon::GEAR, "Ayarlar"),
];

fn today() -> NaiveDate {
    Local::now().date_naive()
}

fn iso(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

/// First and last day covered by `page` around `date`.
fn period(page: Page, date: NaiveDate) -> (NaiveDate, NaiveDate) {
    match page {
        Page::Week => {
            let monday = date - Days::new(date.weekday().num_days_from_monday() as u64);
            (monday, monday + Days::new(6))
        }
        Page::Month => {
            let first = date.with_day(1).unwrap_or(date);
            let last = first
                .checked_add_months(Months::new(1))
                .map(|d| d - Days::new(1))
                .unwrap_or(date);
            (first, last)
        }
        Page::Day | Page::Settings => (date, date),
    }
}

/// Everything the current page shows, read from the database.
struct ViewData {
    page: Page,
    date: NaiveDate,
    loaded_at: Instant,
    total: i64,
    idle: i64,
    apps: Vec<UsageRow>,
    /// Per-day totals for the column chart.
    days: Vec<(NaiveDate, i64)>,
    minutes: Vec<MinuteRow>,
}

impl ViewData {
    fn load(db: &UsageDatabase, page: Page, date: NaiveDate) -> ViewData {
        let (from, to) = period(page, date);
        let apps = db.usage_between(&iso(from), &iso(to)).unwrap_or_default();

        // The day view charts the 7 days ending at the selected day.
        let (chart_from, chart_to) = match page {
            Page::Day => (date - Days::new(6), date),
            _ => (from, to),
        };
        let totals = db
            .daily_totals(&iso(chart_from), &iso(chart_to))
            .unwrap_or_default();
        let days = chart_from
            .iter_days()
            .take_while(|d| *d <= chart_to)
            .map(|d| (d, totals.get(&iso(d)).copied().unwrap_or(0)))
            .collect();

        ViewData {
            page,
            date,
            loaded_at: Instant::now(),
            total: apps.iter().map(|a| a.seconds).sum(),
            idle: db.idle_between(&iso(from), &iso(to)).unwrap_or(0),
            apps,
            days,
            minutes: if page == Page::Day {
                db.minutes_for_date(&iso(date)).unwrap_or_default()
            } else {
                Vec::new()
            },
        }
    }
}

/// Exe icons as textures, looked up by app display name.
#[derive(Default)]
struct IconCache {
    paths: HashMap<String, String>,
    paths_loaded: Option<Instant>,
    textures: HashMap<String, Option<TextureHandle>>,
}

impl IconCache {
    fn prepare<'a>(
        &mut self,
        ctx: &egui::Context,
        db: &UsageDatabase,
        names: impl Iterator<Item = &'a str>,
    ) {
        for name in names {
            if self.textures.contains_key(name) {
                continue;
            }
            if !self.paths.contains_key(name)
                && self
                    .paths_loaded
                    .is_none_or(|t| t.elapsed() > Duration::from_secs(5))
            {
                self.paths = db.app_paths().unwrap_or_default();
                self.paths_loaded = Some(Instant::now());
            }
            // Apps recorded before paths were stored get a letter tile.
            if let Some(path) = self.paths.get(name) {
                let texture = winicon::from_exe(path, 64).map(|icon| {
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [icon.width as usize, icon.height as usize],
                        &icon.rgba,
                    );
                    ctx.load_texture(format!("app-{name}"), image, egui::TextureOptions::LINEAR)
                });
                self.textures.insert(name.to_string(), texture);
            }
        }
    }

    fn ids(&self) -> HashMap<String, TextureId> {
        self.textures
            .iter()
            .filter_map(|(name, t)| t.as_ref().map(|t| (name.clone(), t.id())))
            .collect()
    }
}

pub struct TimeFactoryApp {
    db: UsageDatabase,
    shared: Arc<Shared>,
    tray: Option<Tray>,
    hwnd: HWND,

    page: Page,
    date: NaiveDate,
    data: Option<ViewData>,
    calendar_month: Option<NaiveDate>,

    theme_mode: ThemeMode,
    dark: bool,
    palette: Palette,
    idle_minutes: u64,

    icons: IconCache,
    logo: Option<TextureHandle>,
}

impl TimeFactoryApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        db: UsageDatabase,
        shared: Arc<Shared>,
        show_message: u32,
    ) -> Self {
        let ctx = &cc.egui_ctx;
        style::install_fonts(ctx);

        let hwnd = cc
            .window_handle()
            .ok()
            .and_then(|h| match h.as_raw() {
                RawWindowHandle::Win32(w) => Some(w.hwnd.get() as HWND),
                _ => None,
            })
            .unwrap_or(std::ptr::null_mut());
        tray::set_main_window(hwnd);
        let tray = Tray::create(ctx.clone(), shared.clone(), show_message);

        let logo = winicon::from_own_resource(1, 72).map(|icon| {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [icon.width as usize, icon.height as usize],
                &icon.rgba,
            );
            ctx.load_texture("logo", image, egui::TextureOptions::LINEAR)
        });

        let theme_mode = db
            .setting(THEME_SETTING)
            .and_then(|s| ThemeMode::parse(&s))
            .unwrap_or(ThemeMode::System);
        let idle_minutes = db
            .setting(IDLE_SETTING)
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_IDLE_MINUTES);

        let mut app = Self {
            db,
            shared,
            tray,
            hwnd,
            page: Page::Day,
            date: today(),
            data: None,
            calendar_month: None,
            theme_mode,
            dark: false,
            palette: style::LIGHT,
            idle_minutes,
            icons: IconCache::default(),
            logo,
        };
        app.apply_theme(ctx);
        app
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        self.dark = self.theme_mode.is_dark();
        self.palette = if self.dark { style::DARK } else { style::LIGHT };
        ctx.set_visuals(style::visuals(&self.palette, self.dark));
        if !self.hwnd.is_null() {
            theme::set_title_bar_dark(self.hwnd, self.dark);
        }
        theme::set_app_dark_mode(self.dark);
    }

    /// Reloads the page data once a second (or when the view changed) and
    /// follows the Windows theme in "Sistem" mode.
    fn refresh(&mut self, ctx: &egui::Context) {
        let page = if self.page == Page::Settings {
            self.data.as_ref().map_or(Page::Day, |d| d.page)
        } else {
            self.page
        };
        let stale = self.data.as_ref().is_none_or(|d| {
            d.page != page || d.date != self.date || d.loaded_at.elapsed() >= Duration::from_secs(1)
        });
        if !stale {
            return;
        }
        self.data = Some(ViewData::load(&self.db, page, self.date));
        if self.theme_mode == ThemeMode::System && theme::system_prefers_dark() != self.dark {
            self.apply_theme(ctx);
        }
    }

    fn handle_close(&mut self, ctx: &egui::Context) {
        // The window's close button only hides it; tracking keeps running.
        // Without a tray icon there'd be no way back, so then it really closes.
        if ctx.input(|i| i.viewport().close_requested()) {
            if let Some(tray) = &self.tray {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                tray::hide_main_window();
                tray.notify(APP_TITLE, "Uygulama arka planda çalışmayı sürdürüyor.");
            }
        }
    }

    fn shift_period(&mut self, forward: bool) {
        let d = self.date;
        let shifted = match (self.page, forward) {
            (Page::Week, true) => d.checked_add_days(Days::new(7)),
            (Page::Week, false) => d.checked_sub_days(Days::new(7)),
            (Page::Month, true) => d.checked_add_months(Months::new(1)),
            (Page::Month, false) => d.checked_sub_months(Months::new(1)),
            (_, true) => d.checked_add_days(Days::new(1)),
            (_, false) => d.checked_sub_days(Days::new(1)),
        };
        if let Some(date) = shifted {
            self.date = date.min(today());
        }
    }

    fn period_title(&self) -> String {
        let (from, to) = period(self.page, self.date);
        match self.page {
            Page::Week => format!("{} – {}", from.format("%d.%m"), to.format("%d.%m.%Y")),
            Page::Month => format!("{} {}", month_name(from.month()), from.year()),
            _ => self.date.format("%d.%m.%Y").to_string(),
        }
    }

    fn sidebar(&mut self, ctx: &egui::Context) {
        let p = self.palette;
        egui::SidePanel::left("nav")
            .exact_width(236.0)
            .resizable(false)
            .show_separator_line(false)
            .frame(Frame::new().fill(p.sidebar).inner_margin(Margin {
                left: 16,
                right: 16,
                top: 22,
                bottom: 16,
            }))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    if let Some(logo) = &self.logo {
                        ui.add(egui::Image::from_texture(egui::load::SizedTexture::new(
                            logo.id(),
                            vec2(38.0, 38.0),
                        )));
                    }
                    ui.add_space(4.0);
                    let mut job = LayoutJob::default();
                    let format = |color| TextFormat {
                        font_id: semibold(24.0),
                        color,
                        ..Default::default()
                    };
                    job.append("Time", 0.0, format(p.text));
                    job.append("Factory", 0.0, format(p.accent));
                    ui.label(job);
                });
                ui.add_space(28.0);

                for (page, icon, label) in NAV {
                    if widgets::nav_item(ui, &p, icon, label, self.page == page).clicked() {
                        // "Bugün" jumps back to today.
                        if page == Page::Day {
                            self.date = today();
                        }
                        self.page = page;
                        self.calendar_month = None;
                    }
                    ui.add_space(4.0);
                }
            });
    }

    fn top_bar(&mut self, ui: &mut Ui, live: &LiveState) {
        let p = self.palette;
        let today = today();
        ui.horizontal(|ui| {
            if self.page == Page::Settings {
                ui.label(RichText::new("Ayarlar").font(semibold(24.0)));
            } else {
                let mut calendar_clicked = false;
                let nav = Frame::new()
                    .fill(p.card)
                    .stroke(Stroke::new(1.0_f32, p.card_border))
                    .corner_radius(style::radius(12))
                    .inner_margin(Margin::symmetric(8, 5))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            let arrow = |glyph| {
                                egui::Button::new(RichText::new(glyph).size(18.0))
                                    .frame(false)
                                    .min_size(vec2(34.0, 32.0))
                            };
                            if ui.add(arrow(icon::CARET_LEFT)).clicked() {
                                self.shift_period(false);
                            }
                            let title_w = if self.page == Page::Week {
                                170.0
                            } else {
                                120.0
                            };
                            ui.add_sized(
                                vec2(title_w, 32.0),
                                egui::Label::new(
                                    RichText::new(self.period_title()).font(semibold(17.0)),
                                ),
                            );
                            let can_next = period(self.page, self.date).1 < today;
                            if ui.add_enabled(can_next, arrow(icon::CARET_RIGHT)).clicked() {
                                self.shift_period(true);
                            }
                            ui.separator();
                            calendar_clicked = ui
                                .add(arrow(icon::CALENDAR_BLANK))
                                .on_hover_text("Tarih seç")
                                .clicked();
                        });
                    });
                if calendar_clicked {
                    self.calendar_month = match self.calendar_month {
                        Some(_) => None,
                        None => Some(self.date),
                    };
                }
                self.calendar_popup(ui.ctx(), nav.response.rect, calendar_clicked);
            }

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let paused = self.shared.paused.load(Ordering::Relaxed);
                let (text, color) = if paused {
                    ("Duraklatıldı", PAUSED_COLOR)
                } else if live.idle {
                    ("Boşta", OPEN_COLOR)
                } else {
                    ("Takip ediliyor", ACTIVE_COLOR)
                };
                let hint = if paused {
                    "Takibe devam etmek için tıklayın"
                } else {
                    "Takibi duraklatmak için tıklayın"
                };
                if widgets::status_pill(ui, &p, text, color)
                    .on_hover_text(hint)
                    .clicked()
                {
                    self.shared.paused.store(!paused, Ordering::Relaxed);
                }
            });
        });
    }

    fn calendar_popup(&mut self, ctx: &egui::Context, anchor: egui::Rect, just_opened: bool) {
        let Some(mut month) = self.calendar_month else {
            return;
        };
        let p = self.palette;
        let area = egui::Area::new(egui::Id::new("calendar"))
            .order(egui::Order::Foreground)
            .fixed_pos(anchor.left_bottom() + vec2(0.0, 8.0))
            .show(ctx, |ui| {
                card_frame(&p)
                    .shadow(ui.visuals().popup_shadow)
                    .show(ui, |ui| {
                        widgets::calendar(ui, &p, &mut month, self.date, today())
                    })
                    .inner
            });
        if let Some(date) = area.inner {
            self.date = date;
            self.calendar_month = None;
        } else if area.response.clicked_elsewhere() && !just_opened {
            self.calendar_month = None;
        } else {
            self.calendar_month = Some(month);
        }
    }

    fn kpi_row(ui: &mut Ui, p: &Palette, kpis: Vec<(&str, &str, KpiValue)>) {
        let per_row = if ui.available_width() < 820.0 { 2 } else { 4 };
        let mut kpis = kpis.into_iter();
        loop {
            let chunk: Vec<_> = kpis.by_ref().take(per_row).collect();
            if chunk.is_empty() {
                break;
            }
            ui.columns(per_row, |cols| {
                for (col, (glyph, label, value)) in cols.iter_mut().zip(chunk) {
                    widgets::kpi_card(col, p, glyph, label, value);
                }
            });
        }
    }

    /// Two columns side by side, or stacked when the window is narrow.
    fn two_columns(ui: &mut Ui, left: impl FnOnce(&mut Ui), right: impl FnOnce(&mut Ui)) {
        if ui.available_width() < 760.0 {
            left(ui);
            right(ui);
        } else {
            ui.columns(2, |cols| {
                left(&mut cols[0]);
                right(&mut cols[1]);
            });
        }
    }

    fn app_kpi<'a>(name: Option<&'a str>, icons: &HashMap<String, TextureId>) -> KpiValue<'a> {
        match name {
            Some(name) => KpiValue {
                text: name.to_string(),
                app_icon: Some((icons.get(name).copied(), name)),
            },
            None => KpiValue {
                text: "—".into(),
                app_icon: None,
            },
        }
    }

    fn day_page(
        &mut self,
        ui: &mut Ui,
        data: &ViewData,
        live: &LiveState,
        icons: &HashMap<String, TextureId>,
    ) {
        let p = self.palette;
        let today = today();
        let viewing_today = self.date == today;

        let current = live.active.as_deref().or(live.last_active.as_deref());
        let current_label = if live.active.is_some() {
            "Şu an aktif"
        } else {
            "Son aktif"
        };
        Self::kpi_row(
            ui,
            &p,
            vec![
                (
                    icon::CLOCK,
                    if viewing_today {
                        "Bugün toplam"
                    } else {
                        "Toplam"
                    },
                    KpiValue {
                        text: short_duration(data.total),
                        app_icon: None,
                    },
                ),
                (
                    icon::STAR,
                    "En çok",
                    Self::app_kpi(data.apps.first().map(|a| a.process_name.as_str()), icons),
                ),
                (icon::MONITOR, current_label, Self::app_kpi(current, icons)),
                (
                    icon::PAUSE,
                    "Boşta",
                    KpiValue {
                        text: short_duration(data.idle),
                        app_icon: None,
                    },
                ),
            ],
        );

        let colors = widgets::app_colors(&data.apps, &APP_COLORS);
        let legend: Vec<(String, Color32)> = data
            .apps
            .iter()
            .take(TOP_APPS)
            .filter_map(|a| {
                colors
                    .get(&a.process_name)
                    .map(|c| (a.process_name.clone(), *c))
            })
            .collect();
        let status = |name: &str| -> Option<Color32> {
            if !viewing_today {
                None
            } else if live.active.as_deref() == Some(name) {
                Some(ACTIVE_COLOR)
            } else if live.open.contains(name) {
                Some(OPEN_COLOR)
            } else {
                None
            }
        };

        let mut picked = None;
        Self::two_columns(
            ui,
            |ui| {
                card(ui, &p, "En Çok Kullanılan Uygulamalar", |ui| {
                    if data.apps.is_empty() {
                        empty_note(ui, &p, "Bu gün için kayıt yok.");
                    }
                    widgets::app_bars(ui, &p, icons, &data.apps, TOP_APPS);
                });
                ui.add_space(16.0);
                card(ui, &p, "Uygulama Kullanım Detayları", |ui| {
                    widgets::usage_table(ui, &p, icons, &data.apps, Some(&status));
                });
            },
            |ui| {
                let title = if viewing_today {
                    "Saatlik Aktivite (Bugün)".to_string()
                } else {
                    format!("Saatlik Aktivite ({})", self.date.format("%d.%m"))
                };
                card(ui, &p, &title, |ui| {
                    widgets::timeline(ui, &p, &data.minutes, &colors, &legend);
                });
                ui.add_space(16.0);
                card(ui, &p, "Son 7 Günlük Aktivite", |ui| {
                    picked = widgets::day_columns(ui, &p, &data.days, Some(self.date), today);
                });
            },
        );
        if let Some(date) = picked {
            self.date = date;
        }
    }

    /// Weekly and monthly pages: totals over the period, per-day columns.
    fn period_page(&mut self, ui: &mut Ui, data: &ViewData, icons: &HashMap<String, TextureId>) {
        let p = self.palette;
        let today = today();
        let (from, to) = period(self.page, self.date);
        let elapsed_days = (to.min(today) - from).num_days().max(0) + 1;
        let (total_label, chart_title) = match self.page {
            Page::Week => ("Haftalık toplam", "Günlük Dağılım (Hafta)"),
            _ => ("Aylık toplam", "Günlük Dağılım (Ay)"),
        };

        Self::kpi_row(
            ui,
            &p,
            vec![
                (
                    icon::CLOCK,
                    total_label,
                    KpiValue {
                        text: short_duration(data.total),
                        app_icon: None,
                    },
                ),
                (
                    icon::STAR,
                    "En çok",
                    Self::app_kpi(data.apps.first().map(|a| a.process_name.as_str()), icons),
                ),
                (
                    icon::CHART_BAR,
                    "Günlük ortalama",
                    KpiValue {
                        text: short_duration(data.total / elapsed_days),
                        app_icon: None,
                    },
                ),
                (
                    icon::PAUSE,
                    "Boşta",
                    KpiValue {
                        text: short_duration(data.idle),
                        app_icon: None,
                    },
                ),
            ],
        );

        let mut picked = None;
        Self::two_columns(
            ui,
            |ui| {
                card(ui, &p, "En Çok Kullanılan Uygulamalar", |ui| {
                    if data.apps.is_empty() {
                        empty_note(ui, &p, "Bu dönem için kayıt yok.");
                    }
                    widgets::app_bars(ui, &p, icons, &data.apps, TOP_APPS);
                });
                ui.add_space(16.0);
                card(ui, &p, "Uygulama Kullanım Detayları", |ui| {
                    widgets::usage_table(ui, &p, icons, &data.apps, None);
                });
            },
            |ui| {
                card(ui, &p, chart_title, |ui| {
                    picked = widgets::day_columns(ui, &p, &data.days, Some(self.date), today);
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("Bir güne tıklayarak o günün ayrıntılarını açabilirsiniz.")
                            .color(p.muted)
                            .size(13.0),
                    );
                });
            },
        );
        // Clicking a day opens it in the day view.
        if let Some(date) = picked {
            self.date = date;
            self.page = Page::Day;
        }
    }

    fn settings_page(&mut self, ui: &mut Ui, ctx: &egui::Context) {
        let p = self.palette;
        card(ui, &p, "Görünüm", |ui| {
            ui.label("Tema");
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let mut mode = self.theme_mode;
                for (value, label) in [
                    (ThemeMode::System, "Sistem"),
                    (ThemeMode::Light, "Açık"),
                    (ThemeMode::Dark, "Koyu"),
                ] {
                    ui.selectable_value(&mut mode, value, label);
                }
                if mode != self.theme_mode {
                    self.theme_mode = mode;
                    let _ = self.db.set_setting(THEME_SETTING, mode.as_str());
                    self.apply_theme(ctx);
                }
            });
            ui.label(
                RichText::new("\"Sistem\", Windows'un açık/koyu mod ayarını takip eder.")
                    .color(p.muted)
                    .size(13.0),
            );
        });
        ui.add_space(16.0);

        card(ui, &p, "Boşta Algılama", |ui| {
            ui.horizontal(|ui| {
                ui.label("Klavye ve fare kullanılmazsa");
                let mut minutes = self.idle_minutes;
                egui::ComboBox::from_id_salt("idle-minutes")
                    .selected_text(format!("{minutes} dakika"))
                    .show_ui(ui, |ui| {
                        for choice in IDLE_CHOICES {
                            ui.selectable_value(&mut minutes, choice, format!("{choice} dakika"));
                        }
                    });
                ui.label("sonra boşta say");
                if minutes != self.idle_minutes {
                    self.idle_minutes = minutes;
                    self.shared
                        .idle_threshold_secs
                        .store(minutes * 60, Ordering::Relaxed);
                    let _ = self.db.set_setting(IDLE_SETTING, &minutes.to_string());
                }
            });
            ui.label(
                RichText::new(
                    "Boşta geçen süre uygulamalara eklenmez, \"Boşta\" olarak ayrıca gösterilir.",
                )
                .color(p.muted)
                .size(13.0),
            );
        });
        ui.add_space(16.0);

        card(ui, &p, "Hakkında", |ui| {
            ui.label(format!("TimeFactory {VERSION_TEXT}"));
            ui.label(RichText::new("Veritabanı").color(p.muted));
            ui.add(
                egui::Label::new(database_path().display().to_string())
                    .selectable(true)
                    .wrap(),
            );
            ui.label(
                RichText::new("Tüm veriler yalnızca bu bilgisayarda saklanır.")
                    .color(p.muted)
                    .size(13.0),
            );
        });
    }
}

impl eframe::App for TimeFactoryApp {
    // Shows through the seam between the side panel and the content, so it
    // doubles as the divider colour (eframe's default is near-black).
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        self.palette.card_border.to_normalized_gamma_f32()
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_close(ctx);
        self.refresh(ctx);
        ctx.request_repaint_after(Duration::from_secs(1));

        let live = self.shared.live();
        let data = self.data.take();
        if let Some(data) = &data {
            let names = data
                .apps
                .iter()
                .map(|a| a.process_name.as_str())
                .chain(live.active.as_deref())
                .chain(live.last_active.as_deref());
            self.icons.prepare(ctx, &self.db, names);
        }
        let icons = self.icons.ids();

        self.sidebar(ctx);

        let p = self.palette;
        egui::CentralPanel::default()
            .frame(Frame::new().fill(p.bg).inner_margin(Margin {
                left: 22,
                right: 22,
                top: 18,
                bottom: 6,
            }))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = vec2(16.0, 16.0);
                self.top_bar(ui, &live);

                let height = ui.available_height() - 22.0;
                ScrollArea::vertical()
                    .auto_shrink(false)
                    .max_height(height)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = vec2(16.0, 16.0);
                        match (self.page, &data) {
                            (Page::Settings, _) => self.settings_page(ui, ctx),
                            (Page::Day, Some(data)) => self.day_page(ui, data, &live, &icons),
                            (_, Some(data)) => self.period_page(ui, data, &icons),
                            _ => {}
                        }
                    });

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(VERSION_TEXT).size(12.0).color(p.muted));
                });
            });

        self.data = data;
    }
}

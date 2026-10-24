use std::collections::{BTreeMap, HashMap};

use chrono::{Datelike, Days, Months, NaiveDate};
use eframe::egui::{
    self, pos2, text::LayoutJob, text::TextWrapping, vec2, Align2, Color32, CornerRadius, FontId,
    Frame, Margin, Pos2, Rect, Response, Sense, Stroke, StrokeKind, TextureId, Ui,
};

use super::format::{hm_clock, hms, month_name, short_duration};
use super::style::{radius, regular, semibold, Palette, OTHER_APP_COLOR};
use crate::db::{MinuteRow, UsageRow};

const FULL_UV: Rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));

pub fn card_frame(p: &Palette) -> Frame {
    Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.0_f32, p.card_border))
        .corner_radius(radius(12))
        .inner_margin(Margin::same(18))
}

/// A rounded panel with an optional title, filling the available width.
pub fn card<R>(ui: &mut Ui, p: &Palette, title: &str, add: impl FnOnce(&mut Ui) -> R) -> R {
    card_frame(p)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            if !title.is_empty() {
                ui.label(
                    egui::RichText::new(title)
                        .font(semibold(16.0))
                        .color(p.text),
                );
                ui.add_space(10.0);
            }
            add(ui)
        })
        .inner
}

pub fn empty_note(ui: &mut Ui, p: &Palette, text: &str) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(text).color(p.muted));
    ui.add_space(6.0);
}

/// Paints `text` vertically centred on `left_center`, cut with "…" when it
/// is wider than `max_width`.
pub fn text_truncated(
    ui: &Ui,
    left_center: Pos2,
    text: &str,
    font: FontId,
    color: Color32,
    max_width: f32,
) {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = TextWrapping {
        max_width: max_width.max(1.0),
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };
    let galley = ui.fonts(|f| f.layout_job(job));
    let pos = pos2(left_center.x, left_center.y - galley.size().y / 2.0);
    ui.painter().galley(pos, galley, color);
}

/// The app's exe icon, or a letter tile when there is none.
pub fn paint_app_icon(ui: &Ui, p: &Palette, icon: Option<TextureId>, name: &str, rect: Rect) {
    match icon {
        Some(texture) => {
            ui.painter().image(texture, rect, FULL_UV, Color32::WHITE);
        }
        None => {
            ui.painter().rect_filled(rect, radius(6), p.track);
            let letter: String = name
                .chars()
                .next()
                .map(|c| c.to_uppercase().collect())
                .unwrap_or_default();
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                letter,
                semibold(rect.height() * 0.55),
                p.muted,
            );
        }
    }
}

pub struct KpiValue<'a> {
    pub text: String,
    pub app_icon: Option<(Option<TextureId>, &'a str)>,
}

pub fn kpi_card(ui: &mut Ui, p: &Palette, icon: &str, label: &str, value: KpiValue) {
    card_frame(p).show(ui, |ui| {
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), 62.0), Sense::hover());
        // Long app names get cut; show the full name on hover.
        if value.app_icon.is_some() {
            response.on_hover_text(&value.text);
        }
        let painter = ui.painter();

        let circle = pos2(rect.left() + 26.0, rect.center().y);
        painter.circle_filled(circle, 26.0, p.accent_soft);
        painter.text(circle, Align2::CENTER_CENTER, icon, regular(24.0), p.accent);

        let x = rect.left() + 70.0;
        let width = rect.right() - x;
        text_truncated(
            ui,
            pos2(x, rect.top() + 16.0),
            label,
            regular(14.5),
            p.muted,
            width,
        );

        let value_y = rect.top() + 44.0;
        let mut text_x = x;
        if let Some((texture, name)) = value.app_icon {
            let icon_rect = Rect::from_center_size(pos2(x + 13.0, value_y), vec2(26.0, 26.0));
            paint_app_icon(ui, p, texture, name, icon_rect);
            text_x += 36.0;
        }
        // App names are longer than durations, so they get a smaller font.
        let size = if value.app_icon.is_some() { 18.5 } else { 22.0 };
        text_truncated(
            ui,
            pos2(text_x, value_y),
            &value.text,
            semibold(size),
            p.text,
            rect.right() - text_x,
        );
    });
}

/// Horizontal bars for the top apps, scaled to the largest.
pub fn app_bars(
    ui: &mut Ui,
    p: &Palette,
    icons: &HashMap<String, TextureId>,
    apps: &[UsageRow],
    limit: usize,
) {
    let max = apps.first().map(|a| a.seconds).unwrap_or(1).max(1) as f32;
    ui.spacing_mut().item_spacing.y = 2.0;
    for app in apps.iter().take(limit) {
        let (rect, response) =
            ui.allocate_exact_size(vec2(ui.available_width(), 40.0), Sense::hover());
        let cy = rect.center().y;

        let icon_rect = Rect::from_center_size(pos2(rect.left() + 13.0, cy), vec2(26.0, 26.0));
        paint_app_icon(
            ui,
            p,
            icons.get(&app.process_name).copied(),
            &app.process_name,
            icon_rect,
        );

        let name_x = rect.left() + 40.0;
        let name_w = (rect.width() * 0.32).clamp(90.0, 190.0);
        text_truncated(
            ui,
            pos2(name_x, cy),
            &app.process_name,
            regular(14.5),
            p.text,
            name_w - 10.0,
        );

        let duration_w = 74.0;
        let bar_left = name_x + name_w;
        let bar_right = (rect.right() - duration_w).max(bar_left + 10.0);
        let track = Rect::from_min_max(pos2(bar_left, cy - 6.0), pos2(bar_right, cy + 6.0));
        ui.painter().rect_filled(track, radius(6), p.track);
        let fill_w = (track.width() * app.seconds as f32 / max).max(6.0);
        let fill = Rect::from_min_size(track.min, vec2(fill_w, track.height()));
        ui.painter().rect_filled(fill, radius(6), p.accent);

        ui.painter().text(
            pos2(rect.right(), cy),
            Align2::RIGHT_CENTER,
            short_duration(app.seconds),
            regular(14.0),
            p.text,
        );
        response.on_hover_text(format!("{} · {}", app.process_name, hms(app.seconds)));
    }
}

/// Colour per app for the timeline: the top apps get distinct colours.
pub fn app_colors(apps: &[UsageRow], palette: &[Color32]) -> HashMap<String, Color32> {
    apps.iter()
        .zip(palette)
        .map(|(app, color)| (app.process_name.clone(), *color))
        .collect()
}

/// A strip across the day showing which app was in front each minute.
pub fn timeline(
    ui: &mut Ui,
    p: &Palette,
    minutes: &[MinuteRow],
    colors: &HashMap<String, Color32>,
    legend: &[(String, Color32)],
) {
    // The app with the most seconds in each minute.
    let mut dominant: BTreeMap<u32, (&str, i64)> = BTreeMap::new();
    for row in minutes {
        let entry = dominant.entry(row.minute).or_insert((&row.process_name, 0));
        if row.seconds > entry.1 {
            *entry = (&row.process_name, row.seconds);
        }
    }

    let first_hour = dominant.keys().next().map(|m| m / 60).unwrap_or(8);
    let start_hour = first_hour.min(8) / 2 * 2;
    let start = start_hour * 60;
    let span = (24 - start_hour) as f32 * 60.0;

    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, 36.0), Sense::hover());
    let x_of =
        |minute: u32| rect.left() + (minute.saturating_sub(start)) as f32 / span * rect.width();

    ui.painter().rect_filled(rect, radius(6), p.track);
    let painter = ui.painter().with_clip_rect(rect.shrink(1.0));

    // Merge consecutive minutes of the same app into one segment.
    let mut run: Option<(u32, u32, &str)> = None;
    let paint_run = |run: (u32, u32, &str)| {
        let color = colors.get(run.2).copied().unwrap_or(OTHER_APP_COLOR);
        let x0 = x_of(run.0);
        let x1 = x_of(run.1).max(x0 + 1.5);
        painter.rect_filled(
            Rect::from_x_y_ranges(x0..=x1, rect.y_range()),
            CornerRadius::ZERO,
            color,
        );
    };
    for (&minute, &(name, _)) in &dominant {
        run = match run {
            Some((s, e, n)) if e == minute && n == name => Some((s, minute + 1, n)),
            Some(prev) => {
                paint_run(prev);
                Some((minute, minute + 1, name))
            }
            None => Some((minute, minute + 1, name)),
        };
    }
    if let Some(last) = run {
        paint_run(last);
    }

    let tip = response.hover_pos().map(|pos| {
        let minute = start + ((pos.x - rect.left()) / rect.width() * span) as u32;
        match dominant.get(&minute) {
            Some((name, _)) => format!("{} · {}", hm_clock(minute), name),
            None => format!("{} · kayıt yok", hm_clock(minute)),
        }
    });
    if let Some(tip) = tip {
        response.on_hover_text_at_pointer(tip);
    }

    // Hour labels every two hours.
    let (axis, _) = ui.allocate_exact_size(vec2(width, 22.0), Sense::hover());
    for hour in (start_hour..=24).step_by(2) {
        // The end labels hang inward so they stay inside the card.
        let align = match hour {
            h if h == start_hour => Align2::LEFT_TOP,
            24 => Align2::RIGHT_TOP,
            _ => Align2::CENTER_TOP,
        };
        ui.painter().text(
            pos2(x_of(hour * 60), axis.top() + 4.0),
            align,
            format!("{hour:02}:00"),
            regular(12.5),
            p.muted,
        );
    }

    if !legend.is_empty() {
        ui.add_space(8.0);
        let item_w = ((width - 16.0) / 3.0).floor();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(8.0, 4.0);
            for (name, color) in legend {
                let (r, _) = ui.allocate_exact_size(vec2(item_w, 26.0), Sense::hover());
                ui.painter()
                    .circle_filled(pos2(r.left() + 7.0, r.center().y), 6.0, *color);
                text_truncated(
                    ui,
                    pos2(r.left() + 20.0, r.center().y),
                    name,
                    regular(14.0),
                    p.text,
                    item_w - 24.0,
                );
            }
        });
    }
}

fn axis_step(max: i64) -> i64 {
    const STEPS: [i64; 8] = [300, 900, 1800, 3600, 7200, 10800, 14400, 21600];
    STEPS
        .iter()
        .copied()
        .find(|s| max <= s * 5)
        .unwrap_or(21600)
}

fn axis_label(seconds: i64) -> String {
    if seconds == 0 {
        "0".into()
    } else if seconds % 3600 == 0 {
        format!("{}s", seconds / 3600)
    } else {
        short_duration(seconds)
    }
}

/// One column per day. Returns the day that was clicked, if any.
pub fn day_columns(
    ui: &mut Ui,
    p: &Palette,
    days: &[(NaiveDate, i64)],
    selected: Option<NaiveDate>,
    today: NaiveDate,
) -> Option<NaiveDate> {
    if days.is_empty() {
        return None;
    }
    let (rect, response) =
        ui.allocate_exact_size(vec2(ui.available_width(), 250.0), Sense::click());
    let painter = ui.painter();

    let axis_w = 40.0;
    let plot = Rect::from_min_max(
        pos2(rect.left() + axis_w, rect.top() + 24.0),
        pos2(rect.right(), rect.bottom() - 28.0),
    );

    let max = days.iter().map(|d| d.1).max().unwrap_or(0);
    let step = axis_step(max);
    let steps = ((max + step - 1) / step).max(1);
    let y_max = (step * steps) as f32;

    for i in 0..=steps {
        let y = plot.bottom() - i as f32 / steps as f32 * plot.height();
        painter.line_segment(
            [pos2(plot.left(), y), pos2(plot.right(), y)],
            Stroke::new(1.0_f32, p.track),
        );
        painter.text(
            pos2(plot.left() - 8.0, y),
            Align2::RIGHT_CENTER,
            axis_label(step * i),
            regular(12.0),
            p.muted,
        );
    }

    let n = days.len();
    let slot = plot.width() / n as f32;
    let bar_w = (slot * 0.5).clamp(3.0, 42.0);
    let label_every = (34.0 / slot).ceil().max(1.0) as usize;
    let short_labels = n > 7;
    let selected_index = days.iter().position(|d| Some(d.0) == selected);

    for (i, &(date, seconds)) in days.iter().enumerate() {
        let cx = plot.left() + slot * (i as f32 + 0.5);
        let is_selected = selected == Some(date);

        if is_selected && n <= 7 {
            let outline = Rect::from_min_max(
                pos2(cx - slot * 0.46, plot.top() - 20.0),
                pos2(cx + slot * 0.46, rect.bottom()),
            );
            painter.rect_stroke(
                outline,
                radius(8),
                Stroke::new(1.5_f32, p.accent),
                StrokeKind::Inside,
            );
        }

        if date <= today && seconds > 0 {
            let h = (seconds as f32 / y_max * plot.height()).max(2.0);
            let bar = Rect::from_min_max(
                pos2(cx - bar_w / 2.0, plot.bottom() - h),
                pos2(cx + bar_w / 2.0, plot.bottom()),
            );
            let r = if h >= 6.0 { 4 } else { 0 };
            let corners = CornerRadius {
                nw: r,
                ne: r,
                sw: 0,
                se: 0,
            };
            painter.rect_filled(
                bar,
                corners,
                if is_selected {
                    p.accent
                } else {
                    p.bar_inactive
                },
            );

            if slot >= 56.0 {
                painter.text(
                    pos2(cx, bar.top() - 5.0),
                    Align2::CENTER_BOTTOM,
                    short_duration(seconds),
                    regular(12.5),
                    if is_selected { p.text } else { p.muted },
                );
            }
        }

        // Every n-th label, plus the selected day's, skipping ones that would
        // collide with it.
        let clear_of_selected = selected_index.is_none_or(|s| s.abs_diff(i) >= label_every);
        if is_selected || (i % label_every == 0 && clear_of_selected) {
            let label = if short_labels {
                date.day().to_string()
            } else {
                date.format("%d.%m").to_string()
            };
            let (font, color) = if is_selected {
                (semibold(13.0), p.accent)
            } else {
                (regular(13.0), p.muted)
            };
            painter.text(
                pos2(cx, plot.bottom() + 8.0),
                Align2::CENTER_TOP,
                label,
                font,
                color,
            );
        }
    }

    let index_at = |pos: Pos2| -> Option<usize> {
        if pos.x < plot.left() {
            return None;
        }
        let i = ((pos.x - plot.left()) / slot) as usize;
        (i < n).then_some(i)
    };

    let mut clicked = None;
    if response.clicked() {
        if let Some(i) = response.interact_pointer_pos().and_then(index_at) {
            if days[i].0 <= today {
                clicked = Some(days[i].0);
            }
        }
    }
    let tip = response.hover_pos().and_then(index_at).map(|i| {
        let (date, seconds) = days[i];
        format!("{} · {}", date.format("%d.%m.%Y"), short_duration(seconds))
    });
    if let Some(tip) = tip {
        response
            .on_hover_text_at_pointer(tip)
            .on_hover_cursor(egui::CursorIcon::PointingHand);
    }
    clicked
}

/// Live status colour for an app: `Some` draws a dot in the first column.
pub type StatusFn<'a> = &'a dyn Fn(&str) -> Option<Color32>;

pub fn usage_table(
    ui: &mut Ui,
    p: &Palette,
    icons: &HashMap<String, TextureId>,
    apps: &[UsageRow],
    status: Option<StatusFn>,
) {
    let width = ui.available_width();
    // Without live status (past periods) the "Durum" column is left out.
    let app_x = if status.is_some() { 70.0 } else { 0.0 };
    let name_x = app_x + 38.0;
    let duration_w = 110.0;
    ui.spacing_mut().item_spacing.y = 0.0;

    let (header, _) = ui.allocate_exact_size(vec2(width, 30.0), Sense::hover());
    let painter = ui.painter();
    if status.is_some() {
        painter.text(
            pos2(header.left(), header.center().y),
            Align2::LEFT_CENTER,
            "Durum",
            regular(13.5),
            p.muted,
        );
    }
    painter.text(
        pos2(header.left() + app_x, header.center().y),
        Align2::LEFT_CENTER,
        "Uygulama",
        regular(13.5),
        p.muted,
    );
    painter.text(
        pos2(header.right(), header.center().y),
        Align2::RIGHT_CENTER,
        "Süre",
        regular(13.5),
        p.muted,
    );
    painter.line_segment(
        [header.left_bottom(), header.right_bottom()],
        Stroke::new(1.0_f32, p.track),
    );

    for app in apps {
        let (row, response) = ui.allocate_exact_size(vec2(width, 40.0), Sense::hover());
        let cy = row.center().y;
        if response.hovered() {
            ui.painter()
                .rect_filled(row, radius(6), p.track.gamma_multiply(0.5));
        }
        if let Some(color) = status.and_then(|s| s(&app.process_name)) {
            ui.painter()
                .circle_filled(pos2(row.left() + 16.0, cy), 5.5, color);
        }
        let icon_rect =
            Rect::from_center_size(pos2(row.left() + app_x + 13.0, cy), vec2(24.0, 24.0));
        paint_app_icon(
            ui,
            p,
            icons.get(&app.process_name).copied(),
            &app.process_name,
            icon_rect,
        );
        text_truncated(
            ui,
            pos2(row.left() + name_x, cy),
            &app.process_name,
            regular(14.5),
            p.text,
            width - name_x - duration_w,
        );
        ui.painter().text(
            pos2(row.right(), cy),
            Align2::RIGHT_CENTER,
            hms(app.seconds),
            regular(14.5),
            p.text,
        );
        ui.painter().line_segment(
            [row.left_bottom(), row.right_bottom()],
            Stroke::new(1.0_f32, p.track),
        );
    }
}

pub fn nav_item(ui: &mut Ui, p: &Palette, icon: &str, label: &str, selected: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 46.0), Sense::click());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, radius(10), p.nav_selected);
        let bar = Rect::from_min_size(rect.min + vec2(0.0, 8.0), vec2(3.5, rect.height() - 16.0));
        painter.rect_filled(bar, radius(2), p.accent);
    } else if response.hovered() {
        painter.rect_filled(rect, radius(10), p.track.gamma_multiply(0.6));
    }
    let icon_color = if selected { p.accent } else { p.muted };
    painter.text(
        pos2(rect.left() + 26.0, rect.center().y),
        Align2::CENTER_CENTER,
        icon,
        regular(21.0),
        icon_color,
    );
    let font = if selected {
        semibold(15.5)
    } else {
        regular(15.5)
    };
    painter.text(
        pos2(rect.left() + 52.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        font,
        p.text,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn status_pill(ui: &mut Ui, p: &Palette, text: &str, color: Color32) -> Response {
    // Painted by hand so it sizes to its text in any parent layout.
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), regular(14.0), p.text);
    let (rect, response) =
        ui.allocate_exact_size(vec2(galley.size().x + 46.0, 34.0), Sense::click());
    let [r, g, b, _] = color.to_array();
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        radius(17),
        Color32::from_rgba_unmultiplied(r, g, b, 34),
    );
    painter.rect_stroke(
        rect,
        radius(17),
        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(r, g, b, 90)),
        StrokeKind::Inside,
    );
    painter.circle_filled(pos2(rect.left() + 19.0, rect.center().y), 4.5, color);
    let text_pos = pos2(rect.left() + 32.0, rect.center().y - galley.size().y / 2.0);
    painter.galley(text_pos, galley, p.text);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A month grid; `month` is any day of the shown month and is moved by the
/// arrows. Returns the picked day.
pub fn calendar(
    ui: &mut Ui,
    p: &Palette,
    month: &mut NaiveDate,
    selected: NaiveDate,
    today: NaiveDate,
) -> Option<NaiveDate> {
    let first = month.with_day(1).unwrap_or(*month);
    let cell = vec2(38.0, 32.0);
    let grid_w = cell.x * 7.0;
    let mut picked = None;

    ui.allocate_ui(vec2(grid_w, 30.0), |ui| {
        ui.set_width(grid_w);
        ui.horizontal(|ui| {
            if ui
                .add(egui::Button::new(egui_phosphor::regular::CARET_LEFT).frame(false))
                .clicked()
            {
                *month = first.checked_sub_months(Months::new(1)).unwrap_or(first);
            }
            let title = format!("{} {}", month_name(first.month()), first.year());
            let label_w = grid_w - 2.0 * 34.0 - 2.0 * ui.spacing().item_spacing.x;
            ui.add_sized(
                vec2(label_w, 24.0),
                egui::Label::new(egui::RichText::new(title).font(semibold(15.0))),
            );
            let next = first.checked_add_months(Months::new(1)).unwrap_or(first);
            if ui
                .add_enabled(
                    next <= today,
                    egui::Button::new(egui_phosphor::regular::CARET_RIGHT).frame(false),
                )
                .clicked()
            {
                *month = next;
            }
        });
    });

    let (head, _) = ui.allocate_exact_size(vec2(grid_w, 24.0), Sense::hover());
    for (i, name) in ["Pt", "Sa", "Ça", "Pe", "Cu", "Ct", "Pz"]
        .iter()
        .enumerate()
    {
        let x = head.left() + cell.x * (i as f32 + 0.5);
        ui.painter().text(
            pos2(x, head.center().y),
            Align2::CENTER_CENTER,
            *name,
            regular(12.5),
            p.muted,
        );
    }

    let offset = first.weekday().num_days_from_monday() as u64;
    let start = first - Days::new(offset);
    for week in 0..6 {
        let (row, _) = ui.allocate_exact_size(vec2(grid_w, cell.y), Sense::hover());
        for weekday in 0..7 {
            let date = start + Days::new(week * 7 + weekday);
            if date.month() != first.month() {
                continue;
            }
            let rect =
                Rect::from_min_size(row.min + vec2(cell.x * weekday as f32, 0.0), cell).shrink(2.0);
            let enabled = date <= today;
            let response = ui.interact(
                rect,
                ui.id().with(("day", date)),
                if enabled {
                    Sense::click()
                } else {
                    Sense::hover()
                },
            );

            let painter = ui.painter();
            if date == selected {
                painter.rect_filled(rect, radius(8), p.accent);
            } else if response.hovered() && enabled {
                painter.rect_filled(rect, radius(8), p.track);
            }
            if date == today && date != selected {
                painter.rect_stroke(
                    rect,
                    radius(8),
                    Stroke::new(1.0_f32, p.accent),
                    StrokeKind::Inside,
                );
            }
            let color = if date == selected {
                Color32::WHITE
            } else if enabled {
                p.text
            } else {
                p.muted.gamma_multiply(0.5)
            };
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                date.day().to_string(),
                regular(14.0),
                color,
            );
            if response.clicked() {
                picked = Some(date);
            }
        }
    }
    picked
}

/// "5s 32dk", "48dk", "35sn" — compact durations for cards and charts.
pub fn short_duration(seconds: i64) -> String {
    let h = seconds / 3600;
    let m = (seconds % 3600) / 60;
    if h > 0 {
        format!("{h}s {m}dk")
    } else if m > 0 {
        format!("{m}dk")
    } else if seconds > 0 {
        format!("{seconds}sn")
    } else {
        "0dk".into()
    }
}

/// "01:48:12"
pub fn hms(seconds: i64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        (seconds % 3600) / 60,
        seconds % 60
    )
}

/// Minute of the day as "14:05".
pub fn hm_clock(minute: u32) -> String {
    format!("{:02}:{:02}", minute / 60, minute % 60)
}

pub fn month_name(month: u32) -> &'static str {
    const NAMES: [&str; 12] = [
        "Ocak", "Şubat", "Mart", "Nisan", "Mayıs", "Haziran", "Temmuz", "Ağustos", "Eylül", "Ekim",
        "Kasım", "Aralık",
    ];
    NAMES[(month.clamp(1, 12) - 1) as usize]
}

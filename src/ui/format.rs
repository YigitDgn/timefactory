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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_duration_cases() {
        assert_eq!(short_duration(0), "0dk");
        assert_eq!(short_duration(35), "35sn");
        assert_eq!(short_duration(60), "1dk");
        assert_eq!(short_duration(48 * 60 + 59), "48dk");
        assert_eq!(short_duration(3600), "1s 0dk");
        assert_eq!(short_duration(5 * 3600 + 32 * 60 + 10), "5s 32dk");
    }

    #[test]
    fn hms_cases() {
        assert_eq!(hms(0), "00:00:00");
        assert_eq!(hms(65), "00:01:05");
        assert_eq!(hms(3600 + 48 * 60 + 12), "01:48:12");
        assert_eq!(hms(25 * 3600), "25:00:00");
    }

    #[test]
    fn hm_clock_cases() {
        assert_eq!(hm_clock(0), "00:00");
        assert_eq!(hm_clock(14 * 60 + 5), "14:05");
        assert_eq!(hm_clock(23 * 60 + 59), "23:59");
    }

    #[test]
    fn month_name_maps_and_clamps() {
        assert_eq!(month_name(0), "Ocak");
        assert_eq!(month_name(1), "Ocak");
        assert_eq!(month_name(2), "Şubat");
        assert_eq!(month_name(11), "Kasım");
        assert_eq!(month_name(12), "Aralık");
        assert_eq!(month_name(99), "Aralık");
    }
}

//! Small clock helpers, so the UI shows readable times without a date crate.

use web_time::{SystemTime, UNIX_EPOCH};

/// Unix seconds now. A clock before 1970 reads as zero.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs().min(i64::MAX as u64) as i64)
        .unwrap_or(0)
}

/// A short, human date for a note card.
///
/// Recent edits read as an interval, older ones as a day from the calendar.
pub fn relative_time(now: i64, timestamp: i64) -> String {
    if timestamp <= 0 {
        return String::new();
    }
    let delta = now.saturating_sub(timestamp);
    if delta < 60 {
        return "just now".to_string();
    }
    if delta < 3_600 {
        return format!("{} min ago", delta / 60);
    }
    if delta < 86_400 {
        let hours = delta / 3_600;
        return if hours == 1 {
            "1 hour ago".to_string()
        } else {
            format!("{hours} hours ago")
        };
    }
    if delta < 172_800 {
        return "yesterday".to_string();
    }
    if delta < 604_800 {
        return format!("{} days ago", delta / 86_400);
    }
    let (year, month, day) = civil_from_unix(timestamp);
    let month = MONTHS
        .get(month.saturating_sub(1) as usize)
        .copied()
        .unwrap_or("?");
    let (today_year, _, _) = civil_from_unix(now);
    if year == today_year {
        format!("{month} {day}")
    } else {
        format!("{month} {day}, {year}")
    }
}

/// `2026-10-20` for a Unix timestamp, used in export file names.
pub fn iso_date(timestamp: i64) -> String {
    let (year, month, day) = civil_from_unix(timestamp);
    format!("{year:04}-{month:02}-{day:02}")
}

/// A calendar day for a history row: Today, Yesterday, or the date.
pub fn day_label(now: i64, timestamp: i64) -> String {
    if timestamp <= 0 {
        return "Unknown time".to_string();
    }
    let (year, month, day) = civil_from_unix(timestamp);
    let (today_year, today_month, today_day) = civil_from_unix(now);
    if (year, month, day) == (today_year, today_month, today_day) {
        return "Today".to_string();
    }
    let (yesterday_year, yesterday_month, yesterday_day) = civil_from_unix(now - 86_400);
    if (year, month, day) == (yesterday_year, yesterday_month, yesterday_day) {
        return "Yesterday".to_string();
    }
    let month = MONTHS
        .get(month.saturating_sub(1) as usize)
        .copied()
        .unwrap_or("?");
    if year == today_year {
        format!("{month} {day}")
    } else {
        format!("{month} {day}, {year}")
    }
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// Convert Unix seconds to a proleptic Gregorian date, in UTC.
///
/// Howard Hinnant's `civil_from_days`, with the epoch shift folded in. Kept
/// here so the UI does not pull a calendar crate into the wasm bundle.
fn civil_from_unix(timestamp: i64) -> (i64, u32, u32) {
    let days = timestamp.div_euclid(86_400);
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year, m as u32, d as u32)
}

#[cfg(test)]
mod tests {
    use super::{civil_from_unix, day_label, iso_date, relative_time};

    #[test]
    fn civil_dates_match_known_days() {
        assert_eq!(civil_from_unix(0), (1970, 1, 1));
        assert_eq!(civil_from_unix(1_700_000_000), (2023, 11, 14));
        assert_eq!(iso_date(1_728_000_000), "2024-10-04");
    }

    #[test]
    fn recent_edits_read_as_an_interval() {
        let now = 1_700_000_000;
        assert_eq!(relative_time(now, now - 5), "just now");
        assert_eq!(relative_time(now, now - 120), "2 min ago");
        assert_eq!(relative_time(now, now - 7_200), "2 hours ago");
        assert_eq!(relative_time(now, now - 90_000), "yesterday");
    }

    #[test]
    fn an_empty_timestamp_has_no_label() {
        assert_eq!(relative_time(100, 0), "");
    }

    #[test]
    fn history_days_read_as_today_yesterday_or_a_date() {
        // 2026-10-20 12:00 UTC.
        let now = 1_792_497_600;
        assert_eq!(day_label(now, now - 60), "Today");
        assert_eq!(day_label(now, now - 86_400), "Yesterday");
        assert_eq!(day_label(now, now - 3 * 86_400), "Oct 17");
        assert_eq!(day_label(now, 1_700_000_000), "Nov 14, 2023");
        assert_eq!(day_label(now, 0), "Unknown time");
    }
}

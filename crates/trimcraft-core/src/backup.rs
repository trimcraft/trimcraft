//! Automatic backups of setup.json: file names, which backups to keep, and what the list shows.
//!
//! The app writes the files; everything that can be decided without the disk is here. Times
//! are UTC seconds since the Unix epoch, so names and retention do not depend on the time zone;
//! the UI shows them in local time.

use serde::Serialize;
use std::collections::HashSet;

/// Backups are written at most this often (seconds), unless one is asked for.
pub const MIN_INTERVAL_SECS: u64 = 5 * 60;
/// Retention: the newest this many backups are kept ...
pub const KEEP_LAST: usize = 20;
/// ... and the newest backup of each of the last this many days.
pub const KEEP_DAYS: u64 = 14;

const DAY: u64 = 86_400;
const PREFIX: &str = "setup-";
const SUFFIX: &str = ".json";

/// A backup as the Settings page lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    /// File name; also the id for restoring.
    pub name: String,
    /// When the backup was written (UTC seconds), from its name.
    pub unix_secs: u64,
    pub size: u64,
    /// Number of profiles in it; `None` if the file cannot be read.
    pub profiles: Option<usize>,
}

/// Days since 1970-01-01 of a civil date (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Civil date `(year, month, day)` of a day count since 1970-01-01.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

fn split(secs: u64) -> (i64, i64, i64, u64, u64, u64) {
    let days = i64::try_from(secs / DAY).unwrap_or(0);
    let (y, m, d) = civil_from_days(days);
    let rest = secs % DAY;
    (y, m, d, rest / 3600, rest % 3600 / 60, rest % 60)
}

/// `YYYYMMDD-HHMMSS` (UTC).
pub fn stamp(secs: u64) -> String {
    let (y, m, d, h, min, s) = split(secs);
    format!("{y:04}{m:02}{d:02}-{h:02}{min:02}{s:02}")
}

/// `2026-10-13T12:00:00Z`.
pub fn iso(secs: u64) -> String {
    let (y, m, d, h, min, s) = split(secs);
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{min:02}:{s:02}Z")
}

/// `setup-YYYYMMDD-HHMMSS.json`.
pub fn file_name(secs: u64) -> String {
    format!("{PREFIX}{}{SUFFIX}", stamp(secs))
}

/// The time in a backup file name; `None` for any other name. This is also what keeps a
/// restore request from naming a file outside the backup folder.
pub fn parse_name(name: &str) -> Option<u64> {
    let s = name.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?;
    let (date, time) = s.split_once('-')?;
    if date.len() != 8 || time.len() != 6 || !s.bytes().all(|b| b.is_ascii_digit() || b == b'-') {
        return None;
    }
    let n = |range: std::ops::Range<usize>, text: &str| text[range].parse::<i64>().ok();
    let (y, m, d) = (n(0..4, date)?, n(4..6, date)?, n(6..8, date)?);
    let (h, min, sec) = (n(0..2, time)?, n(2..4, time)?, n(4..6, time)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || h > 23 || min > 59 || sec > 59 {
        return None;
    }
    let days = days_from_civil(y, m, d);
    u64::try_from(days * 86_400 + h * 3600 + min * 60 + sec).ok()
}

/// Which of `backups` (name, time) to delete: everything except the newest [`KEEP_LAST`] and the
/// newest of each of the last [`KEEP_DAYS`] days (counting today).
pub fn to_prune(backups: &[(String, u64)], now_secs: u64) -> Vec<String> {
    let mut sorted: Vec<&(String, u64)> = backups.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| b.0.cmp(&a.0)));
    let mut keep: HashSet<&str> = sorted
        .iter()
        .take(KEEP_LAST)
        .map(|(n, _)| n.as_str())
        .collect();
    let today = now_secs / DAY;
    for age in 0..KEEP_DAYS {
        let Some(day) = today.checked_sub(age) else {
            break;
        };
        if let Some((name, _)) = sorted.iter().find(|(_, t)| t / DAY == day) {
            keep.insert(name.as_str());
        }
    }
    sorted
        .iter()
        .filter(|(n, _)| !keep.contains(n.as_str()))
        .map(|(n, _)| n.clone())
        .collect()
}

/// Number of profiles in a setup file (1 for files from before profiles); `None` if it is not a
/// setup.
pub fn profile_count(json: &str) -> Option<usize> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let obj = value.as_object()?;
    Some(
        obj.get("profiles")
            .and_then(|p| p.as_array())
            .map_or(1, Vec::len),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-13 12:00:00 UTC.
    const NOW: u64 = 1_791_892_800;

    #[test]
    fn names_round_trip() {
        assert_eq!(stamp(0), "19700101-000000");
        assert_eq!(stamp(NOW), "20261013-120000");
        assert_eq!(iso(NOW), "2026-10-13T12:00:00Z");
        assert_eq!(file_name(NOW), "setup-20261013-120000.json");
        assert_eq!(parse_name("setup-20261013-120000.json"), Some(NOW));
        // Leap day and a year boundary.
        for t in [951_782_400, 1_709_208_000, 1_767_225_599, 4_102_444_800] {
            assert_eq!(parse_name(&file_name(t)), Some(t), "{t}");
        }
    }

    #[test]
    fn other_names_are_rejected() {
        for name in [
            "setup.json",
            "setup-20261013-120000.json.tmp",
            "../setup-20261013-120000.json",
            "setup-2026101-1200000.json",
            "setup-20261313-120000.json",
            "setup-20261013-250000.json",
            "setup-abcdefgh-120000.json",
            "setup-20261013/120000.json",
        ] {
            assert_eq!(parse_name(name), None, "{name}");
        }
    }

    fn at(day_offset: u64, secs_in_day: u64) -> (String, u64) {
        let t = NOW / DAY * DAY - day_offset * DAY + secs_in_day;
        (file_name(t), t)
    }

    #[test]
    fn few_backups_are_all_kept() {
        let list: Vec<_> = (0..5).map(|i| at(0, i * 100)).collect();
        assert!(to_prune(&list, NOW).is_empty());
    }

    #[test]
    fn the_newest_twenty_are_kept_whatever_their_age() {
        // 30 backups an hour apart, today (the last 12 hours) and before.
        let list: Vec<_> = (0..30)
            .map(|i| (file_name(NOW - i * 3600), NOW - i * 3600))
            .collect();
        let pruned = to_prune(&list, NOW);
        // Keep the newest 20 plus the newest of each day not already covered.
        assert!(!pruned.contains(&list[0].0));
        assert!(!pruned.contains(&list[19].0));
        assert!(
            pruned
                .iter()
                .all(|n| list[20..].iter().any(|(m, _)| m == n))
        );
        // Entries 20..30 are on yesterday's date; the newest of that day is among the first 20,
        // so exactly those ten older ones go.
        assert_eq!(pruned.len(), 10);
    }

    #[test]
    fn one_per_day_for_two_weeks_then_nothing() {
        // Three backups per day for 20 days back.
        let mut list = Vec::new();
        for day in 0..20 {
            for k in 0..3 {
                list.push(at(day, 1000 + k * 7000));
            }
        }
        let pruned = to_prune(&list, NOW);
        let kept: Vec<_> = list.iter().filter(|(n, _)| !pruned.contains(n)).collect();
        // The newest 20 (the last 7 days) and the newest of each of the 14 days.
        for day in 0..14u64 {
            let newest = list
                .iter()
                .filter(|(_, t)| t / DAY == NOW / DAY - day)
                .max_by_key(|(_, t)| *t)
                .unwrap();
            assert!(kept.contains(&newest), "day {day}");
        }
        // Days older than 14 are gone unless among the newest 20 (they are not).
        assert!(
            kept.iter().all(|(_, t)| *t / DAY + KEEP_DAYS > NOW / DAY),
            "{kept:?}"
        );
        assert!(kept.len() >= 20 && kept.len() < 40);
    }

    #[test]
    fn profile_count_of_setup_files() {
        assert_eq!(profile_count(r#"{"profiles":[{},{}]}"#), Some(2));
        assert_eq!(profile_count(r#"{"virtualControls":[]}"#), Some(1));
        assert_eq!(profile_count("nonsense"), None);
        assert_eq!(profile_count("[1]"), None);
    }

    #[test]
    fn info_serialises_for_the_ui() {
        let info = BackupInfo {
            name: "setup-20261013-120000.json".into(),
            unix_secs: NOW,
            size: 12,
            profiles: Some(3),
        };
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["unixSecs"], NOW);
        assert_eq!(json["profiles"], 3);
    }
}

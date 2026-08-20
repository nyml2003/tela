//! Wall-clock time conversion: unix milliseconds + timezone offset to civil local time.
//!
//! Zero-dependency civil date arithmetic (Howard Hinnant's `civil_from_days` algorithm).
//! This is a pure conversion utility; it does not resolve IANA timezone rules.

/// A civil (calendar) local time produced by [`civil_from_unix_millis`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CivilDateTime {
    /// Year (proleptic Gregorian, can be negative for BC).
    pub year: i64,
    /// Month, 1-12.
    pub month: u32,
    /// Day of month, 1-31.
    pub day: u32,
    /// Hour, 0-23.
    pub hour: u32,
    /// Minute, 0-59.
    pub minute: u32,
    /// Second, 0-59.
    pub second: u32,
}

/// Converts a unix millisecond timestamp plus a timezone offset (seconds, DST-aware as reported
/// by the host) into a civil local time.
///
/// # Examples
///
/// ```
/// use tela_utils::time::{civil_from_unix_millis, CivilDateTime};
/// // 1970-01-01T00:00:00Z in UTC.
/// assert_eq!(
///     civil_from_unix_millis(0, 0),
///     CivilDateTime { year: 1970, month: 1, day: 1, hour: 0, minute: 0, second: 0 }
/// );
/// // Same instant in UTC+8.
/// assert_eq!(
///     civil_from_unix_millis(0, 28_800),
///     CivilDateTime { year: 1970, month: 1, day: 1, hour: 8, minute: 0, second: 0 }
/// );
/// ```
pub fn civil_from_unix_millis(unix_millis: u64, timezone_offset_seconds: i32) -> CivilDateTime {
    let total_seconds = (unix_millis as i64) / 1000 + i64::from(timezone_offset_seconds);
    let days = total_seconds.div_euclid(86_400);
    let seconds_of_day = total_seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    CivilDateTime {
        year,
        month,
        day,
        hour: (seconds_of_day / 3_600) as u32,
        minute: ((seconds_of_day % 3_600) / 60) as u32,
        second: (seconds_of_day % 60) as u32,
    }
}

/// Inverse of Howard Hinnant's `days_from_civil`: days since 1970-01-01 to a civil date.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let shifted = z + 719_468;
    let era = if shifted >= 0 {
        shifted / 146_097
    } else {
        (shifted - 146_096) / 146_097
    };
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    let year = if month <= 2 { year + 1 } else { year };
    (year, month as u32, day as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_in_utc_and_positive_offset() {
        let utc = civil_from_unix_millis(0, 0);
        assert_eq!(utc.year, 1970);
        assert_eq!(utc.month, 1);
        assert_eq!(utc.day, 1);
        assert_eq!((utc.hour, utc.minute, utc.second), (0, 0, 0));

        let beijing = civil_from_unix_millis(0, 28_800);
        assert_eq!((beijing.hour, beijing.minute), (8, 0));
    }

    #[test]
    fn negative_offset_crosses_day_boundary() {
        // 1970-01-01T00:30:00Z in UTC-1 is 1969-12-31T23:30:00.
        let western = civil_from_unix_millis(30 * 60 * 1000, -3_600);
        assert_eq!((western.year, western.month, western.day), (1969, 12, 31));
        assert_eq!((western.hour, western.minute), (23, 30));
    }

    #[test]
    fn leap_year_february_29() {
        // 2024-02-29T00:00:00Z = unix 1709164800000.
        let leap = civil_from_unix_millis(1_709_164_800_000, 0);
        assert_eq!((leap.year, leap.month, leap.day), (2024, 2, 29));
    }

    #[test]
    fn midnight_rollover_with_dst_like_offset() {
        // 2026-08-20T16:30:00Z in UTC+8 = 2026-08-21T00:30:00.
        let ts = 1_787_243_400_000u64;
        let local = civil_from_unix_millis(ts, 28_800);
        assert_eq!((local.year, local.month, local.day), (2026, 8, 21));
        assert_eq!((local.hour, local.minute), (0, 30));
    }
}

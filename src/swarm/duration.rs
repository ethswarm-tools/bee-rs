//! Bee-flavored [`Duration`] type. Mirrors bee-go's
//! `pkg/swarm/duration.go` and bee-js `Duration`.
//!
//! Non-negative whole-second duration. Negative inputs clamp to
//! zero, fractional seconds round up. The type wraps an `i64`
//! seconds count and is `Copy`. Use [`Duration::to_std`] to convert
//! to [`std::time::Duration`].

use std::fmt;
use std::str::FromStr;

use crate::swarm::Error;

/// Non-negative whole-second duration.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Duration {
    seconds: i64,
}

const SECONDS_IN_MINUTE: i64 = 60;
const SECONDS_IN_HOUR: i64 = 60 * SECONDS_IN_MINUTE;
const SECONDS_IN_DAY: i64 = 24 * SECONDS_IN_HOUR;
const SECONDS_IN_WEEK: i64 = 7 * SECONDS_IN_DAY;
const SECONDS_IN_MONTH: i64 = 30 * SECONDS_IN_DAY;
const SECONDS_IN_YEAR: i64 = 365 * SECONDS_IN_DAY;

impl Duration {
    /// Zero-length duration.
    pub const ZERO: Duration = Duration { seconds: 0 };

    /// Build from whole seconds (rounds up if fractional, clamps
    /// negatives / NaN to zero).
    pub fn from_seconds(s: f64) -> Self {
        Self::new(s)
    }

    /// Build from milliseconds.
    pub fn from_milliseconds(ms: f64) -> Self {
        Self::new(ms / 1000.0)
    }

    /// Build from minutes.
    pub fn from_minutes(m: f64) -> Self {
        Self::new(m * SECONDS_IN_MINUTE as f64)
    }

    /// Build from hours.
    pub fn from_hours(h: f64) -> Self {
        Self::new(h * SECONDS_IN_HOUR as f64)
    }

    /// Build from days.
    pub fn from_days(d: f64) -> Self {
        Self::new(d * SECONDS_IN_DAY as f64)
    }

    /// Build from weeks.
    pub fn from_weeks(w: f64) -> Self {
        Self::new(w * SECONDS_IN_WEEK as f64)
    }

    /// Build from 30-day months.
    pub fn from_months(m: f64) -> Self {
        Self::new(m * SECONDS_IN_MONTH as f64)
    }

    /// Build from 365-day years.
    pub fn from_years(y: f64) -> Self {
        Self::new(y * SECONDS_IN_YEAR as f64)
    }

    /// Build from a [`std::time::Duration`].
    pub fn from_std(d: std::time::Duration) -> Self {
        Self::new(d.as_secs_f64())
    }

    /// Parse strings like `"1.5h"`, `"5 d"`, `"2weeks"`, `"30s"`,
    /// `"1d 4h 5m 30s"`. Case-insensitive, whitespace-tolerant.
    /// Supported unit families: `ms` / `s` / `m` / `h` / `d` / `w` /
    /// `month` / `y`.
    pub fn parse(s: &str) -> Result<Self, Error> {
        <Self as FromStr>::from_str(s)
    }

    /// Whole-second count.
    pub const fn to_seconds(self) -> i64 {
        self.seconds
    }

    /// Milliseconds (whole-second precision).
    pub const fn to_milliseconds(self) -> i64 {
        self.seconds * 1000
    }

    /// Fractional minutes.
    pub fn to_minutes(self) -> f64 {
        self.seconds as f64 / SECONDS_IN_MINUTE as f64
    }

    /// Fractional hours.
    pub fn to_hours(self) -> f64 {
        self.seconds as f64 / SECONDS_IN_HOUR as f64
    }

    /// Fractional days.
    pub fn to_days(self) -> f64 {
        self.seconds as f64 / SECONDS_IN_DAY as f64
    }

    /// Fractional weeks.
    pub fn to_weeks(self) -> f64 {
        self.seconds as f64 / SECONDS_IN_WEEK as f64
    }

    /// Fractional 365-day years.
    pub fn to_years(self) -> f64 {
        self.seconds as f64 / SECONDS_IN_YEAR as f64
    }

    /// Convert to a [`std::time::Duration`].
    pub fn to_std(self) -> std::time::Duration {
        std::time::Duration::from_secs(self.seconds.max(0) as u64)
    }

    /// True iff the duration is zero.
    pub const fn is_zero(self) -> bool {
        self.seconds == 0
    }

    fn new(seconds: f64) -> Self {
        if seconds.is_nan() || seconds < 0.0 {
            return Self::ZERO;
        }
        Self {
            seconds: seconds.ceil() as i64,
        }
    }
}

impl fmt::Display for Duration {
    /// Render as "1y 4w 2d 3h 5m 30s" (only non-zero parts; zero
    /// renders as `"0s"`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.seconds == 0 {
            return f.write_str("0s");
        }
        let parts: [(&str, i64); 6] = [
            ("y", SECONDS_IN_YEAR),
            ("w", SECONDS_IN_WEEK),
            ("d", SECONDS_IN_DAY),
            ("h", SECONDS_IN_HOUR),
            ("m", SECONDS_IN_MINUTE),
            ("s", 1),
        ];
        let mut remaining = self.seconds;
        let mut wrote_any = false;
        for (unit, size) in parts {
            if remaining >= size {
                let n = remaining / size;
                remaining -= n * size;
                if wrote_any {
                    f.write_str(" ")?;
                }
                write!(f, "{n}{unit}")?;
                wrote_any = true;
            }
        }
        Ok(())
    }
}

impl FromStr for Duration {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        let lower = clean.to_ascii_lowercase();
        if lower.is_empty() {
            return Err(Error::argument("empty duration string"));
        }
        let mut total: f64 = 0.0;
        let mut chars = lower.chars().peekable();
        let mut found = false;
        while chars.peek().is_some() {
            let mut num = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_ascii_digit() || c == '.' {
                    num.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            if num.is_empty() {
                return Err(Error::argument(format!(
                    "unrecognized duration string: {s}"
                )));
            }
            let value: f64 = num
                .parse()
                .map_err(|_| Error::argument(format!("invalid duration number: {num}")))?;

            let mut unit = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_ascii_alphabetic() {
                    unit.push(c);
                    chars.next();
                } else {
                    break;
                }
            }
            if unit.is_empty() {
                return Err(Error::argument(format!("missing unit in: {s}")));
            }
            total += value * unit_to_seconds(&unit)?;
            found = true;
        }
        if !found {
            return Err(Error::argument(format!(
                "unrecognized duration string: {s}"
            )));
        }
        Ok(Self::new(total))
    }
}

fn unit_to_seconds(unit: &str) -> Result<f64, Error> {
    Ok(match unit {
        "ms" | "milli" | "millis" | "millisecond" | "milliseconds" => 0.001,
        "s" | "sec" | "second" | "seconds" => 1.0,
        "m" | "min" | "minute" | "minutes" => SECONDS_IN_MINUTE as f64,
        "h" | "hour" | "hours" => SECONDS_IN_HOUR as f64,
        "d" | "day" | "days" => SECONDS_IN_DAY as f64,
        "w" | "week" | "weeks" => SECONDS_IN_WEEK as f64,
        "month" | "months" => SECONDS_IN_MONTH as f64,
        "y" | "year" | "years" => SECONDS_IN_YEAR as f64,
        other => {
            return Err(Error::argument(format!(
                "unsupported duration unit: {other}"
            )));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn negative_or_nan_clamps_to_zero() {
        assert_eq!(Duration::from_seconds(-1.0), Duration::ZERO);
        assert_eq!(Duration::from_seconds(f64::NAN), Duration::ZERO);
    }

    #[test]
    fn fractional_seconds_round_up() {
        assert_eq!(Duration::from_seconds(0.1).to_seconds(), 1);
        assert_eq!(Duration::from_milliseconds(1500.0).to_seconds(), 2);
    }

    #[test]
    fn unit_constructors_match_seconds() {
        assert_eq!(Duration::from_minutes(1.0).to_seconds(), 60);
        assert_eq!(Duration::from_hours(1.0).to_seconds(), 3600);
        assert_eq!(Duration::from_days(1.0).to_seconds(), 86_400);
        assert_eq!(Duration::from_weeks(1.0).to_seconds(), 7 * 86_400);
        assert_eq!(Duration::from_years(1.0).to_seconds(), 365 * 86_400);
    }

    #[test]
    fn parse_compound_string() {
        let d = Duration::parse("1d 4h 5m 30s").unwrap();
        let want = SECONDS_IN_DAY + 4 * SECONDS_IN_HOUR + 5 * SECONDS_IN_MINUTE + 30;
        assert_eq!(d.to_seconds(), want);
    }

    #[test]
    fn parse_decimal_hours() {
        let d = Duration::parse("1.5h").unwrap();
        assert_eq!(d.to_seconds(), 5400);
    }

    #[test]
    fn parse_handles_whitespace_and_case() {
        let d = Duration::parse("  2 Weeks  ").unwrap();
        assert_eq!(d.to_seconds(), 14 * SECONDS_IN_DAY);
    }

    #[test]
    fn parse_milliseconds() {
        // 1500 ms → 2s after ceil rounding.
        let d = Duration::parse("1500ms").unwrap();
        assert_eq!(d.to_seconds(), 2);
    }

    #[test]
    fn parse_rejects_empty() {
        assert!(Duration::parse("").is_err());
        assert!(Duration::parse("   ").is_err());
    }

    #[test]
    fn parse_rejects_unknown_unit() {
        assert!(Duration::parse("3decades").is_err());
    }

    #[test]
    fn display_decomposes_into_units() {
        assert_eq!(Duration::ZERO.to_string(), "0s");
        let d = Duration::from_seconds((SECONDS_IN_DAY + 4 * SECONDS_IN_HOUR + 5) as f64);
        assert_eq!(d.to_string(), "1d 4h 5s");
    }

    #[test]
    fn round_trip_through_std() {
        let d = Duration::from_minutes(2.5);
        let std = d.to_std();
        let back = Duration::from_std(std);
        assert_eq!(d, back);
    }
}

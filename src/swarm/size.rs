//! Decimal-base file size type. Mirrors bee-go's `pkg/swarm/size.go`
//! (and bee-js `Size`): all conversions use 1000 as the base so they
//! stay aligned with the Swarm theoretical/effective storage tables.

use std::fmt;
use std::str::FromStr;

use crate::swarm::Error;

/// Non-negative size in bytes. Constructors round fractional inputs
/// up; negative or NaN inputs return [`Error::Argument`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Size {
    bytes: i64,
}

const KB: i64 = 1_000;
const MB: i64 = KB * 1_000;
const GB: i64 = MB * 1_000;
const TB: i64 = GB * 1_000;

impl Size {
    /// Build from a byte count (rounded up if fractional).
    pub fn from_bytes(b: f64) -> Result<Self, Error> {
        Self::new(b)
    }

    /// Build from kilobytes (1 kB = 1000 B).
    pub fn from_kilobytes(k: f64) -> Result<Self, Error> {
        Self::new(k * KB as f64)
    }

    /// Build from megabytes.
    pub fn from_megabytes(m: f64) -> Result<Self, Error> {
        Self::new(m * MB as f64)
    }

    /// Build from gigabytes.
    pub fn from_gigabytes(g: f64) -> Result<Self, Error> {
        Self::new(g * GB as f64)
    }

    /// Build from terabytes.
    pub fn from_terabytes(t: f64) -> Result<Self, Error> {
        Self::new(t * TB as f64)
    }

    /// Parse strings like `"28MB"`, `"1gb"`, `"512 kb"`,
    /// `"2megabytes"`. Case-insensitive, whitespace-tolerant. Accepts
    /// decimal values (`"1.5gb"`) and concatenated parts (e.g.
    /// `"1gb 256mb"`). See [`FromStr`].
    pub fn parse(s: &str) -> Result<Self, Error> {
        <Self as FromStr>::from_str(s)
    }

    /// Bytes (rounded up at construction).
    pub const fn to_bytes(self) -> i64 {
        self.bytes
    }

    /// Fractional kilobytes.
    pub fn to_kilobytes(self) -> f64 {
        self.bytes as f64 / KB as f64
    }

    /// Fractional megabytes.
    pub fn to_megabytes(self) -> f64 {
        self.bytes as f64 / MB as f64
    }

    /// Fractional gigabytes.
    pub fn to_gigabytes(self) -> f64 {
        self.bytes as f64 / GB as f64
    }

    /// Fractional terabytes.
    pub fn to_terabytes(self) -> f64 {
        self.bytes as f64 / TB as f64
    }

    fn new(b: f64) -> Result<Self, Error> {
        if b.is_nan() {
            return Err(Error::argument("size is NaN"));
        }
        if b < 0.0 {
            return Err(Error::argument("size must be at least 0"));
        }
        Ok(Self {
            bytes: b.ceil() as i64,
        })
    }
}

impl fmt::Display for Size {
    /// Auto-scaled human-readable rendering (e.g. `"1.50 GB"`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.bytes >= TB {
            write!(f, "{:.2} TB", self.to_terabytes())
        } else if self.bytes >= GB {
            write!(f, "{:.2} GB", self.to_gigabytes())
        } else if self.bytes >= MB {
            write!(f, "{:.2} MB", self.to_megabytes())
        } else if self.bytes >= KB {
            write!(f, "{:.2} kB", self.to_kilobytes())
        } else {
            write!(f, "{} B", self.bytes)
        }
    }
}

impl FromStr for Size {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        let clean: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        let lower = clean.to_ascii_lowercase();
        if lower.is_empty() {
            return Err(Error::argument("empty size string"));
        }

        let mut total_bytes: f64 = 0.0;
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
                return Err(Error::argument(format!("unrecognized size string: {s}")));
            }
            let value: f64 = num
                .parse()
                .map_err(|_| Error::argument(format!("invalid size number: {num}")))?;

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
            total_bytes += value * unit_to_bytes(&unit)?;
            found = true;
        }

        if !found {
            return Err(Error::argument(format!("unrecognized size string: {s}")));
        }
        Self::new(total_bytes)
    }
}

fn unit_to_bytes(unit: &str) -> Result<f64, Error> {
    Ok(match unit {
        "b" | "byte" | "bytes" => 1.0,
        "kb" | "kilobyte" | "kilobytes" => KB as f64,
        "mb" | "megabyte" | "megabytes" => MB as f64,
        "gb" | "gigabyte" | "gigabytes" => GB as f64,
        "tb" | "terabyte" | "terabytes" => TB as f64,
        other => return Err(Error::argument(format!("unsupported size unit: {other}"))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_bytes_rounds_up() {
        assert_eq!(Size::from_bytes(10.4).unwrap().to_bytes(), 11);
        assert_eq!(Size::from_bytes(0.0).unwrap().to_bytes(), 0);
    }

    #[test]
    fn from_kilobytes_uses_decimal_base() {
        let s = Size::from_kilobytes(1.5).unwrap();
        assert_eq!(s.to_bytes(), 1500);
    }

    #[test]
    fn from_megabytes_to_kilobytes_round_trip() {
        let s = Size::from_megabytes(2.0).unwrap();
        assert!((s.to_kilobytes() - 2_000.0).abs() < f64::EPSILON);
    }

    #[test]
    fn from_str_parses_compound() {
        let s = Size::parse("1gb256mb").unwrap();
        assert_eq!(s.to_bytes(), GB + 256 * MB);
    }

    #[test]
    fn from_str_handles_whitespace_and_mixed_case() {
        let s = Size::parse("  512 kB  ").unwrap();
        assert_eq!(s.to_bytes(), 512 * KB);
    }

    #[test]
    fn from_str_decimal_value() {
        let s = Size::parse("1.5gb").unwrap();
        assert_eq!(s.to_bytes(), (1.5 * GB as f64).ceil() as i64);
    }

    #[test]
    fn from_str_rejects_empty() {
        assert!(Size::parse("").is_err());
        assert!(Size::parse("   ").is_err());
    }

    #[test]
    fn from_str_rejects_unknown_unit() {
        assert!(Size::parse("2pb").is_err());
    }

    #[test]
    fn negative_or_nan_rejected() {
        assert!(Size::from_bytes(-1.0).is_err());
        assert!(Size::from_bytes(f64::NAN).is_err());
    }

    #[test]
    fn display_auto_scales() {
        assert_eq!(Size::from_bytes(1.0).unwrap().to_string(), "1 B");
        assert_eq!(Size::from_kilobytes(1.5).unwrap().to_string(), "1.50 kB");
        assert_eq!(Size::from_megabytes(28.0).unwrap().to_string(), "28.00 MB");
        assert_eq!(Size::from_gigabytes(1.0).unwrap().to_string(), "1.00 GB");
        assert_eq!(Size::from_terabytes(2.0).unwrap().to_string(), "2.00 TB");
    }
}

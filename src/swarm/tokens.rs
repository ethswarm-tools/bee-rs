//! BZZ / DAI fixed-point token math. Mirrors bee-go's
//! `pkg/swarm/tokens.go` and bee-js `BZZ` / `DAI`.
//!
//! `Bzz` stores amounts as PLUR (`1 BZZ = 10^16 PLUR`). `Dai` stores
//! amounts as wei (`1 DAI = 10^18 wei`). All operations are pure —
//! the underlying [`BigInt`] is owned, so callers can hand instances
//! around without worrying about aliasing.

use std::fmt;
use std::ops::{Add, Sub};
use std::str::FromStr;

use num_bigint::BigInt;
use num_traits::{Pow, Signed, Zero};

use crate::swarm::Error;

/// Decimal digits used by the BZZ token (`1 BZZ = 10^16 PLUR`).
pub const BZZ_DIGITS: u32 = 16;
/// Decimal digits used by the DAI / native token (`1 DAI = 10^18 wei`).
pub const DAI_DIGITS: u32 = 18;

macro_rules! token_type {
    (
        $(#[$meta:meta])*
        $name:ident,
        $base_field:ident,
        $base_unit:literal,
        $digits_const:ident
    ) => {
        $(#[$meta])*
        #[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name {
            $base_field: BigInt,
        }

        impl $name {
            #[doc = concat!("Build from base units (", $base_unit, ").")]
            pub fn from_base_units(v: impl Into<BigInt>) -> Self {
                Self { $base_field: v.into() }
            }

            #[doc = concat!("Parse a base-unit decimal integer string (", $base_unit, ").")]
            pub fn from_base_units_str(s: &str) -> Result<Self, Error> {
                let v: BigInt = s.parse().map_err(|e| {
                    Error::argument(format!(
                        concat!("invalid ", $base_unit, " string {:?}: {}"),
                        s, e
                    ))
                })?;
                Ok(Self { $base_field: v })
            }

            /// Parse a human-readable decimal amount such as `"1.5"`.
            /// Extra fractional digits beyond the token's precision are
            /// silently truncated (matching bee-js / bee-go).
            pub fn from_decimal_str(s: &str) -> Result<Self, Error> {
                let v = decimal_to_base_units(s, $digits_const)?;
                Ok(Self { $base_field: v })
            }

            /// Build from an `f64`. Lossy for very large or very
            /// precise values. Mirrors bee-js `FixedPointNumber.fromFloat`.
            pub fn from_float(f: f64) -> Result<Self, Error> {
                if !f.is_finite() {
                    return Err(Error::argument(format!("non-finite float: {f}")));
                }
                let s = format!("{:.*}", $digits_const as usize, f);
                let trimmed = trim_trailing_decimal_zeros(&s);
                Self::from_decimal_str(trimmed)
            }

            #[doc = concat!("Borrow the underlying ", $base_unit, " value.")]
            pub fn as_base_units(&self) -> &BigInt {
                &self.$base_field
            }

            #[doc = concat!("Owned copy of the ", $base_unit, " value.")]
            pub fn to_base_units(&self) -> BigInt {
                self.$base_field.clone()
            }

            #[doc = concat!("Render the ", $base_unit, " value as a base-10 integer string.")]
            pub fn to_base_units_string(&self) -> String {
                self.$base_field.to_string()
            }

            /// Render with exactly the token's full fractional digits
            /// (e.g. `"1.5000000000000000"` for BZZ). Trailing zeros
            /// are kept so the format is stable.
            pub fn to_decimal_string(&self) -> String {
                base_units_to_decimal(&self.$base_field, $digits_const)
            }

            /// Truncate [`Self::to_decimal_string`] to `digits`
            /// fractional digits.
            pub fn to_significant_digits(&self, digits: u32) -> String {
                truncate_decimal(&self.to_decimal_string(), digits)
            }

            /// Lossy conversion to `f64`.
            pub fn to_f64(&self) -> f64 {
                let parts: Vec<f64> = self
                    .$base_field
                    .iter_u64_digits()
                    .map(|d| d as f64)
                    .collect();
                let mut acc = 0.0_f64;
                for (i, p) in parts.iter().enumerate() {
                    acc += p * (u64::MAX as f64 + 1.0).powi(i as i32);
                }
                if self.$base_field.sign() == num_bigint::Sign::Minus {
                    acc = -acc;
                }
                acc / 10f64.powi($digits_const as i32)
            }

            /// True iff this amount is exactly zero.
            pub fn is_zero(&self) -> bool {
                self.$base_field.is_zero()
            }
        }

        impl Add for $name {
            type Output = $name;
            fn add(self, rhs: $name) -> $name {
                $name { $base_field: self.$base_field + rhs.$base_field }
            }
        }

        impl<'a, 'b> Add<&'b $name> for &'a $name {
            type Output = $name;
            fn add(self, rhs: &'b $name) -> $name {
                $name { $base_field: &self.$base_field + &rhs.$base_field }
            }
        }

        impl Sub for $name {
            type Output = $name;
            fn sub(self, rhs: $name) -> $name {
                $name { $base_field: self.$base_field - rhs.$base_field }
            }
        }

        impl<'a, 'b> Sub<&'b $name> for &'a $name {
            type Output = $name;
            fn sub(self, rhs: &'b $name) -> $name {
                $name { $base_field: &self.$base_field - &rhs.$base_field }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.to_decimal_string())
            }
        }

        impl FromStr for $name {
            type Err = Error;
            fn from_str(s: &str) -> Result<Self, Error> {
                Self::from_decimal_str(s)
            }
        }
    };
}

token_type!(
    /// BZZ token amount stored as PLUR base units.
    Bzz,
    plur,
    "PLUR",
    BZZ_DIGITS
);

token_type!(
    /// DAI / native-token amount stored as wei base units.
    Dai,
    wei,
    "wei",
    DAI_DIGITS
);

impl Bzz {
    /// Integer division by `divisor`.
    pub fn divide(&self, divisor: u64) -> Self {
        Self {
            plur: &self.plur / BigInt::from(divisor),
        }
    }

    /// Multiply by an unsigned integer.
    pub fn times(&self, factor: u64) -> Self {
        Self {
            plur: &self.plur * BigInt::from(factor),
        }
    }

    /// Convert to [`Dai`] given an exchange rate expressed as DAI per
    /// 1 BZZ. `dai = (plur * dai_per_bzz.wei) / 10^BZZ_DIGITS`.
    pub fn exchange_to_dai(&self, dai_per_bzz: &Dai) -> Dai {
        let num = &self.plur * &dai_per_bzz.wei;
        Dai {
            wei: num / pow10(BZZ_DIGITS),
        }
    }
}

impl Dai {
    /// Integer division by `divisor`.
    pub fn divide(&self, divisor: u64) -> Self {
        Self {
            wei: &self.wei / BigInt::from(divisor),
        }
    }

    /// Multiply by an unsigned integer.
    pub fn times(&self, factor: u64) -> Self {
        Self {
            wei: &self.wei * BigInt::from(factor),
        }
    }

    /// Convert to [`Bzz`] given an exchange rate expressed as DAI per
    /// 1 BZZ. `bzz_plur = (wei * 10^BZZ_DIGITS) / dai_per_bzz.wei`.
    pub fn exchange_to_bzz(&self, dai_per_bzz: &Dai) -> Bzz {
        let num = &self.wei * pow10(BZZ_DIGITS);
        Bzz {
            plur: num / &dai_per_bzz.wei,
        }
    }
}

// ---- helpers --------------------------------------------------------------

fn pow10(n: u32) -> BigInt {
    BigInt::from(10u32).pow(n)
}

fn decimal_to_base_units(s: &str, digits: u32) -> Result<BigInt, Error> {
    if s.is_empty() {
        return Err(Error::argument("empty decimal string"));
    }
    let (negative, rest) = match s.as_bytes()[0] {
        b'-' => (true, &s[1..]),
        b'+' => (false, &s[1..]),
        _ => (false, s),
    };
    let (int_part, frac_part) = match rest.find('.') {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => (rest, ""),
    };
    let int_part = if int_part.is_empty() { "0" } else { int_part };
    let digits_usize = digits as usize;
    let frac_truncated = if frac_part.len() > digits_usize {
        &frac_part[..digits_usize]
    } else {
        frac_part
    };
    let mut combined = String::with_capacity(int_part.len() + digits_usize);
    combined.push_str(int_part);
    combined.push_str(frac_truncated);
    for _ in frac_truncated.len()..digits_usize {
        combined.push('0');
    }
    let mut v: BigInt = combined
        .parse()
        .map_err(|e| Error::argument(format!("invalid decimal string {s:?}: {e}")))?;
    if negative {
        v = -v;
    }
    Ok(v)
}

fn base_units_to_decimal(v: &BigInt, digits: u32) -> String {
    let negative = v.sign() == num_bigint::Sign::Minus;
    let abs = v.abs();
    let denom = pow10(digits);
    let int_part = (&abs / &denom).to_string();
    let mut frac_part = (&abs % &denom).to_string();
    let want = digits as usize;
    while frac_part.len() < want {
        frac_part.insert(0, '0');
    }
    if negative {
        format!("-{int_part}.{frac_part}")
    } else {
        format!("{int_part}.{frac_part}")
    }
}

fn truncate_decimal(s: &str, digits: u32) -> String {
    match s.find('.') {
        None => s.to_string(),
        Some(dot) => {
            let end = (dot + 1 + digits as usize).min(s.len());
            s[..end].to_string()
        }
    }
}

fn trim_trailing_decimal_zeros(s: &str) -> &str {
    if !s.contains('.') {
        return s;
    }
    let trimmed = s.trim_end_matches('0');
    trimmed.trim_end_matches('.')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bzz_from_decimal_truncates_extra_digits() {
        // 17 fractional digits — last is dropped (BZZ has 16).
        let b = Bzz::from_decimal_str("1.50000000000000009").unwrap();
        assert_eq!(b.to_base_units_string(), "15000000000000000");
        assert_eq!(b.to_decimal_string(), "1.5000000000000000");
    }

    #[test]
    fn bzz_from_plur_string_round_trip() {
        let b = Bzz::from_base_units_str("12345678901234567890").unwrap();
        assert_eq!(b.to_base_units_string(), "12345678901234567890");
    }

    #[test]
    fn bzz_arithmetic_matches_plur() {
        let a = Bzz::from_base_units(100u64);
        let b = Bzz::from_base_units(40u64);
        assert_eq!((a.clone() + b.clone()).to_base_units_string(), "140");
        assert_eq!((a - b).to_base_units_string(), "60");
    }

    #[test]
    fn bzz_negative_decimal_round_trip() {
        let b = Bzz::from_decimal_str("-2.5").unwrap();
        assert_eq!(b.to_decimal_string(), "-2.5000000000000000");
    }

    #[test]
    fn dai_pads_fractional_part_to_eighteen_digits() {
        let d = Dai::from_decimal_str("0.5").unwrap();
        // 0.5 DAI = 5 * 10^17 wei
        assert_eq!(d.to_base_units_string(), "500000000000000000");
        assert_eq!(d.to_decimal_string(), "0.500000000000000000");
    }

    #[test]
    fn exchange_to_dai_uses_dai_per_bzz_rate() {
        // Rate: 0.10 DAI per 1 BZZ → 1 BZZ → 0.10 DAI.
        let rate = Dai::from_decimal_str("0.10").unwrap();
        let one_bzz = Bzz::from_decimal_str("1").unwrap();
        let got = one_bzz.exchange_to_dai(&rate);
        assert_eq!(got.to_decimal_string(), "0.100000000000000000");
    }

    #[test]
    fn exchange_to_bzz_inverts_rate() {
        // Rate: 0.10 DAI per 1 BZZ → 1 DAI buys 10 BZZ.
        let rate = Dai::from_decimal_str("0.10").unwrap();
        let one_dai = Dai::from_decimal_str("1").unwrap();
        let bzz = one_dai.exchange_to_bzz(&rate);
        assert_eq!(bzz.to_decimal_string(), "10.0000000000000000");
    }

    #[test]
    fn cmp_orders_by_base_units() {
        let a = Bzz::from_base_units(100u64);
        let b = Bzz::from_base_units(200u64);
        assert!(a < b);
        assert!(b > a);
        assert_eq!(a, Bzz::from_base_units(100u64));
    }

    #[test]
    fn divide_and_times_scale_correctly() {
        let a = Bzz::from_base_units(1000u64);
        assert_eq!(a.divide(4).to_base_units_string(), "250");
        assert_eq!(a.times(3).to_base_units_string(), "3000");
    }

    #[test]
    fn to_significant_digits_truncates_only() {
        let b = Bzz::from_decimal_str("1.5").unwrap();
        assert_eq!(b.to_significant_digits(2), "1.50");
        // Bee-go parity: digits=0 keeps the trailing dot.
        assert_eq!(b.to_significant_digits(0), "1.");
    }

    #[test]
    fn from_str_parses_decimal() {
        let b: Bzz = "0.5".parse().unwrap();
        assert_eq!(b.to_base_units_string(), "5000000000000000");
    }

    #[test]
    fn from_float_strips_trailing_zeros_before_parse() {
        let b = Bzz::from_float(0.5).unwrap();
        assert_eq!(b.to_decimal_string(), "0.5000000000000000");
    }

    #[test]
    fn rejects_non_finite_floats() {
        assert!(Bzz::from_float(f64::NAN).is_err());
        assert!(Dai::from_float(f64::INFINITY).is_err());
    }
}

// SPDX-License-Identifier: MIT
//
// Copyright 2016-2026, Johann Tuffe.

//! Conversion of spreadsheet number text into [`rust_decimal::Decimal`].
//!
//! Text based formats (xlsx, ods) store numbers as decimal strings. They are
//! converted directly into a `Decimal`, never via `f64`, so the conversion is
//! exact whenever the value fits into a `Decimal` (at most 28 fractional digits
//! and a 96 bit mantissa). Otherwise the value is rounded once, half to even,
//! to the nearest representable `Decimal`.

use rust_decimal::Decimal;

use crate::formats::{format_excel_f64_ref, CellFormat};
use crate::{Data, DataRef, Range};

/// Maximum number of significant digits that can fit into a 96 bit mantissa.
const MAX_DIGITS: usize = 29;

/// Number of leading significant digits kept for rounding. Must be greater
/// than `MAX_DIGITS` so the rounding digit is always available; any further
/// digits only matter as a "sticky" non-zero flag.
const BUF_DIGITS: usize = MAX_DIGITS + 2;

/// Upper bound for the absolute value of the parsed exponent. Anything bigger
/// either overflows or rounds to zero anyway.
const MAX_EXPONENT: i64 = 1_000_000;

/// Parse a number in plain (`-123.45`) or scientific (`1.2345E-5`) notation
/// into a `Decimal`.
///
/// Returns `None` if `s` is not a number or if its integer part is too large
/// to be represented by a `Decimal` (more than ~7.9e28).
pub(crate) fn decimal_from_str(s: &[u8]) -> Option<Decimal> {
    let (negative, s) = match s.split_first()? {
        (b'-', rest) => (true, rest),
        (b'+', rest) => (false, rest),
        _ => (false, s),
    };

    // Significant digits (leading zeros stripped). Only the first
    // `BUF_DIGITS` are stored, `sticky` records if any digit dropped after
    // that is non-zero.
    let mut buf = [0u8; BUF_DIGITS];
    let mut len = 0usize;
    let mut sticky = false;
    // Total number of significant digits, including the ones not stored.
    let mut n_digits = 0i64;
    let mut frac_digits = 0i64;
    let mut any_digit = false;
    let mut in_frac = false;

    let mut i = 0;
    while i < s.len() {
        match s[i] {
            c @ b'0'..=b'9' => {
                any_digit = true;
                if in_frac {
                    frac_digits += 1;
                }
                if n_digits > 0 || c != b'0' {
                    if len < BUF_DIGITS {
                        buf[len] = c - b'0';
                        len += 1;
                    } else if c != b'0' {
                        sticky = true;
                    }
                    n_digits += 1;
                }
            }
            b'.' if !in_frac => in_frac = true,
            _ => break,
        }
        i += 1;
    }
    if !any_digit {
        return None;
    }

    let mut exponent = 0i64;
    if i < s.len() {
        if !matches!(s[i], b'e' | b'E') {
            return None;
        }
        i += 1;
        let exp_negative = match s.get(i) {
            Some(b'-') => {
                i += 1;
                true
            }
            Some(b'+') => {
                i += 1;
                false
            }
            _ => false,
        };
        if i == s.len() {
            return None;
        }
        for &c in &s[i..] {
            if !c.is_ascii_digit() {
                return None;
            }
            exponent = (exponent * 10 + i64::from(c - b'0')).min(MAX_EXPONENT);
        }
        if exp_negative {
            exponent = -exponent;
        }
    }

    // value = digits * 10^-scale
    let mut scale = frac_digits - exponent;

    if n_digits == 0 {
        // Zero, keep the written scale where possible.
        let scale = scale.clamp(0, i64::from(Decimal::MAX_SCALE)) as u32;
        return Some(Decimal::new(0, scale));
    }

    if scale < 0 {
        // Positive exponent beyond the fractional digits: append zeros.
        if n_digits - scale > MAX_DIGITS as i64 {
            return None;
        }
        for _ in 0..-scale {
            buf[len] = 0;
            len += 1;
        }
        n_digits -= scale;
        scale = 0;
    }

    // Minimal number of trailing digits to drop: enough to bring the scale
    // down to `MAX_SCALE` and the digit count down to `MAX_DIGITS`. A 29 digit
    // mantissa may still exceed 96 bits (or round up to 30 digits), in which
    // case one more digit is dropped. Each attempt rounds the original digits
    // so there is no double rounding.
    let min_drop = (scale - i64::from(Decimal::MAX_SCALE))
        .max(n_digits - MAX_DIGITS as i64)
        .max(0);
    for drop in min_drop..=min_drop + 1 {
        if drop > scale {
            // Would need to round the integer part: too large.
            return None;
        }
        let keep = n_digits - drop;
        let mantissa = if keep < 0 {
            // All the digits are below the rounding position.
            0
        } else {
            round_half_even(&buf[..len], sticky, keep as usize)
        };
        let mantissa = if negative {
            -(mantissa as i128)
        } else {
            mantissa as i128
        };
        if let Ok(d) = Decimal::try_from_i128_with_scale(mantissa, (scale - drop) as u32) {
            return Some(d);
        }
    }
    None
}

/// Convert a `f64` into a `Decimal` using its shortest round trip decimal
/// representation (i.e. `0.1f64` becomes `0.1`, not
/// `0.1000000000000000055511151231`).
///
/// Returns `None` for non finite values or values too large for a `Decimal`.
pub(crate) fn decimal_from_f64(v: f64) -> Option<Decimal> {
    if !v.is_finite() {
        return None;
    }
    decimal_from_str(format!("{v:e}").as_bytes())
}

/// Convert a number from a binary format (xls, xlsb) into a `DataRef`.
///
/// Cells with a date or time format are converted to `DataRef::DateTime` as
/// usual, values that cannot be represented by a `Decimal` stay
/// `DataRef::Float`.
pub(crate) fn format_excel_f64_decimal_ref(
    value: f64,
    format: Option<&CellFormat>,
    is_1904: bool,
) -> DataRef<'static> {
    match format {
        Some(CellFormat::DateTime | CellFormat::TimeDelta) => {
            format_excel_f64_ref(value, format, is_1904)
        }
        _ => decimal_from_f64(value).map_or(DataRef::Float(value), DataRef::Decimal),
    }
}

/// Convert the numbers (ints and floats) of a range into decimals.
///
/// Used by readers that parse whole sheets from `f64` values before the
/// decimal option is known (xls). Values that cannot be represented by a
/// `Decimal` stay `Data::Float`.
pub(crate) fn range_to_decimal(mut range: Range<Data>) -> Range<Data> {
    for cell in &mut range.inner {
        match *cell {
            Data::Int(v) => *cell = Data::Decimal(v.into()),
            Data::Float(v) => {
                if let Some(d) = decimal_from_f64(v) {
                    *cell = Data::Decimal(d);
                }
            }
            _ => (),
        }
    }
    range
}

/// Round the significant `digits` to the first `keep` digits, half to even.
///
/// `digits` holds the leading significant digits, `sticky` is set if any
/// digit not present in `digits` is non-zero. `keep` is at most `MAX_DIGITS`.
fn round_half_even(digits: &[u8], sticky: bool, keep: usize) -> u128 {
    let mut mantissa = digits
        .iter()
        .take(keep)
        .fold(0u128, |acc, &d| acc * 10 + u128::from(d));
    // Digits beyond `digits.len()` are implicit zeros (only `sticky` matters).
    let round_digit = digits.get(keep).copied().unwrap_or(0);
    let rest_non_zero = sticky || digits.iter().skip(keep + 1).any(|&d| d != 0);
    let round_up = match round_digit {
        6..=9 => true,
        5 => rest_non_zero || mantissa % 2 == 1,
        _ => false,
    };
    if round_up {
        mantissa += 1;
    }
    mantissa
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn parse(s: &str) -> Option<String> {
        decimal_from_str(s.as_bytes()).map(|d| d.to_string())
    }

    #[test]
    fn exact() {
        for s in [
            "0",
            "1",
            "-1",
            "0.1",
            "1.50",
            "-123.456",
            "2.9999999999999996",
            "0.30000000000000004",
            "0.10000000000000001",
            "123456789012345.67",
            "1234567890123456789012345678",
            "0.1234567890123456789012345678",
            "79228162514264337593543950335",
            "-79228162514264337593543950335",
            "0.0000000000000000000000000001",
        ] {
            assert_eq!(parse(s).as_deref(), Some(s), "{s}");
        }
    }

    #[test]
    fn scientific() {
        assert_eq!(parse("1E-5").as_deref(), Some("0.00001"));
        assert_eq!(parse("1e-5").as_deref(), Some("0.00001"));
        assert_eq!(
            parse("1.2345E+20").as_deref(),
            Some("123450000000000000000")
        );
        assert_eq!(parse("1.50E+1").as_deref(), Some("15.0"));
        assert_eq!(parse("-2.5E2").as_deref(), Some("-250"));
        assert_eq!(
            parse("0.0001E+30").as_deref(),
            Some("100000000000000000000000000")
        );
        assert_eq!(parse("9.7e-7").as_deref(), Some("0.00000097"));
    }

    #[test]
    fn zero() {
        assert_eq!(parse("0.000").as_deref(), Some("0.000"));
        assert_eq!(parse("-0").as_deref(), Some("0"));
        assert_eq!(
            parse("0E-50").as_deref(),
            Some("0.0000000000000000000000000000")
        );
        assert_eq!(parse("0E+50").as_deref(), Some("0"));
    }

    #[test]
    fn rounded() {
        // Too many fractional digits, half to even.
        assert_eq!(
            parse("2e-29").as_deref(),
            Some("0.0000000000000000000000000000")
        );
        assert_eq!(
            parse("2.5e-28").as_deref(),
            Some("0.0000000000000000000000000002")
        );
        assert_eq!(
            parse("3.5e-28").as_deref(),
            Some("0.0000000000000000000000000004")
        );
        assert_eq!(
            parse("2.50000000000000000000000000000000001e-28").as_deref(),
            Some("0.0000000000000000000000000003")
        );
        assert_eq!(
            parse("-2.6e-28").as_deref(),
            Some("-0.0000000000000000000000000003")
        );
        assert_eq!(
            parse("1E-300").as_deref(),
            Some("0.0000000000000000000000000000")
        );
        // Too many significant digits.
        assert_eq!(
            parse("1.2345678901234567890123456789012345").as_deref(),
            Some("1.2345678901234567890123456789")
        );
        assert_eq!(
            parse("0.123456789012345678901234567890").as_deref(),
            Some("0.1234567890123456789012345679")
        );
        // 29 digits but above the 96 bit maximum: one more digit is dropped.
        assert_eq!(
            parse("8.1234567890123456789012345678").as_deref(),
            Some("8.123456789012345678901234568")
        );
        // Carry propagates into a new digit.
        assert_eq!(
            parse("9.99999999999999999999999999999").as_deref(),
            Some("10.000000000000000000000000000")
        );
    }

    #[test]
    fn overflow() {
        assert_eq!(parse("1E+300"), None);
        assert_eq!(parse("79228162514264337593543950336"), None);
        assert_eq!(parse("100000000000000000000000000000"), None);
        assert_eq!(parse("1E+29"), None);
        assert_eq!(parse("1E+99999999999999999999"), None);
    }

    #[test]
    fn invalid() {
        for s in [
            "", "-", "+", ".", "e5", "1e", "1e+", "1.2.3", "1,2", "abc", "NaN", "inf", " 1", "1 ",
            "1_000",
        ] {
            assert_eq!(parse(s), None, "{s:?}");
        }
        assert_eq!(parse(".5").as_deref(), Some("0.5"));
        assert_eq!(parse("5.").as_deref(), Some("5"));
        assert_eq!(parse("+5").as_deref(), Some("5"));
    }

    /// Compare with the exact parsers of `rust_decimal` on random inputs.
    #[test]
    fn matches_rust_decimal() {
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        let (mut plain, mut scientific) = (0, 0);
        for _ in 0..200_000 {
            let mut s = String::new();
            if next(2) == 0 {
                s.push('-');
            }
            for _ in 0..1 + next(20) {
                s.push(char::from(b'0' + next(10) as u8));
            }
            if next(2) == 0 {
                s.push('.');
                for _ in 0..1 + next(20) {
                    s.push(char::from(b'0' + next(10) as u8));
                }
            }
            let ours = decimal_from_str(s.as_bytes());
            if let Ok(d) = Decimal::from_str_exact(&s) {
                assert_eq!(ours, Some(d), "{s}");
                assert_eq!(ours.unwrap().scale(), d.scale(), "{s}");
                plain += 1;
            }
            let digits = s.bytes().filter(u8::is_ascii_digit).count();
            let frac_digits = s.split_once('.').map_or(0, |(_, f)| f.len()) as i64;
            let exponent = next(40) as i64 - 20;
            s.push('e');
            s.push_str(&exponent.to_string());
            // Only compare inputs that `from_scientific` represents exactly.
            if digits <= 28 && (0..=28).contains(&(frac_digits - exponent)) {
                if let Ok(d) = Decimal::from_scientific(&s) {
                    assert_eq!(decimal_from_str(s.as_bytes()), Some(d), "{s}");
                    scientific += 1;
                }
            }
        }
        assert!(
            plain > 100_000 && scientific > 10_000,
            "{plain} {scientific}"
        );
    }

    #[test]
    fn from_f64() {
        assert_eq!(
            decimal_from_f64(0.1),
            Some(Decimal::from_str("0.1").unwrap())
        );
        assert_eq!(decimal_from_f64(100.0), Some(Decimal::from(100)));
        assert_eq!(
            decimal_from_f64(-1.5),
            Some(Decimal::from_str("-1.5").unwrap())
        );
        assert_eq!(
            decimal_from_f64(0.1 + 0.2),
            Some(Decimal::from_str("0.30000000000000004").unwrap())
        );
        assert_eq!(decimal_from_f64(1e300), None);
        assert_eq!(decimal_from_f64(f64::NAN), None);
        assert_eq!(decimal_from_f64(f64::INFINITY), None);
    }
}

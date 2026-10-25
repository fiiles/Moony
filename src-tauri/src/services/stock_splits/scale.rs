//! Exact decimal scaling for stock splits: `quantity × n / d` and `price × d / n` on decimal
//! text, without floats, so a split never drifts a position by a rounding error.

use crate::error::{AppError, Result};
use crate::services::dedup::canonical_amount;

/// Fractional digits kept in a scaled amount.
const FRACTION_DIGITS: usize = 8;

/// How a quotient that does not terminate within [`FRACTION_DIGITS`] is rounded. The caller
/// chooses: buys round half-up, sells round down, prices half-up (spec D3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rounding {
    HalfUp,
    Down,
}

/// `text × mul / div`, exact base-ten digits on the canonical text; at most 8 fractional
/// digits, rounded per `rounding` when the quotient does not terminate; result canonical.
///
/// Errors: `validation.amountInvalid` for non-numeric or negative text, `div == 0`, `mul == 0`.
pub fn scale_amount(text: &str, mul: u64, div: u64, rounding: Rounding) -> Result<String> {
    let invalid = || AppError::Validation("validation.amountInvalid".into());
    if mul == 0 || div == 0 || !is_unsigned_decimal(text) {
        return Err(invalid());
    }
    let canonical = canonical_amount(text);
    let (integer, fraction) = canonical.split_once('.').unwrap_or((&canonical, ""));
    // An exponent beyond what `canonical_amount` expands comes back unchanged.
    if !integer
        .bytes()
        .chain(fraction.bytes())
        .all(|b| b.is_ascii_digit())
    {
        return Err(invalid());
    }

    // numerator = all digits x mul, the value being numerator / 10^fraction.len().
    let digits: Vec<u8> = integer
        .bytes()
        .chain(fraction.bytes())
        .map(|b| b - b'0')
        .collect();
    let numerator = multiply(&digits, mul);

    // Divide with enough extra zeros that the quotient carries at least one digit past the
    // kept fraction (the guard digit for half-up rounding).
    let extra = (FRACTION_DIGITS + 1).saturating_sub(fraction.len());
    let scale = fraction.len() + extra;
    let mut quotient = divide(&numerator, extra, div);
    if quotient.len() <= scale {
        let mut padded = vec![0; scale + 1 - quotient.len()];
        padded.append(&mut quotient);
        quotient = padded;
    }

    let dropped = scale - FRACTION_DIGITS;
    let guard = quotient[quotient.len() - dropped];
    quotient.truncate(quotient.len() - dropped);
    if rounding == Rounding::HalfUp && guard >= 5 {
        increment(&mut quotient);
    }

    let split = quotient.len() - FRACTION_DIGITS;
    let text: String = quotient.iter().map(|d| char::from(b'0' + d)).collect();
    Ok(canonical_amount(&format!(
        "{}.{}",
        &text[..split],
        &text[split..]
    )))
}

/// Plain non-negative decimal text, optionally with an exponent: `12`, `.5`, `1.50`, `2e3`.
/// `canonical_amount` returns unparseable text unchanged, so the digits are checked here.
fn is_unsigned_decimal(text: &str) -> bool {
    let text = text.trim().trim_start_matches('+');
    let (mantissa, exponent) = text.split_once(['e', 'E']).unwrap_or((text, "0"));
    let exponent = exponent.trim_start_matches(['+', '-']);
    let (integer, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    (!integer.is_empty() || !fraction.is_empty())
        && digits(integer)
        && digits(fraction)
        && !exponent.is_empty()
        && digits(exponent)
}

/// Long multiplication of a digit string by a small factor.
fn multiply(digits: &[u8], factor: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(digits.len() + 20);
    let mut carry: u128 = 0;
    for &digit in digits.iter().rev() {
        let product = u128::from(digit) * u128::from(factor) + carry;
        out.push((product % 10) as u8);
        carry = product / 10;
    }
    while carry > 0 {
        out.push((carry % 10) as u8);
        carry /= 10;
    }
    out.reverse();
    out
}

/// Long division of `digits` followed by `extra_zeros` zeros by `divisor`, truncated.
fn divide(digits: &[u8], extra_zeros: usize, divisor: u64) -> Vec<u8> {
    let divisor = u128::from(divisor);
    let mut remainder: u128 = 0;
    digits
        .iter()
        .copied()
        .chain(std::iter::repeat_n(0, extra_zeros))
        .map(|digit| {
            remainder = remainder * 10 + u128::from(digit);
            let quotient = remainder / divisor;
            remainder %= divisor;
            quotient as u8
        })
        .collect()
}

/// Adds one unit in the last place of a digit string, growing it on overflow.
fn increment(digits: &mut Vec<u8>) {
    for digit in digits.iter_mut().rev() {
        if *digit == 9 {
            *digit = 0;
        } else {
            *digit += 1;
            return;
        }
    }
    digits.insert(0, 1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiplies_exactly() {
        assert_eq!(scale_amount("15", 4, 1, Rounding::HalfUp).unwrap(), "60");
        assert_eq!(scale_amount("2.5", 4, 1, Rounding::HalfUp).unwrap(), "10");
    }

    #[test]
    fn divides_exactly() {
        assert_eq!(scale_amount("950", 1, 10, Rounding::HalfUp).unwrap(), "95");
        assert_eq!(
            scale_amount("185.5", 1, 4, Rounding::HalfUp).unwrap(),
            "46.375"
        );
    }

    #[test]
    fn reverse_split_quantity() {
        assert_eq!(scale_amount("30", 1, 10, Rounding::HalfUp).unwrap(), "3");
    }

    #[test]
    fn non_terminating_half_up_and_down() {
        assert_eq!(
            scale_amount("1", 1, 3, Rounding::HalfUp).unwrap(),
            "0.33333333"
        );
        assert_eq!(
            scale_amount("2", 1, 3, Rounding::HalfUp).unwrap(),
            "0.66666667"
        );
        assert_eq!(
            scale_amount("2", 1, 3, Rounding::Down).unwrap(),
            "0.66666666"
        );
        assert_eq!(
            scale_amount("0.5", 1, 3, Rounding::HalfUp).unwrap(),
            "0.16666667"
        );
    }

    #[test]
    fn the_1_3_oversold_example_stays_valid() {
        // buys 1 + 1 (half-up) >= sell 2 (down): 0.33333333 + 0.33333333 >= 0.66666666
        let b = scale_amount("1", 1, 3, Rounding::HalfUp).unwrap();
        let s = scale_amount("2", 1, 3, Rounding::Down).unwrap();
        assert!(b.parse::<f64>().unwrap() * 2.0 >= s.parse::<f64>().unwrap());
    }

    #[test]
    fn canonical_output_and_large_values() {
        assert_eq!(scale_amount("0.10", 2, 1, Rounding::HalfUp).unwrap(), "0.2");
        assert_eq!(
            scale_amount("123456789012.5", 3, 2, Rounding::HalfUp).unwrap(),
            "185185183518.75"
        );
    }

    #[test]
    fn rejects_bad_input() {
        for t in ["abc", "-1", ""] {
            assert!(scale_amount(t, 2, 1, Rounding::HalfUp).is_err());
        }
        assert!(scale_amount("1", 0, 1, Rounding::HalfUp).is_err());
        assert!(scale_amount("1", 1, 0, Rounding::HalfUp).is_err());
        for t in ["1e", "1e-", ".", "+", "1.2.3", "1e1001", "1,5"] {
            assert!(scale_amount(t, 2, 1, Rounding::HalfUp).is_err(), "{t}");
        }
    }

    #[test]
    fn accepts_exponent_and_plain_fraction_forms() {
        assert_eq!(scale_amount("1e3", 2, 1, Rounding::HalfUp).unwrap(), "2000");
        assert_eq!(scale_amount(".5", 3, 1, Rounding::HalfUp).unwrap(), "1.5");
        assert_eq!(scale_amount("0", 3, 7, Rounding::HalfUp).unwrap(), "0");
    }

    #[test]
    fn rounding_carries_into_the_integer_part() {
        assert_eq!(
            scale_amount("0.999999999", 1, 1, Rounding::HalfUp).unwrap(),
            "1"
        );
        assert_eq!(
            scale_amount("0.999999999", 1, 1, Rounding::Down).unwrap(),
            "0.99999999"
        );
    }
}

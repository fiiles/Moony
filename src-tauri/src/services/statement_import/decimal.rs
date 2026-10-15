//! Exact decimal arithmetic for the statement balance check (never `f64`).

/// A decimal number as an integer mantissa and a count of fraction digits,
/// always kept normalized (no trailing zero in the fraction, zero has scale 0),
/// so the derived equality is numeric equality. The mantissa is never
/// `i128::MIN`, so [`Dec::neg`] cannot overflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dec {
    mantissa: i128,
    scale: u32,
}

/// Most fraction digits `parse` accepts; keeps every rescale inside `i128`.
const MAX_SCALE: u32 = 18;

impl Dec {
    /// An optional sign, digits, an optional `.` and up to 18 fraction digits;
    /// nothing else (no spaces, separators or exponent). At least one digit.
    pub fn parse(text: &str) -> Option<Dec> {
        let (negative, unsigned) = match text.as_bytes().first()? {
            b'-' => (true, &text[1..]),
            b'+' => (false, &text[1..]),
            _ => (false, text),
        };
        let (integer, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
        let digits = integer.bytes().chain(fraction.bytes());
        if integer.len() + fraction.len() == 0
            || fraction.len() > MAX_SCALE as usize
            || !digits.clone().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let mut mantissa: i128 = 0;
        for b in digits {
            mantissa = mantissa
                .checked_mul(10)?
                .checked_add(i128::from(b - b'0'))?;
        }
        Some(
            Dec {
                mantissa: if negative { -mantissa } else { mantissa },
                scale: fraction.len() as u32,
            }
            .normalized(),
        )
    }

    pub fn zero() -> Dec {
        Dec {
            mantissa: 0,
            scale: 0,
        }
    }

    /// `None` when the sum does not fit (so it cannot be `std::ops::Add`).
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, other: Dec) -> Option<Dec> {
        let scale = self.scale.max(other.scale);
        let rescaled = |d: Dec| {
            10i128
                .checked_pow(scale - d.scale)
                .and_then(|factor| d.mantissa.checked_mul(factor))
        };
        let mantissa = rescaled(self)?
            .checked_add(rescaled(other)?)
            .filter(|m| *m != i128::MIN)?;
        Some(Dec { mantissa, scale }.normalized())
    }

    /// An inherent method, so callers need no `std::ops::Neg` import.
    #[allow(clippy::should_implement_trait)]
    pub fn neg(self) -> Dec {
        Dec {
            mantissa: -self.mantissa,
            scale: self.scale,
        }
    }

    pub fn is_zero(self) -> bool {
        self.mantissa == 0
    }

    /// Canonical text: `-12.5`, `1250.5`, `0`.
    pub fn to_text(self) -> String {
        let sign = if self.mantissa < 0 { "-" } else { "" };
        let digits = self.mantissa.unsigned_abs().to_string();
        if self.scale == 0 {
            return format!("{sign}{digits}");
        }
        let scale = self.scale as usize;
        let padded = format!("{digits:0>width$}", width = scale + 1);
        let (integer, fraction) = padded.split_at(padded.len() - scale);
        format!("{sign}{integer}.{fraction}")
    }

    /// Drops trailing fraction zeros so equal numbers have equal fields.
    fn normalized(mut self) -> Dec {
        while self.scale > 0 && self.mantissa % 10 == 0 {
            self.mantissa /= 10;
            self.scale -= 1;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::Dec;

    fn d(s: &str) -> Dec {
        Dec::parse(s).expect(s)
    }

    #[test]
    fn adds_exactly() {
        assert_eq!(d("0.1").add(d("0.2")).unwrap(), d("0.3"));
        assert_eq!(
            d("1000.00")
                .add(d("500.50"))
                .unwrap()
                .add(d("-250"))
                .unwrap()
                .to_text(),
            "1250.5"
        );
    }

    #[test]
    fn canonical_text() {
        assert_eq!(d("-012.50").to_text(), "-12.5");
        assert_eq!(d("0.000").to_text(), "0");
        assert_eq!(d("+7").to_text(), "7");
    }

    #[test]
    fn rejects_non_decimals() {
        for bad in ["", "1,5", "1.2.3", "abc", "1e5", "-"] {
            assert!(Dec::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn equal_numbers_are_equal_whatever_the_spelling() {
        assert_eq!(d("1.50"), d("1.5"));
        assert_eq!(d("-0"), Dec::zero());
        assert_eq!(d("0.00"), Dec::zero());
        assert!(d("0.0").is_zero());
        assert!(!d("0.1").is_zero());
    }

    #[test]
    fn negation_and_cancelling() {
        assert_eq!(d("12.5").neg().to_text(), "-12.5");
        assert_eq!(d("-0.05").neg().to_text(), "0.05");
        assert!(d("12.5").add(d("12.5").neg()).unwrap().is_zero());
        assert_eq!(Dec::zero().to_text(), "0");
    }

    #[test]
    fn small_fractions_keep_their_leading_zeros() {
        assert_eq!(d("0.05").to_text(), "0.05");
        assert_eq!(d("-0.001").to_text(), "-0.001");
        assert_eq!(d(".5").to_text(), "0.5");
        assert_eq!(d("5.").to_text(), "5");
    }

    #[test]
    fn limits_are_refused_not_wrapped() {
        // 18 fraction digits fit, 19 do not.
        assert!(Dec::parse("0.123456789012345678").is_some());
        assert!(Dec::parse("0.1234567890123456789").is_none());
        // 39 digits do not fit an i128.
        assert!(Dec::parse(&"9".repeat(39)).is_none());
        let big = d(&"9".repeat(38));
        assert!(big.add(big).is_none());
        assert!(big.add(d("0.000000000000000001")).is_none());
    }

    #[test]
    fn spaces_signs_and_separators_are_not_decimals() {
        for bad in [" 1", "1 ", "--1", "+-1", "1_000", ".", "-.", "1.e5", "١٢"] {
            assert!(Dec::parse(bad).is_none(), "{bad}");
        }
    }
}

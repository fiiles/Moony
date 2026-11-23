//! Integer money of the stock transactions report. Every amount is rounded once to the report
//! currency's minor units and split by cumulative allocation, so every figure on the page is the
//! exact sum of the figures under it (hero = sales table = tickers = buckets, ledger = stats).

use crate::services::reporting::minor_units;

/// Quantities below this are rounding dust.
const EPSILON: f64 = 1e-9;

/// Minor units of one currency (`CZK`: 1 CZK = 100 units).
#[derive(Debug, Clone, Copy)]
pub struct Minor {
    decimals: usize,
    scale: f64,
}

impl Minor {
    pub fn of(currency: &str) -> Self {
        let decimals = minor_units(currency);
        Self {
            decimals,
            scale: 10_f64.powi(decimals as i32),
        }
    }

    /// `amount` rounded to whole minor units.
    pub fn units(self, amount: f64) -> i64 {
        (amount * self.scale).round() as i64
    }

    /// Back to the currency unit, for per-share figures that are not summed.
    pub fn amount(self, units: i64) -> f64 {
        units as f64 / self.scale
    }

    /// Wire text with exactly the currency's decimals, as `reporting::money_in_currency`.
    pub fn text(self, units: i64) -> String {
        let sign = if units < 0 { "-" } else { "" };
        let magnitude = units.unsigned_abs();
        if self.decimals == 0 {
            return format!("{sign}{magnitude}");
        }
        let divisor = 10_u64.pow(self.decimals as u32);
        format!(
            "{sign}{}.{:0width$}",
            magnitude / divisor,
            magnitude % divisor,
            width = self.decimals
        )
    }
}

/// Splits `total` units over consecutive pieces of a `whole` quantity: the piece ending at the
/// cumulative quantity `c` gets `round(total · c / whole)` minus what the pieces before it got,
/// so the pieces always add up to the allocated part exactly and to `total` once `whole` is
/// reached.
#[derive(Debug, Clone, Copy)]
pub struct Split {
    total: i64,
    whole: f64,
    quantity: f64,
    units: i64,
}

impl Split {
    pub fn new(total: i64, whole: f64) -> Self {
        Self {
            total,
            whole,
            quantity: 0.0,
            units: 0,
        }
    }

    /// The units of the next `quantity`.
    pub fn take(&mut self, quantity: f64) -> i64 {
        self.quantity += quantity;
        let cumulative = if self.whole <= 0.0 || self.quantity >= self.whole - EPSILON {
            self.total
        } else {
            (self.total as f64 * self.quantity / self.whole).round() as i64
        };
        let piece = cumulative - self.units;
        self.units = cumulative;
        piece
    }

    /// What no piece has taken yet.
    pub fn rest(&self) -> i64 {
        self.total - self.units
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::reporting::money_in_currency;

    #[test]
    fn text_matches_money_in_currency() {
        for (units, currency) in [
            (0, "CZK"),
            (5, "CZK"),
            (-5, "CZK"),
            (-123_456, "CZK"),
            (1_234_567_890, "USD"),
            (1_500, "UGX"),
            (-1_235, "JOD"),
        ] {
            let minor = Minor::of(currency);
            assert_eq!(
                minor.text(units),
                money_in_currency(minor.amount(units), currency),
                "{units} {currency}"
            );
            assert_eq!(minor.units(minor.amount(units)), units);
        }
    }

    #[test]
    fn pieces_add_up_to_the_total() {
        let mut split = Split::new(1_000, 3.0);
        let pieces = [split.take(1.0), split.take(1.0), split.take(1.0)];
        assert_eq!(pieces, [333, 334, 333]);
        assert_eq!(split.rest(), 0);

        // Float quantities that do not quite add up to the whole still end on the total.
        let mut split = Split::new(10_001, 0.3);
        let a = split.take(0.1);
        let b = split.take(0.2);
        assert_eq!(a + b, 10_001);

        // A partly taken total keeps the rest for the open remainder.
        let mut split = Split::new(999, 10.0);
        let taken = split.take(4.0);
        assert_eq!(taken, 400);
        assert_eq!(split.rest(), 599);
    }
}

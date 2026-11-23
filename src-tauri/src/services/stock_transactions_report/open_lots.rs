//! Open lots of the stock transactions report (spec D12): every still-open quantity with the
//! day it passes the holding-period test, its value today and its unrealized result.

use std::collections::HashMap;

use rusqlite::Connection;

use super::lots::Replay;
use super::sales::{pass_day, Converter, SalesOutcome};
use crate::error::Result;
use crate::models::stock_dividends::amount_text;
use crate::models::{StockReportCostMethod, StockReportOpenLot, StockTransactionsReportRequest};
use crate::services::fx::Quotes;
use crate::services::positions::{self, AssetKind};
use crate::services::reporting::money_in_currency;

/// The current native price of a position and its currency.
struct Price {
    price: f64,
    currency: String,
}

pub fn open_lots(
    conn: &Connection,
    replay: &Replay,
    outcome: &SalesOutcome,
    request: &StockTransactionsReportRequest,
    current: &Quotes,
    today: i64,
    conv: &mut Converter,
) -> Result<Vec<StockReportOpenLot>> {
    if replay.open.is_empty() {
        return Ok(Vec::new());
    }
    // The price and its conversion are the positions service's: its resolver and the current
    // quotes. Its quality (cost basis, dividends of every position) is not the report's.
    let positions = positions::value_positions_with_quotes(
        conn,
        AssetKind::Stock,
        &request.report_currency,
        current,
        today,
    )?;
    let currency = positions.currency.as_str();
    let (valued, pools) = (&outcome.valued, &outcome.pools);
    let prices: HashMap<&str, Price> = positions
        .items
        .iter()
        .filter_map(|p| {
            Some((
                p.investment_id.as_str(),
                Price {
                    price: p.price?,
                    currency: p.price_currency.clone()?,
                },
            ))
        })
        .collect();
    let months = request
        .holding_test_months
        .and_then(|m| u32::try_from(m).ok());

    let mut lots = Vec::new();
    for (index, open) in &replay.open {
        let buy = &replay.trades[*index];
        let value = prices.get(buy.investment_id.as_str()).and_then(|p| {
            let amount = open * p.price;
            if amount == 0.0 {
                return Some(0.0);
            }
            let converted = current.convert(amount, &p.currency, currency);
            conv.quality
                .track_conversion(current, &p.currency, currency, converted)
        });
        let share = open / buy.quantity;
        let cost = match request.cost_method {
            StockReportCostMethod::Fifo => valued.volume[*index]
                .zip(valued.fee[*index])
                .map(|(volume, fee)| (volume + fee) * share),
            StockReportCostMethod::Average => pools
                .get(&buy.investment_id)
                .filter(|pool| pool.quantity > 0.0)
                .and_then(|pool| Some((pool.cost? + pool.fee?) / pool.quantity * open)),
        };
        lots.push(StockReportOpenLot {
            investment_id: buy.investment_id.clone(),
            ticker: buy.ticker.clone(),
            company_name: buy.company_name.clone(),
            buy_day: buy.day,
            quantity: amount_text(*open),
            lot_quantity: amount_text(buy.quantity),
            buy_price: amount_text(buy.price),
            buy_currency: buy.currency.clone(),
            pass_day: months.map(|n| pass_day(buy.day, n)),
            value: value.map(|v| money_in_currency(v, currency)),
            unrealized: value
                .zip(cost)
                .map(|(v, c)| money_in_currency(v - c, currency)),
        });
    }
    Ok(lots)
}

#[cfg(test)]
mod tests {
    use super::super::super::fx;
    use super::super::report;
    use super::super::tests::*;
    use crate::models::{
        StockReportCostMethod, StockTransactionsReport, StockTransactionsReportRequest,
    };
    use crate::services::reporting::money_in_currency;

    const TODAY: i64 = 1_790_000_000;

    fn czk(amount: f64) -> String {
        money_in_currency(amount, "CZK")
    }

    fn run(
        conn: &rusqlite::Connection,
        method: StockReportCostMethod,
        months: Option<i64>,
    ) -> StockTransactionsReport {
        fx::set_current_for_test(&[("CZK", 25.0), ("USD", 1.0)]);
        let request = StockTransactionsReportRequest {
            from_day: 1_767_225_600,
            to_day: 1_798_675_200,
            report_currency: "CZK".into(),
            cost_method: method,
            holding_test_months: months,
        };
        report(conn, &request, TODAY).expect("report")
    }

    #[test]
    fn open_lots_carry_the_pass_day_and_the_unrealized_result() {
        let conn = fixture();
        add_price(&conn, "AAA", "400", "USD");
        let r = run(&conn, StockReportCostMethod::Fifo, Some(36));
        assert_eq!(r.open_lots.len(), 1);
        let lot = &r.open_lots[0];
        assert_eq!(lot.ticker, "AAA");
        assert_eq!(lot.quantity, "5");
        assert_eq!(lot.lot_quantity, "10");
        assert_eq!(lot.buy_day, D2);
        assert_eq!(lot.pass_day, Some(1_799_625_600)); // 2027-01-11
        assert_eq!(lot.value.as_deref(), Some(czk(50_000.0).as_str()));
        assert_eq!(lot.unrealized.as_deref(), Some(czk(24_950.0).as_str()));

        let r = run(&conn, StockReportCostMethod::Average, Some(36));
        assert_eq!(
            r.open_lots[0].unrealized.as_deref(),
            Some(czk(32_465.0).as_str())
        );
    }

    #[test]
    fn open_lots_have_no_pass_day_when_the_test_is_off() {
        let conn = fixture();
        add_price(&conn, "AAA", "400", "USD");
        let r = run(&conn, StockReportCostMethod::Fifo, None);
        assert_eq!(r.open_lots.len(), 1);
        assert!(r.open_lots[0].pass_day.is_none());
        assert!(r.open_lots[0].value.is_some());
    }

    #[test]
    fn an_unpriced_lot_has_no_value() {
        let conn = fixture();
        let r = run(&conn, StockReportCostMethod::Fifo, Some(36));
        let lot = &r.open_lots[0];
        assert!(lot.value.is_none() && lot.unrealized.is_none());
        assert!(r.fx_quality.missing_currencies.is_empty());
    }
}

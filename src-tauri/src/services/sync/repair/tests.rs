use rusqlite::Connection;

use super::*;
use crate::services::sync::capture::{self, Mode};
use crate::services::sync::hlc::DeviceId;
use crate::services::sync::state;
use crate::services::sync::test_support::migrated;

const DEVICE: DeviceId = DeviceId([7; 16]);
const DAY: i64 = 1_767_225_600; // 2026-01-01 00:00 UTC
const NEXT_DAY: i64 = DAY + 86_400;
const FEB: i64 = 1_769_904_000; // 2026-02-01
const NOW: i64 = 1_772_323_200; // 2026-03-01

fn exec(conn: &Connection, sql: &str) {
    conn.execute_batch(sql).expect(sql);
}

/// Writes `sql` the way the merge does: in `merge` mode, so nothing is clocked or queued.
fn merged(conn: &Connection, sql: &str) {
    capture::with_mode(conn, Mode::Merge, || {
        conn.execute_batch(sql)?;
        Ok(())
    })
    .expect(sql);
}

/// Sync on, capture installed, an empty outbox.
fn enabled() -> Connection {
    let conn = migrated();
    state::enable(&conn, DEVICE).expect("enable");
    exec(&conn, "DELETE FROM sync_outbox");
    conn
}

fn value(conn: &Connection, sql: &str) -> Option<String> {
    conn.query_row(sql, [], |r| r.get(0)).expect(sql)
}

fn text(conn: &Connection, sql: &str) -> String {
    value(conn, sql).expect("a value")
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).expect(sql)
}

fn clocks(conn: &Connection) -> Vec<(String, String, String, i64, Vec<u8>)> {
    let mut stmt = conn
        .prepare("SELECT tbl, pk, col, hlc, device FROM sync_clock ORDER BY tbl, pk, col")
        .expect("prepare");
    stmt.query_map([], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
    })
    .expect("query")
    .map(|r| r.expect("row"))
    .collect()
}

fn stock(conn: &Connection, id: &str, ticker: &str, quantity: &str) {
    merged(
        conn,
        &format!(
            "INSERT INTO stock_investments (id, ticker, company_name, quantity, currency) \
             VALUES ('{id}', '{ticker}', '{ticker} Inc', '{quantity}', 'USD')"
        ),
    );
}

fn stock_tx(
    conn: &Connection,
    id: &str,
    owner: &str,
    ticker: &str,
    kind: &str,
    qty: &str,
    day: i64,
) {
    merged(
        conn,
        &format!(
            "INSERT INTO investment_transactions (id, investment_id, type, ticker, company_name, \
               quantity, price_per_unit, currency, transaction_date, entry_quantity, \
               entry_price_per_unit) \
             VALUES ('{id}', '{owner}', '{kind}', '{ticker}', '{ticker} Inc', '{qty}', '100', \
               'USD', {day}, '{qty}', '100')"
        ),
    );
}

#[test]
fn oversold_merged_history_commits_with_the_unchecked_quantity() {
    let conn = enabled();
    stock(&conn, "s1", "AAPL", "15");
    stock_tx(&conn, "t1", "s1", "AAPL", "buy", "15", DAY);
    stock_tx(&conn, "t2", "s1", "AAPL", "sell", "10", NEXT_DAY);
    stock_tx(&conn, "t3", "s1", "AAPL", "sell", "10", NEXT_DAY + 86_400);
    let mut touched = Touched::default();
    for pk in ["t1", "t2", "t3"] {
        touched.row("investment_transactions", pk);
    }
    touched.parent("stock_investments", "s1");
    touched.tickers.insert("AAPL".into());

    let first = after_merge(&conn, &touched, NOW).expect("after_merge");
    let quantity = "SELECT quantity FROM stock_investments WHERE id = 's1'";
    assert_eq!(text(&conn, quantity), "0");
    assert_eq!(first.warnings.len(), 1, "{:?}", first.warnings);
    assert!(first.warnings[0].contains("s1"), "{:?}", first.warnings);

    let second = after_merge(&conn, &touched, NOW).expect("second run");
    assert_eq!(text(&conn, quantity), "0");
    assert_eq!(second, first);
}

/// An oversold ticker with an applied split: the units are still derived from the entry values
/// (ruling T18c: they must not depend on whether the holdings check passes), and the position
/// gets the unchecked quantity with one warning, not two.
#[test]
fn an_oversold_ticker_with_splits_warns_once() {
    let conn = enabled();
    stock(&conn, "s1", "AAPL", "15");
    stock_tx(&conn, "t1", "s1", "AAPL", "buy", "15", DAY);
    stock_tx(&conn, "t2", "s1", "AAPL", "sell", "10", NEXT_DAY);
    stock_tx(&conn, "t3", "s1", "AAPL", "sell", "10", NEXT_DAY + 86_400);
    split(&conn, "sp1", "AAPL", "2", "applied");
    let touched = ticker_touched(&["AAPL"]);

    let report = after_merge(&conn, &touched, NOW).expect("after_merge");
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
    assert!(report.warnings[0].contains("s1"), "{:?}", report.warnings);
    assert_eq!(
        text(
            &conn,
            "SELECT quantity FROM stock_investments WHERE id = 's1'"
        ),
        "0"
    );
    assert_eq!(
        units(&conn, "t1"),
        ("30".into(), "50".into(), Some(r#"["sp1"]"#.into()))
    );
    assert_eq!(
        units(&conn, "t2"),
        ("20".into(), "50".into(), Some(r#"["sp1"]"#.into()))
    );
}

#[test]
fn failing_recalculation_keeps_the_value_and_warns() {
    let conn = enabled();
    merged(
        &conn,
        "INSERT INTO other_assets (id, name, quantity, market_price, currency, average_purchase_price) \
           VALUES ('o1', 'Gold', '2', '100', 'EUR', '95'); \
         INSERT INTO other_asset_transactions (id, asset_id, type, quantity, price_per_unit, currency, \
           transaction_date) VALUES ('ot1', 'o1', 'buy', '2', '100', 'USD', 1767225600); \
         DELETE FROM exchange_rate_history;",
    );
    let mut touched = Touched::default();
    touched.row("other_asset_transactions", "ot1");
    touched.parent("other_assets", "o1");

    let report = after_merge(&conn, &touched, NOW).expect("after_merge");
    assert_eq!(
        text(
            &conn,
            "SELECT average_purchase_price FROM other_assets WHERE id = 'o1'"
        ),
        "95"
    );
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
    assert!(report.warnings[0].contains("o1"), "{:?}", report.warnings);
}

#[test]
fn typed_values_without_source_rows_survive_repair() {
    let conn = enabled();
    exec(
        &conn,
        "INSERT INTO bank_accounts (id, name, currency, balance) VALUES ('a1', 'Main', 'CZK', '1234'); \
         INSERT INTO real_estate (id, name, address, type, market_price) \
           VALUES ('r1', 'Flat', 'Street 1', 'apartment', '5000000'); \
         INSERT INTO stock_investments (id, ticker, company_name, quantity, currency) \
           VALUES ('s1', 'MSFT', 'Microsoft', '7', 'USD');",
    );
    let report = full(&conn, NOW).expect("full");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(
        text(&conn, "SELECT balance FROM bank_accounts WHERE id = 'a1'"),
        "1234"
    );
    assert_eq!(
        text(
            &conn,
            "SELECT market_price FROM real_estate WHERE id = 'r1'"
        ),
        "5000000"
    );
    assert_eq!(
        text(
            &conn,
            "SELECT quantity FROM stock_investments WHERE id = 's1'"
        ),
        "7"
    );
}

#[test]
fn mirrors_follow_the_merged_source_rows() {
    let conn = enabled();
    merged(
        &conn,
        &format!(
            "INSERT INTO bank_accounts (id, name, currency, balance) VALUES ('a1', 'Main', 'CZK', '1'); \
             INSERT INTO bank_balance_records (id, bank_account_id, amount, currency, as_of) VALUES \
               ('b1', 'a1', '100', 'CZK', {DAY}), ('b2', 'a1', '250', 'CZK', {NEXT_DAY}); \
             INSERT INTO real_estate (id, name, address, type, market_price) \
               VALUES ('r1', 'Flat', 'Street 1', 'apartment', '1'); \
             INSERT INTO real_estate_valuations (id, real_estate_id, value, currency, valued_at) \
               VALUES ('v1', 'r1', '2000000', 'EUR', {DAY}); \
             INSERT INTO loans (id, name, principal) VALUES ('l1', 'Car', '100000'); \
             INSERT INTO loan_events (id, loan_id, kind, event_day, amount) \
               VALUES ('e1', 'l1', 'balance_check', {DAY}, '90000'); \
             INSERT INTO crypto_investments (id, ticker, name, quantity, currency) \
               VALUES ('c1', 'BTC', 'Bitcoin', '0', 'CZK'); \
             INSERT INTO crypto_transactions (id, investment_id, type, ticker, name, quantity, \
               price_per_unit, currency, transaction_date) \
               VALUES ('ct1', 'c1', 'buy', 'BTC', 'Bitcoin', '0.5', '60000', 'usd', {DAY});"
        ),
    );
    stock(&conn, "s1", "MSFT", "0");
    stock_tx(&conn, "t1", "s1", "MSFT", "buy", "3", DAY);
    stock_tx(&conn, "t2", "s1", "MSFT", "buy", "2", NEXT_DAY);
    let mut touched = Touched::default();
    for (table, id) in [
        ("bank_accounts", "a1"),
        ("real_estate", "r1"),
        ("loans", "l1"),
        ("crypto_investments", "c1"),
        ("stock_investments", "s1"),
    ] {
        touched.parent(table, id);
    }

    let report = after_merge(&conn, &touched, NOW).expect("after_merge");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(
        text(&conn, "SELECT balance FROM bank_accounts WHERE id = 'a1'"),
        "250"
    );
    assert_eq!(
        count(
            &conn,
            "SELECT balance_as_of FROM bank_accounts WHERE id = 'a1'"
        ),
        NEXT_DAY
    );
    assert_eq!(
        text(
            &conn,
            "SELECT market_price || ' ' || market_price_currency FROM real_estate WHERE id = 'r1'"
        ),
        "2000000 EUR"
    );
    assert_eq!(
        text(
            &conn,
            "SELECT balance_anchor_amount FROM loans WHERE id = 'l1'"
        ),
        "90000"
    );
    assert_eq!(
        count(
            &conn,
            "SELECT balance_anchor_date FROM loans WHERE id = 'l1'"
        ),
        DAY
    );
    assert_eq!(
        text(
            &conn,
            "SELECT quantity || ' ' || currency FROM crypto_investments WHERE id = 'c1'"
        ),
        "0.5 USD"
    );
    assert_eq!(
        text(
            &conn,
            "SELECT quantity FROM stock_investments WHERE id = 's1'"
        ),
        "5"
    );
}

#[test]
fn one_tag_per_group_keeps_the_newest_assignment() {
    let conn = enabled();
    exec(
        &conn,
        "INSERT INTO stock_investments (id, ticker, company_name, quantity, currency) \
           VALUES ('s1', 'MSFT', 'Microsoft', '1', 'USD'); \
         INSERT INTO stock_tag_groups (id, name) VALUES ('g1', 'Sector'); \
         INSERT INTO stock_tags (id, name, group_id) VALUES \
           ('t-a', 'Energy', 'g1'), ('t-b', 'Tech', 'g1'), ('t-free', 'Watch', NULL); \
         INSERT INTO stock_investment_tags (investment_id, tag_id) VALUES ('s1', 't-a'); \
         INSERT INTO stock_investment_tags (investment_id, tag_id) VALUES ('s1', 't-b'); \
         INSERT INTO stock_investment_tags (investment_id, tag_id) VALUES ('s1', 't-free'); \
         DELETE FROM sync_outbox;",
    );
    let winner = row_clock(&conn, "stock_investment_tags", r#"["s1","t-b"]"#).unwrap();
    let loser = row_clock(&conn, "stock_investment_tags", r#"["s1","t-a"]"#).unwrap();
    assert!(winner > loser);
    let mut touched = Touched::default();
    touched.row("stock_investment_tags", r#"["s1","t-b"]"#);

    after_merge(&conn, &touched, NOW).expect("after_merge");
    let mut stmt = conn
        .prepare("SELECT tag_id FROM stock_investment_tags ORDER BY tag_id")
        .unwrap();
    let tags: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(tags, vec!["t-b".to_string(), "t-free".to_string()]);
    assert_eq!(
        count(
            &conn,
            "SELECT hlc FROM sync_clock WHERE tbl = 'stock_investment_tags' \
             AND pk = '[\"s1\",\"t-a\"]' AND col = '-'"
        ),
        winner.hlc as i64
    );
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_outbox"), 0);
}

fn event_ids(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT id FROM loan_events ORDER BY id")
        .unwrap();
    stmt.query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect()
}

/// The newest record has the higher id and the older `created_at`, so neither "lowest id wins"
/// nor the anchor mirror's own order would keep it.
#[test]
fn one_balance_check_per_day_keeps_the_newest() {
    let conn = enabled();
    exec(
        &conn,
        &format!(
            "INSERT INTO loans (id, name, principal) VALUES ('l1', 'Car', '100000'); \
             INSERT INTO loan_events (id, loan_id, kind, event_day, amount, created_at) \
               VALUES ('e-a', 'l1', 'balance_check', {DAY}, '100', 2); \
             INSERT INTO loan_events (id, loan_id, kind, event_day, amount, created_at) \
               VALUES ('e-z', 'l1', 'balance_check', {DAY}, '200', 1); \
             UPDATE loans SET balance_anchor_amount = '100', balance_anchor_date = {DAY}; \
             DELETE FROM sync_outbox;"
        ),
    );
    let winner = row_clock(&conn, "loan_events", "e-z").unwrap();
    assert!(winner > row_clock(&conn, "loan_events", "e-a").unwrap());
    let mut touched = Touched::default();
    touched.row("loan_events", "e-z");

    after_merge(&conn, &touched, NOW).expect("after_merge");
    assert_eq!(event_ids(&conn), vec!["e-z".to_string()]);
    assert_eq!(
        text(
            &conn,
            "SELECT balance_anchor_amount FROM loans WHERE id = 'l1'"
        ),
        "200"
    );
    assert_eq!(
        count(
            &conn,
            "SELECT hlc FROM sync_clock WHERE tbl = 'loan_events' AND pk = 'e-a' AND col = '-'"
        ),
        winner.hlc as i64
    );
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_outbox"), 0);
}

/// Equal row clocks: the lowest id stays, whatever order the rows were written in.
#[test]
fn one_balance_check_per_day_ties_keep_the_lowest_id() {
    let conn = enabled();
    merged(
        &conn,
        &format!(
            "INSERT INTO loans (id, name, principal) VALUES ('l1', 'Car', '100000'); \
             INSERT INTO loan_events (id, loan_id, kind, event_day, amount, created_at) \
               VALUES ('e-z', 'l1', 'balance_check', {DAY}, '200', 2); \
             INSERT INTO loan_events (id, loan_id, kind, event_day, amount, created_at) \
               VALUES ('e-b', 'l1', 'balance_check', {DAY}, '100', 1); \
             INSERT INTO sync_clock (tbl, pk, col, hlc, device) VALUES \
               ('loan_events', 'e-z', '*', 500, x'09090909090909090909090909090909'), \
               ('loan_events', 'e-b', '*', 500, x'09090909090909090909090909090909');"
        ),
    );
    let mut touched = Touched::default();
    touched.row("loan_events", "e-z");

    after_merge(&conn, &touched, NOW).expect("after_merge");
    assert_eq!(event_ids(&conn), vec!["e-b".to_string()]);
    assert_eq!(
        text(
            &conn,
            "SELECT balance_anchor_amount FROM loans WHERE id = 'l1'"
        ),
        "100"
    );
    assert_eq!(
        count(
            &conn,
            "SELECT hlc FROM sync_clock WHERE tbl = 'loan_events' AND pk = 'e-z' AND col = '-'"
        ),
        500
    );
}

#[test]
fn budget_versions_are_normalised_and_the_goal_row_rebuilt() {
    let conn = enabled();
    merged(
        &conn,
        &format!(
            "INSERT INTO transaction_categories (id, name) VALUES ('cat-x', 'Kids'); \
             INSERT INTO budget_goal_versions (id, category_id, timeframe, effective_month, amount, \
               currency) VALUES \
               ('v1', 'cat-x', 'monthly', {DAY}, '100', 'CZK'), \
               ('v2', 'cat-x', 'monthly', {FEB}, '100', 'CZK');"
        ),
    );
    let mut touched = Touched::default();
    touched
        .budget_keys
        .insert(("cat-x".to_string(), "monthly".to_string()));

    let report = after_merge(&conn, &touched, NOW).expect("after_merge");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(
        text(&conn, "SELECT group_concat(id) FROM budget_goal_versions"),
        "v1"
    );
    assert_eq!(
        text(
            &conn,
            "SELECT amount || ' ' || currency FROM budget_goals \
             WHERE category_id = 'cat-x' AND timeframe = 'monthly'"
        ),
        "100 CZK"
    );
}

fn split(conn: &Connection, id: &str, ticker: &str, numerator: &str, status: &str) {
    merged(
        conn,
        &format!(
            "INSERT INTO stock_splits (id, ticker, effective_day, numerator, denominator, source, \
               status) VALUES ('{id}', '{ticker}', {}, '{numerator}', '1', 'yahoo', '{status}')",
            DAY + 10 * 86_400
        ),
    );
}

fn units(conn: &Connection, id: &str) -> (String, String, Option<String>) {
    conn.query_row(
        "SELECT quantity, price_per_unit, split_basis FROM investment_transactions WHERE id = ?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .unwrap()
}

fn ticker_touched(tickers: &[&str]) -> Touched {
    let mut touched = Touched::default();
    touched
        .tickers
        .extend(tickers.iter().map(|t| t.to_string()));
    touched
}

#[test]
fn split_units_follow_the_merged_status() {
    let conn = enabled();
    stock(&conn, "s1", "AAPL", "10");
    stock_tx(&conn, "t1", "s1", "AAPL", "buy", "10", DAY);
    split(&conn, "sp1", "AAPL", "2", "applied");
    let touched = ticker_touched(&["AAPL"]);

    let report = after_merge(&conn, &touched, NOW).expect("apply");
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(
        units(&conn, "t1"),
        ("20".into(), "50".into(), Some(r#"["sp1"]"#.into()))
    );
    assert_eq!(
        text(
            &conn,
            "SELECT quantity FROM stock_investments WHERE id = 's1'"
        ),
        "20"
    );

    merged(
        &conn,
        "UPDATE stock_splits SET status = 'dismissed' WHERE id = 'sp1'",
    );
    after_merge(&conn, &touched, NOW).expect("dismiss");
    assert_eq!(units(&conn, "t1"), ("10".into(), "100".into(), None));
    assert_eq!(
        text(
            &conn,
            "SELECT quantity FROM stock_investments WHERE id = 's1'"
        ),
        "10"
    );
}

/// Task 7 obligation (a): an entry basis naming a split that has no row (deleted on one device
/// while another applied it) is tolerated: no error, no loop; the unknown ID scales nothing and
/// is not part of the derived `split_basis` (ruling T18c).
#[test]
fn an_entry_basis_naming_a_missing_split_is_left_alone() {
    let conn = enabled();
    stock(&conn, "s1", "AAPL", "10");
    stock_tx(&conn, "t1", "s1", "AAPL", "buy", "10", DAY);
    merged(
        &conn,
        r#"UPDATE investment_transactions SET entry_basis = '["ghost"]' WHERE id = 't1'"#,
    );
    split(&conn, "sp1", "AAPL", "2", "applied");
    let touched = ticker_touched(&["AAPL"]);

    let first = after_merge(&conn, &touched, NOW).expect("first run");
    assert!(first.warnings.is_empty(), "{:?}", first.warnings);
    let expected = (
        "20".to_string(),
        "50".to_string(),
        Some(r#"["sp1"]"#.to_string()),
    );
    assert_eq!(units(&conn, "t1"), expected);

    let second = after_merge(&conn, &touched, NOW).expect("second run");
    assert!(second.warnings.is_empty(), "{:?}", second.warnings);
    assert_eq!(units(&conn, "t1"), expected);
}

/// Task 7 obligation (b): a merged split row with an unparseable ratio fails only its ticker:
/// a warning, that ticker's reconcile skipped, everything else repaired.
#[test]
fn an_unreadable_split_skips_its_ticker_and_warns() {
    let conn = enabled();
    stock(&conn, "s1", "AAPL", "10");
    stock_tx(&conn, "t1", "s1", "AAPL", "buy", "10", DAY);
    split(&conn, "sp-bad", "AAPL", "abc", "applied");
    stock(&conn, "s2", "MSFT", "10");
    stock_tx(&conn, "t2", "s2", "MSFT", "buy", "10", DAY);
    split(&conn, "sp-ok", "MSFT", "2", "applied");
    let touched = ticker_touched(&["AAPL", "MSFT"]);

    let report = after_merge(&conn, &touched, NOW).expect("after_merge");
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
    assert!(report.warnings[0].contains("AAPL"), "{:?}", report.warnings);
    assert_eq!(units(&conn, "t1"), ("10".into(), "100".into(), None));
    assert_eq!(
        units(&conn, "t2"),
        ("20".into(), "50".into(), Some(r#"["sp-ok"]"#.into()))
    );
}

/// Task 7 obligation (b), the reachable case: a merged `entry_basis` that is not JSON is
/// reported by `stock_splits` as a database error, so without the pre-check it would abort the
/// whole merge. It must fail only its ticker.
#[test]
fn an_unreadable_entry_basis_skips_its_ticker_and_warns() {
    let conn = enabled();
    stock(&conn, "s1", "AAPL", "10");
    stock_tx(&conn, "t1", "s1", "AAPL", "buy", "10", DAY);
    merged(
        &conn,
        "UPDATE investment_transactions SET entry_basis = 'not-json' WHERE id = 't1'",
    );
    split(&conn, "sp-a", "AAPL", "2", "applied");
    stock(&conn, "s2", "MSFT", "10");
    stock_tx(&conn, "t2", "s2", "MSFT", "buy", "10", DAY);
    split(&conn, "sp-ok", "MSFT", "2", "applied");
    let touched = ticker_touched(&["AAPL", "MSFT"]);

    let report = after_merge(&conn, &touched, NOW).expect("after_merge");
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
    assert!(report.warnings[0].contains("AAPL"), "{:?}", report.warnings);
    assert_eq!(units(&conn, "t1"), ("10".into(), "100".into(), None));
    assert_eq!(
        units(&conn, "t2"),
        ("20".into(), "50".into(), Some(r#"["sp-ok"]"#.into()))
    );
}

#[test]
fn touched_domains_are_queued_with_the_earliest_day() {
    let conn = enabled();
    let mut touched = Touched::default();
    touched.day("bank-accounts", Some(NEXT_DAY));
    touched.day("bank-accounts", Some(DAY));
    touched.day("bank-accounts", Some(FEB));
    touched.day("loans", Some(DAY));
    touched.day("loans", None);
    touched.day("loans", Some(FEB));

    let report = after_merge(&conn, &touched, NOW).expect("after_merge");
    let pending = mcp_refresh::pending(&conn).unwrap();
    assert_eq!(
        pending
            .iter()
            .find(|j| j.domain == "bank-accounts")
            .unwrap()
            .earliest_date,
        Some(DAY)
    );
    assert_eq!(
        pending
            .iter()
            .find(|j| j.domain == "loans")
            .unwrap()
            .earliest_date,
        None
    );
    assert_eq!(pending.len(), 2);
    assert_eq!(
        report.domains,
        BTreeMap::from([("bank-accounts", Some(DAY)), ("loans", None)])
    );
}

#[test]
fn domain_of_names_the_refresh_domain_and_day_column() {
    assert_eq!(
        domain_of("bank_balance_records"),
        Some(("bank-accounts", Some("as_of")))
    );
    assert_eq!(
        domain_of("stock_splits"),
        Some(("stocks", Some("effective_day")))
    );
    assert_eq!(domain_of("bonds"), Some(("bonds", None)));
    assert_eq!(domain_of("transaction_categories"), None);
}

#[test]
fn repairs_record_no_clock_and_queue_nothing() {
    let conn = enabled();
    exec(
        &conn,
        "INSERT INTO bank_accounts (id, name, currency, balance) VALUES ('a1', 'Main', 'CZK', '1'); \
         INSERT INTO loans (id, name, principal) VALUES ('l1', 'Car', '100000'); \
         INSERT INTO stock_tags (id, name) VALUES ('tag-a', 'Growth');",
    );
    merged(
        &conn,
        &format!(
            "INSERT INTO bank_balance_records (id, bank_account_id, amount, currency, as_of) \
               VALUES ('b1', 'a1', '100', 'CZK', {DAY}); \
             INSERT INTO loan_events (id, loan_id, kind, event_day, amount) \
               VALUES ('e1', 'l1', 'balance_check', {DAY}, '90000'); \
             INSERT INTO transaction_categories (id, name) VALUES ('c1', 'Děti'), ('c2', 'děti');"
        ),
    );
    stock(&conn, "s1", "AAPL", "0");
    stock_tx(&conn, "t1", "s1", "AAPL", "buy", "10", DAY);
    split(&conn, "sp1", "AAPL", "2", "applied");
    exec(&conn, "DELETE FROM sync_outbox");
    let before = clocks(&conn);

    let mut touched = Touched::default();
    touched.parent("bank_accounts", "a1");
    touched.parent("loans", "l1");
    touched.parent("stock_investments", "s1");
    touched.tickers.insert("AAPL".into());
    touched
        .rename_scopes
        .insert(("transaction_categories".into(), String::new()));
    let report = after_merge(&conn, &touched, NOW).expect("after_merge");

    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(
        text(&conn, "SELECT balance FROM bank_accounts WHERE id = 'a1'"),
        "100"
    );
    assert_eq!(units(&conn, "t1").0, "20");
    assert_eq!(
        text(
            &conn,
            "SELECT name FROM transaction_categories WHERE id = 'c2'"
        ),
        "děti (2)"
    );
    assert_eq!(clocks(&conn), before);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_outbox"), 0);
    assert_eq!(capture::mode(&conn).unwrap(), Mode::Local);
}

#[test]
fn a_seeded_row_with_a_tombstone_is_deleted_again() {
    let conn = enabled();
    let seed = "SELECT * FROM transaction_categories WHERE id = 'cat_entertainment'";
    let row: (String, Option<String>, Option<String>, i64, i64, String) = conn
        .query_row(
            "SELECT name, icon, color, sort_order, is_system, kind FROM transaction_categories \
             WHERE id = 'cat_entertainment'",
            [],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .expect(seed);
    exec(
        &conn,
        "DELETE FROM transaction_categories WHERE id = 'cat_entertainment'",
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM sync_clock WHERE tbl = 'transaction_categories' \
             AND pk = 'cat_entertainment' AND col = '-'"
        ),
        1
    );
    capture::uninstall(&conn).unwrap();
    conn.execute(
        "INSERT INTO transaction_categories (id, name, icon, color, sort_order, is_system, kind) \
         VALUES ('cat_entertainment', ?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![row.0, row.1, row.2, row.3, row.4, row.5],
    )
    .unwrap();

    assert_eq!(delete_revived_seeds(&conn).unwrap(), 1);
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM transaction_categories WHERE id = 'cat_entertainment'"
        ),
        0
    );
    assert_eq!(delete_revived_seeds(&conn).unwrap(), 0);
}

#[test]
fn a_row_inserted_after_its_tombstone_is_not_a_revived_seed() {
    let conn = enabled();
    exec(
        &conn,
        "INSERT INTO institutions (id, name) VALUES ('i1', 'Bank'); \
         DELETE FROM institutions WHERE id = 'i1'; \
         INSERT INTO institutions (id, name) VALUES ('i1', 'Bank');",
    );
    capture::uninstall(&conn).unwrap();
    assert_eq!(delete_revived_seeds(&conn).unwrap(), 0);
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM institutions WHERE id = 'i1'"),
        1
    );
}

#[test]
fn opening_a_synced_profile_deletes_revived_seeds_before_capture() {
    let conn = enabled();
    exec(
        &conn,
        "INSERT INTO institutions (id, name) VALUES ('i1', 'Bank'); \
         DELETE FROM institutions WHERE id = 'i1';",
    );
    capture::uninstall(&conn).unwrap();
    exec(
        &conn,
        "INSERT INTO institutions (id, name) VALUES ('i1', 'Bank')",
    );
    exec(&conn, "DELETE FROM sync_outbox");

    crate::services::sync::on_open(&conn);
    assert!(capture::is_installed(&conn).unwrap());
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM institutions WHERE id = 'i1'"),
        0
    );
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_outbox"), 0);
}

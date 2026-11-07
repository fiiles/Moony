//! Split units across devices (D27, rulings T18c and its fix 1): the current units of a split
//! side row derive from its entry values and the time they were entered (`entry_at`), so a later
//! edit of another column (the watch notes) never changes which splits the target includes.

use super::*;
use crate::models::stock_monitor::InsertWatchedStock;
use crate::services::{stock_monitor, stock_splits};

fn target(conn: &Connection) -> Option<String> {
    value(
        conn,
        "SELECT target_price FROM watched_stocks WHERE ticker = 'AAPL'",
    )
}

/// The target is set, a 4:1 split dated after it is applied, and later the notes are edited
/// (`updated_at` moves past the ex-date). The target keeps the split on the author after its
/// next reconcile and on a receiver, which derives at once.
#[test]
fn a_notes_edit_after_an_apply_keeps_the_target_on_author_and_receiver() {
    let a = device(A);
    let b = device(B);
    stock_monitor::follow_stock(
        &a,
        &InsertWatchedStock {
            ticker: "AAPL".into(),
        },
    )
    .expect("follow");
    stock_monitor::set_target_price(&a, "AAPL", Some("100".into()), Some("above".into()))
        .expect("target");
    // An ex-date after the target was entered (the services stamp the real clock).
    let ex_date = (chrono::Utc::now().timestamp() / 86_400 + 10) * 86_400;
    let split = natural_id("stock_splits", &["AAPL", &ex_date.to_string()]);
    exec(
        &a,
        &format!(
            "INSERT INTO stock_splits (id, ticker, effective_day, numerator, denominator, source, \
               status) VALUES ('{split}', 'AAPL', {ex_date}, '4', '1', 'manual', 'pending')"
        ),
    );
    stock_splits::apply(&a, &split).expect("apply");
    assert_eq!(target(&a).as_deref(), Some("25"));
    apply(&b, &send(&a));
    assert_eq!(target(&b).as_deref(), Some("25"));

    // The notes are edited after the ex-date, as `update_notes` does then.
    exec(
        &a,
        &format!(
            "UPDATE watched_stocks SET notes = 'later', updated_at = {} WHERE ticker = 'AAPL'",
            ex_date + 86_400
        ),
    );
    stock_splits::reconcile_ticker(&a, "AAPL").expect("the author's next reconcile");
    apply(&b, &send(&a));

    for conn in [&a, &b] {
        assert_eq!(target(conn).as_deref(), Some("25"));
        assert_eq!(
            value(conn, "SELECT notes FROM watched_stocks").as_deref(),
            Some("later")
        );
    }
    assert_converged(&[&a, &b]);
}

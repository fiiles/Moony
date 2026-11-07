//! The unlink rule (spec §7.3, ruling U3): among the cash-flow items whose typed
//! `replacement_source_id` names the same source, the lowest id shows the link and every other one
//! shows NULL. The typed link is what syncs (U4); `sync_typed` keeps it while the row shows NULL,
//! so deleting or relinking the item that holds the link gives it back. A pure function of the
//! typed links, run after every merge (`invariants`) and every local write.

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, Connection};

use crate::error::Result;
use crate::services::sync::capture::{self, Mode};

/// `(table, column)` of every link the index allows only once.
pub const LINKS: [(&str, &str); 1] = [("cashflow_items", "replacement_source_id")];

/// The link every row shows, from the typed links `(pk, typed)`: in pk order, the first row of
/// each source keeps it, later ones show `None`. Independent of input order.
pub fn assign_links(rows: &[(String, Option<String>)]) -> Vec<(String, Option<String>)> {
    let mut sorted = rows.to_vec();
    sorted.sort();
    let mut taken = BTreeSet::new();
    sorted
        .into_iter()
        .map(|(pk, typed)| {
            let shown = typed.filter(|source| taken.insert(source.clone()));
            (pk, shown)
        })
        .collect()
}

/// Re-applies the unlink rule to every link column. The typed link of a row is its
/// `sync_typed.typed` while the stored value is still the one a repair assigned, else the stored
/// value. Changed links are written in `repair` mode in two passes (cleared first, then set), so
/// the unique index never trips. Returns how many rows changed.
pub fn recompute(conn: &Connection) -> Result<usize> {
    let mut changed = 0;
    for (table, column) in LINKS {
        let stored: Vec<(String, Option<String>)> = {
            let mut stmt =
                conn.prepare(&format!("SELECT id, {column} FROM {table} ORDER BY id"))?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<std::result::Result<_, _>>()?;
            rows
        };
        let entries: BTreeMap<String, (String, Option<String>)> = {
            let mut stmt = conn.prepare(
                "SELECT pk, typed, assigned FROM sync_typed WHERE tbl = ?1 AND col = ?2",
            )?;
            let rows = stmt
                .query_map([table, column], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?
                .collect::<std::result::Result<_, _>>()?;
            rows
        };
        let typed: Vec<(String, Option<String>)> = stored
            .iter()
            .map(|(pk, current)| match entries.get(pk) {
                Some((typed, assigned)) if assigned == current => (pk.clone(), Some(typed.clone())),
                _ => (pk.clone(), current.clone()),
            })
            .collect();
        let shown = assign_links(&typed);
        let changes: Vec<(&str, Option<&str>)> = stored
            .iter()
            .zip(&shown)
            .filter(|((_, current), (_, link))| current != link)
            .map(|((pk, _), (_, link))| (pk.as_str(), link.as_deref()))
            .collect();
        capture::with_mode(conn, Mode::Repair, || {
            let update = format!("UPDATE {table} SET {column} = ?2 WHERE id = ?1");
            for (pk, _) in &changes {
                conn.execute(&update, params![pk, Option::<&str>::None])?;
            }
            for (pk, link) in changes.iter().filter(|(_, link)| link.is_some()) {
                conn.execute(&update, params![pk, link])?;
            }
            Ok(())
        })?;
        for ((pk, typed), (_, link)) in typed.iter().zip(&shown) {
            super::set_typed(conn, table, pk, column, typed.as_deref(), link.as_deref())?;
        }
        super::forget_deleted(conn, table, column)?;
        changed += changes.len();
    }
    Ok(changed)
}

/// Re-applies the unlink rule after a local write (a deleted or relinked item may free a link).
/// `Ok(0)` when capture is not installed: a profile without sync never has repair-cleared links
/// (G5).
pub fn after_local_write(conn: &Connection) -> Result<usize> {
    if !capture::is_installed(conn)? {
        return Ok(0);
    }
    recompute(conn)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::sync::hlc::DeviceId;
    use crate::services::sync::state;
    use crate::services::sync::test_support::migrated;

    fn rows(pairs: &[(&str, Option<&str>)]) -> Vec<(String, Option<String>)> {
        pairs
            .iter()
            .map(|(pk, link)| (pk.to_string(), link.map(str::to_string)))
            .collect()
    }

    fn links(conn: &Connection) -> Vec<(String, Option<String>)> {
        let mut stmt = conn
            .prepare("SELECT id, replacement_source_id FROM cashflow_items ORDER BY id")
            .expect("prepare");
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .expect("query")
            .map(|r| r.expect("row"))
            .collect()
    }

    #[test]
    fn assign_links_keeps_the_lowest_id_per_source_in_any_order() {
        let expected = rows(&[
            ("a", Some("s1")),
            ("b", None),
            ("c", Some("s2")),
            ("d", None),
            ("e", None),
        ]);
        let input = rows(&[
            ("d", Some("s1")),
            ("b", Some("s1")),
            ("e", None),
            ("a", Some("s1")),
            ("c", Some("s2")),
        ]);
        assert_eq!(assign_links(&input), expected);
        let mut reversed = input.clone();
        reversed.reverse();
        assert_eq!(assign_links(&reversed), expected);
    }

    #[test]
    fn deleting_the_holder_gives_the_typed_link_back() {
        let conn = migrated();
        state::enable(&conn, DeviceId([7; 16])).expect("enable");
        conn.execute_batch(
            "INSERT INTO cashflow_items (id, name, amount, frequency, item_type, replacement_source_id) VALUES \
               ('src', 'S', '1', 'monthly', 'expense', NULL), \
               ('a', 'A', '1', 'monthly', 'expense', 'src'), \
               ('b', 'B', '1', 'monthly', 'expense', NULL); \
             INSERT INTO sync_typed (tbl, pk, col, typed, assigned) \
               VALUES ('cashflow_items', 'b', 'replacement_source_id', 'src', NULL);",
        )
        .expect("seed");
        assert_eq!(recompute(&conn).unwrap(), 0);

        conn.execute("DELETE FROM cashflow_items WHERE id = 'a'", [])
            .expect("delete");
        assert_eq!(after_local_write(&conn).unwrap(), 1);

        assert_eq!(links(&conn), rows(&[("b", Some("src")), ("src", None)]));
        let entries: i64 = conn
            .query_row("SELECT COUNT(*) FROM sync_typed", [], |r| r.get(0))
            .unwrap();
        assert_eq!(entries, 0);
    }

    #[test]
    fn links_without_sync_do_nothing() {
        let conn = migrated();
        assert_eq!(after_local_write(&conn).unwrap(), 0);
    }
}

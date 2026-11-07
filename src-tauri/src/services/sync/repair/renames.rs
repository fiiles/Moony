//! The rename rule (spec §7.3, §7.4, R9): among rows of one scope whose names fold to the same
//! key, the lowest ID keeps its typed name and every other one gets " (2)", " (3)"…. The ONE
//! rename function: the merge's unique-key rule, the repairs after a merge and a local write all
//! call it. `sync_typed` (with `col` the name column) remembers the name a person typed while a
//! repair shows another, so a rename that frees the name gives the typed one back, and the
//! journal sends the typed name (U4).

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, Connection};

use crate::error::{AppError, Result};
use crate::services::spending_plans::name_key;
use crate::services::sync::capture::{self, Mode};

/// One table whose names are kept apart by the rename rule.
pub struct RenameScope {
    pub table: &'static str,
    pub name_column: &'static str,
    /// The column that splits the table into independent scopes; `None` for the whole table.
    pub scope_column: Option<&'static str>,
    /// The key two names collide on.
    pub fold: fn(&str) -> String,
}

/// Tags and tag groups compare binary (their UNIQUE index does); spending plans per year and
/// categories case-insensitively, as their services do.
pub const RENAME_SCOPES: [RenameScope; 4] = [
    RenameScope {
        table: "stock_tags",
        name_column: "name",
        scope_column: None,
        fold: identity,
    },
    RenameScope {
        table: "stock_tag_groups",
        name_column: "name",
        scope_column: None,
        fold: identity,
    },
    RenameScope {
        table: "spending_plans",
        name_column: "name",
        scope_column: Some("year"),
        fold: name_key,
    },
    RenameScope {
        table: "transaction_categories",
        name_column: "name",
        scope_column: None,
        fold: category_fold,
    },
];

fn identity(name: &str) -> String {
    name.to_string()
}

/// As `categories::name_taken` compares names.
fn category_fold(name: &str) -> String {
    name.trim().to_lowercase()
}

/// The name every row of one scope shows, from the names people typed: `(pk, typed)` →
/// `(pk, name)`, ordered by pk. Rows are grouped by `fold(typed)` and the groups walked in
/// ascending fold order; in each group the lowest pk keeps its typed name and every further row
/// gets `typed + " (k)"` with the smallest `k ≥ 2` whose fold no typed or assigned name has yet.
/// A function of the set of rows only, so it is idempotent and independent of input order.
pub fn assign_names(rows: &[(String, String)], fold: fn(&str) -> String) -> Vec<(String, String)> {
    let mut taken: BTreeSet<String> = rows.iter().map(|(_, typed)| fold(typed)).collect();
    let mut groups: BTreeMap<String, Vec<(&str, &str)>> = BTreeMap::new();
    for (pk, typed) in rows {
        groups
            .entry(fold(typed))
            .or_default()
            .push((pk.as_str(), typed.as_str()));
    }
    let mut out = Vec::with_capacity(rows.len());
    for members in groups.values_mut() {
        members.sort();
        let mut members = members.iter();
        if let Some((pk, typed)) = members.next() {
            out.push((pk.to_string(), typed.to_string()));
        }
        for (pk, typed) in members {
            let mut k = 2_u64;
            let name = loop {
                let candidate = format!("{typed} ({k})");
                if !taken.contains(&fold(&candidate)) {
                    break candidate;
                }
                k += 1;
            };
            taken.insert(fold(&name));
            out.push((pk.to_string(), name));
        }
    }
    out.sort();
    out
}

/// Re-applies the rename rule to one scope (`scope_value` is ignored for a whole-table scope).
/// The typed name of a row is its `sync_typed.typed` while the stored name is still the one a
/// repair assigned, else the stored name (a person or a merge wrote it since). Changed names are
/// written in `repair` mode in two passes — every changing row first to a placeholder, then to
/// its final name — so the binary UNIQUE index of tags never trips. Returns how many names
/// changed.
pub fn recompute_scope(conn: &Connection, scope: &RenameScope, scope_value: &str) -> Result<usize> {
    let table = scope.table;
    let name = scope.name_column;
    let stored: Vec<(String, String)> = {
        let mut stmt = match scope.scope_column {
            Some(column) => conn.prepare(&format!(
                "SELECT id, {name} FROM {table} WHERE {column} = ?1 ORDER BY id"
            ))?,
            None => conn.prepare(&format!("SELECT id, {name} FROM {table} ORDER BY id"))?,
        };
        let map = |r: &rusqlite::Row| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?));
        let rows = match scope.scope_column {
            Some(_) => stmt.query_map([scope_value], map)?,
            None => stmt.query_map([], map)?,
        };
        rows.collect::<std::result::Result<_, _>>()?
    };
    let entries: BTreeMap<String, (String, Option<String>)> = {
        let mut stmt =
            conn.prepare("SELECT pk, typed, assigned FROM sync_typed WHERE tbl = ?1 AND col = ?2")?;
        let rows = stmt.query_map([table, name], |r| {
            Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?)))
        })?;
        rows.collect::<std::result::Result<_, _>>()?
    };
    let typed: Vec<(String, String)> = stored
        .iter()
        .map(|(pk, current)| match entries.get(pk) {
            Some((typed, Some(assigned))) if assigned == current => (pk.clone(), typed.clone()),
            _ => (pk.clone(), current.clone()),
        })
        .collect();
    let assigned = assign_names(&typed, scope.fold);

    let changes: Vec<(&str, &str)> = stored
        .iter()
        .zip(&assigned)
        .filter(|((_, current), (_, name))| current != name)
        .map(|((pk, _), (_, name))| (pk.as_str(), name.as_str()))
        .collect();
    capture::with_mode(conn, Mode::Repair, || {
        let update = format!("UPDATE {table} SET {name} = ?2 WHERE id = ?1");
        for (pk, _) in &changes {
            conn.execute(&update, params![pk, format!("\u{1f}{pk}")])?;
        }
        for (pk, final_name) in &changes {
            conn.execute(&update, params![pk, final_name])?;
        }
        Ok(())
    })?;

    for ((pk, typed), (_, assigned_name)) in typed.iter().zip(&assigned) {
        super::set_typed(conn, table, pk, name, Some(typed), Some(assigned_name))?;
    }
    super::forget_deleted(conn, table, name)?;
    Ok(changes.len())
}

/// The rename scope of `table`.
pub fn scope_of(table: &str) -> Result<&'static RenameScope> {
    RENAME_SCOPES
        .iter()
        .find(|s| s.table == table)
        .ok_or_else(|| AppError::Internal(format!("{table} has no rename scope")))
}

/// Re-applies the rename rule after a local write to `table` (its scope `scope_value`), so a
/// rename that frees a name restores the typed names on the author's device too. `Ok(0)` when
/// capture is not installed: a profile without sync never has repair-renamed rows (G5).
pub fn after_local_write(conn: &Connection, table: &str, scope_value: &str) -> Result<usize> {
    if !capture::is_installed(conn)? {
        return Ok(0);
    }
    recompute_scope(conn, scope_of(table)?, scope_value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::InsertStockTag;
    use crate::services::stock_tags;
    use crate::services::sync::hlc::DeviceId;
    use crate::services::sync::state;
    use crate::services::sync::test_support::migrated;

    const DEVICE: DeviceId = DeviceId([7; 16]);

    fn rows(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(pk, name)| (pk.to_string(), name.to_string()))
            .collect()
    }

    fn enabled() -> Connection {
        let conn = migrated();
        state::enable(&conn, DEVICE).expect("enable");
        conn
    }

    fn names(conn: &Connection, table: &str) -> Vec<(String, String)> {
        let mut stmt = conn
            .prepare(&format!("SELECT id, name FROM {table} ORDER BY id"))
            .expect("prepare");
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .expect("query")
            .map(|r| r.expect("row"))
            .collect()
    }

    fn renamed(conn: &Connection) -> Vec<(String, String, String, String)> {
        let mut stmt = conn
            .prepare("SELECT tbl, pk, typed, assigned FROM sync_typed WHERE col = 'name' ORDER BY tbl, pk")
            .expect("prepare");
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .expect("query")
            .map(|r| r.expect("row"))
            .collect()
    }

    #[test]
    fn assign_names_numbers_duplicates_by_id_and_skips_taken_names() {
        assert_eq!(
            assign_names(
                &rows(&[("b", "Holiday"), ("a", "holiday"), ("c", "Holiday (2)")]),
                name_key
            ),
            rows(&[("a", "holiday"), ("b", "Holiday (3)"), ("c", "Holiday (2)")])
        );
    }

    #[test]
    fn assign_names_gives_the_same_names_in_any_input_order() {
        let input = rows(&[("d", "G"), ("b", "g"), ("a", "G"), ("c", "G (2)")]);
        let expected = rows(&[("a", "G"), ("b", "g (3)"), ("c", "G (2)"), ("d", "G (4)")]);
        let mut permutations = 0;
        for i in 0..4 {
            for j in 0..4 {
                for k in 0..4 {
                    for l in 0..4 {
                        let order = [i, j, k, l];
                        let distinct: BTreeSet<_> = order.iter().collect();
                        if distinct.len() < 4 {
                            continue;
                        }
                        let permuted: Vec<_> = order.iter().map(|&n| input[n].clone()).collect();
                        assert_eq!(assign_names(&permuted, name_key), expected, "{order:?}");
                        permutations += 1;
                    }
                }
            }
        }
        assert_eq!(permutations, 24);
    }

    /// A tag arriving from another device with the typed name "G": the merge writes it under a
    /// placeholder and records the typed name, as `merge::unique` does, then the scope is
    /// recomputed.
    fn arrive(conn: &Connection, pk: &str) {
        conn.execute(
            "INSERT INTO stock_tags (id, name) VALUES (?1, char(31) || ?1)",
            [pk],
        )
        .expect("insert");
        conn.execute(
            "INSERT INTO sync_typed (tbl, pk, col, typed, assigned) \
             VALUES ('stock_tags', ?1, 'name', 'G', char(31) || ?1)",
            [pk],
        )
        .expect("renamed");
        capture::with_mode(conn, Mode::Repair, || {
            recompute_scope(conn, scope_of("stock_tags")?, "")
        })
        .expect("recompute");
    }

    #[test]
    fn renames_applied_in_two_orders_end_with_the_same_names() {
        let forward = enabled();
        let backward = enabled();
        for pk in ["a", "b", "c"] {
            arrive(&forward, pk);
        }
        for pk in ["c", "b", "a"] {
            arrive(&backward, pk);
        }
        let expected = vec![
            ("a".to_string(), "G".to_string()),
            ("b".to_string(), "G (2)".to_string()),
            ("c".to_string(), "G (3)".to_string()),
        ];
        assert_eq!(names(&forward, "stock_tags"), expected);
        assert_eq!(names(&backward, "stock_tags"), expected);
        let expected_renamed = vec![
            ("stock_tags".into(), "b".into(), "G".into(), "G (2)".into()),
            ("stock_tags".into(), "c".into(), "G".into(), "G (3)".into()),
        ];
        assert_eq!(renamed(&forward), expected_renamed);
        assert_eq!(renamed(&backward), expected_renamed);
    }

    #[test]
    fn a_rename_that_frees_a_name_restores_the_typed_name() {
        let conn = enabled();
        conn.execute_batch(
            "INSERT INTO stock_tags (id, name) VALUES ('a', 'G'), ('b', 'G (2)'); \
             INSERT INTO sync_typed (tbl, pk, col, typed, assigned) \
               VALUES ('stock_tags', 'b', 'name', 'G', 'G (2)');",
        )
        .expect("seed");
        stock_tags::update_tag(
            &conn,
            "a",
            InsertStockTag {
                name: "Value".into(),
                color: None,
                group_id: None,
            },
        )
        .expect("update_tag");
        assert_eq!(
            names(&conn, "stock_tags"),
            vec![
                ("a".to_string(), "Value".to_string()),
                ("b".to_string(), "G".to_string())
            ]
        );
        assert!(renamed(&conn).is_empty());
    }

    #[test]
    fn duplicate_category_names_fold_non_ascii_case() {
        let conn = enabled();
        conn.execute_batch(
            "INSERT INTO transaction_categories (id, name) VALUES ('c1', 'Děti'), ('c2', 'děti');",
        )
        .expect("seed");
        assert_eq!(
            recompute_scope(&conn, scope_of("transaction_categories").unwrap(), "").unwrap(),
            1
        );
        let names = names(&conn, "transaction_categories");
        assert!(names.contains(&("c1".to_string(), "Děti".to_string())));
        assert!(names.contains(&("c2".to_string(), "děti (2)".to_string())));
        assert_eq!(
            renamed(&conn),
            vec![(
                "transaction_categories".into(),
                "c2".into(),
                "děti".into(),
                "děti (2)".into()
            )]
        );
    }

    #[test]
    fn renames_without_sync_do_nothing() {
        let conn = migrated();
        conn.execute_batch(
            "INSERT INTO transaction_categories (id, name) VALUES ('c1', 'Děti'), ('c2', 'děti');",
        )
        .expect("seed");
        assert_eq!(
            after_local_write(&conn, "transaction_categories", "").unwrap(),
            0
        );
        assert!(renamed(&conn).is_empty());
        let names = names(&conn, "transaction_categories");
        assert!(names.contains(&("c2".to_string(), "děti".to_string())));
    }

    #[test]
    fn spending_plan_names_are_kept_apart_per_year_only() {
        let conn = enabled();
        conn.execute_batch(
            "INSERT INTO spending_plans (id, name, year, amount, currency, icon, color) VALUES \
               ('p1', 'Holiday', 2026, '1', 'CZK', 'i', 'c'), \
               ('p2', 'holiday', 2026, '1', 'CZK', 'i', 'c'), \
               ('p3', 'Holiday', 2027, '1', 'CZK', 'i', 'c');",
        )
        .expect("seed");
        assert_eq!(
            after_local_write(&conn, "spending_plans", "2026").unwrap(),
            1
        );
        assert_eq!(
            names(&conn, "spending_plans"),
            vec![
                ("p1".to_string(), "Holiday".to_string()),
                ("p2".to_string(), "holiday (2)".to_string()),
                ("p3".to_string(), "Holiday".to_string())
            ]
        );
    }
}

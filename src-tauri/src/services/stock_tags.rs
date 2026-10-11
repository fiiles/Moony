//! Stock tags: tag groups, tags and the assignment of tags to stock positions.
//!
//! A tag group is one partition of the portfolio, so a position carries at most one tag of each
//! group (`set_investment_tags` rejects a second one, `update_tag` rejects a move into a group
//! that would give a position two). Tags without a group stay free, overlapping labels.

use crate::error::{AppError, Result};
use crate::models::{InsertStockTag, InsertStockTagGroup, StockTag, StockTagGroup};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

fn group_row(row: &rusqlite::Row) -> rusqlite::Result<StockTagGroup> {
    Ok(StockTagGroup {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        created_at: row.get(3)?,
    })
}

fn tag_row(row: &rusqlite::Row) -> rusqlite::Result<StockTag> {
    Ok(StockTag {
        id: row.get(0)?,
        name: row.get(1)?,
        color: row.get(2)?,
        group_id: row.get(3)?,
        created_at: row.get(4)?,
    })
}

// ========================== Tag groups ==========================

/// Every tag group by name.
pub fn list_groups(conn: &Connection) -> Result<Vec<StockTagGroup>> {
    let mut stmt = conn
        .prepare("SELECT id, name, description, created_at FROM stock_tag_groups ORDER BY name")?;
    let groups = stmt
        .query_map([], group_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(groups)
}

/// Create a tag group; the name is stored trimmed.
pub fn create_group(conn: &Connection, data: InsertStockTagGroup) -> Result<StockTagGroup> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let name = data.name.trim().to_string();

    conn.execute(
        "INSERT INTO stock_tag_groups (id, name, description, created_at) VALUES (?1, ?2, ?3, ?4)",
        params![id, name, data.description, now],
    )?;

    Ok(StockTagGroup {
        id,
        name,
        description: data.description,
        created_at: now,
    })
}

/// Rename or re-describe a tag group; `NotFound` for an unknown id.
pub fn update_group(
    conn: &Connection,
    id: &str,
    data: InsertStockTagGroup,
) -> Result<StockTagGroup> {
    let changes = conn.execute(
        "UPDATE stock_tag_groups SET name = ?1, description = ?2 WHERE id = ?3",
        params![data.name.trim(), data.description, id],
    )?;

    if changes == 0 {
        return Err(AppError::NotFound("Stock tag group not found".into()));
    }

    conn.query_row(
        "SELECT id, name, description, created_at FROM stock_tag_groups WHERE id = ?1",
        [id],
        group_row,
    )
    .map_err(|_| AppError::NotFound("Stock tag group not found".into()))
}

/// Delete a tag group (its tags stay, without a group); `NotFound` for an unknown id.
pub fn delete_group(conn: &Connection, id: &str) -> Result<()> {
    let changes = conn.execute("DELETE FROM stock_tag_groups WHERE id = ?1", [id])?;
    if changes == 0 {
        return Err(AppError::NotFound("Stock tag group not found".into()));
    }
    Ok(())
}

// ========================== Tags ==========================

/// Every tag by name (with its group id).
pub fn list_tags(conn: &Connection) -> Result<Vec<StockTag>> {
    let mut stmt =
        conn.prepare("SELECT id, name, color, group_id, created_at FROM stock_tags ORDER BY name")?;
    let tags = stmt
        .query_map([], tag_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(tags)
}

/// Create a tag; the name is stored trimmed.
pub fn create_tag(conn: &Connection, data: InsertStockTag) -> Result<StockTag> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let name = data.name.trim().to_string();

    conn.execute(
        "INSERT INTO stock_tags (id, name, color, group_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, name, data.color, data.group_id, now],
    )?;

    Ok(StockTag {
        id,
        name,
        color: data.color,
        group_id: data.group_id,
        created_at: now,
    })
}

/// Rename, recolour or regroup a tag; `NotFound` for an unknown id. Moving the tag into another
/// group is rejected (`validation.tagGroupConflict`) when a position carrying this tag already
/// carries another tag of that group, because it would then hold two tags of one group.
pub fn update_tag(conn: &Connection, id: &str, data: InsertStockTag) -> Result<StockTag> {
    let stored_group: Option<Option<String>> = conn
        .query_row("SELECT group_id FROM stock_tags WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()?;
    let Some(stored_group) = stored_group else {
        return Err(AppError::NotFound("Stock tag not found".into()));
    };

    if let Some(target) = data.group_id.as_deref() {
        if stored_group.as_deref() != Some(target) {
            let conflicts: i64 = conn.query_row(
                "SELECT COUNT(*) FROM stock_investment_tags mine
                 JOIN stock_investment_tags other ON other.investment_id = mine.investment_id
                 JOIN stock_tags ot ON ot.id = other.tag_id
                 WHERE mine.tag_id = ?1 AND other.tag_id <> ?1 AND ot.group_id = ?2",
                params![id, target],
                |r| r.get(0),
            )?;
            if conflicts > 0 {
                return Err(AppError::Validation("validation.tagGroupConflict".into()));
            }
        }
    }

    conn.execute(
        "UPDATE stock_tags SET name = ?1, color = ?2, group_id = ?3 WHERE id = ?4",
        params![data.name.trim(), data.color, data.group_id, id],
    )?;

    conn.query_row(
        "SELECT id, name, color, group_id, created_at FROM stock_tags WHERE id = ?1",
        [id],
        tag_row,
    )
    .map_err(|_| AppError::NotFound("Stock tag not found".into()))
}

/// Delete a tag (its assignments go with it); `NotFound` for an unknown id.
pub fn delete_tag(conn: &Connection, id: &str) -> Result<()> {
    let changes = conn.execute("DELETE FROM stock_tags WHERE id = ?1", [id])?;
    if changes == 0 {
        return Err(AppError::NotFound("Stock tag not found".into()));
    }
    Ok(())
}

// ========================== Assignments ==========================

/// The tags of one position by name.
pub fn investment_tags(conn: &Connection, investment_id: &str) -> Result<Vec<StockTag>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.name, t.color, t.group_id, t.created_at
         FROM stock_tags t
         INNER JOIN stock_investment_tags sit ON t.id = sit.tag_id
         WHERE sit.investment_id = ?1
         ORDER BY t.name",
    )?;
    let tags = stmt
        .query_map([investment_id], tag_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(tags)
}

/// Fails with `validation.tagNotFound` for an unknown tag and `validation.oneTagPerGroup` when
/// two of `tag_ids` share a group (the one-tag-per-group rule).
fn check_one_per_group(conn: &Connection, tag_ids: &[String]) -> Result<()> {
    let mut seen: HashMap<String, &str> = HashMap::new();
    for id in tag_ids {
        let group: Option<Option<String>> = conn
            .query_row("SELECT group_id FROM stock_tags WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .optional()?;
        let Some(group) = group else {
            return Err(AppError::Validation("validation.tagNotFound".into()));
        };
        if let Some(group) = group {
            if seen.insert(group, id).is_some() {
                return Err(AppError::Validation("validation.oneTagPerGroup".into()));
            }
        }
    }
    Ok(())
}

/// Replace the tags of a position with `tag_ids` (duplicates ignored). `NotFound` for an
/// unknown position. Assignments that stay keep their `created_at`; only the removed ones are
/// deleted and only the new ones inserted, all in one transaction.
pub fn set_investment_tags(
    conn: &Connection,
    investment_id: &str,
    tag_ids: &[String],
) -> Result<()> {
    let exists = conn
        .query_row(
            "SELECT 1 FROM stock_investments WHERE id = ?1",
            [investment_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(AppError::NotFound("Stock investment not found".into()));
    }

    let mut seen = HashSet::new();
    let wanted: Vec<String> = tag_ids
        .iter()
        .filter(|id| seen.insert(id.as_str()))
        .cloned()
        .collect();
    check_one_per_group(conn, &wanted)?;

    let tx = conn.unchecked_transaction()?;
    let current: Vec<String> = {
        let mut stmt =
            tx.prepare("SELECT tag_id FROM stock_investment_tags WHERE investment_id = ?1")?;
        let rows = stmt
            .query_map([investment_id], |r| r.get(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows
    };
    for tag_id in current.iter().filter(|id| !seen.contains(id.as_str())) {
        tx.execute(
            "DELETE FROM stock_investment_tags WHERE investment_id = ?1 AND tag_id = ?2",
            params![investment_id, tag_id],
        )?;
    }
    for tag_id in &wanted {
        tx.execute(
            "INSERT OR IGNORE INTO stock_investment_tags (investment_id, tag_id, created_at)
             VALUES (?1, ?2, unixepoch())",
            params![investment_id, tag_id],
        )?;
    }
    tx.commit()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            CREATE TABLE stock_tag_groups (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                description TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE stock_tags (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL UNIQUE,
                color TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                group_id TEXT REFERENCES stock_tag_groups(id) ON DELETE SET NULL
            );
            CREATE TABLE stock_investments (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                company_name TEXT NOT NULL,
                quantity TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK'
            );
            CREATE TABLE stock_investment_tags (
                investment_id TEXT NOT NULL REFERENCES stock_investments(id) ON DELETE CASCADE,
                tag_id TEXT NOT NULL REFERENCES stock_tags(id) ON DELETE CASCADE,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                PRIMARY KEY (investment_id, tag_id)
            );
            "#,
        )
        .expect("schema");
        conn
    }

    /// Groups `g1` (Strategy) and `g2` (Region); tags `a`, `b` in `g1`, `c` in `g2`, `d` and `f`
    /// without a group; positions `i1` and `i2`.
    fn seeded() -> Connection {
        let conn = setup_test_db();
        conn.execute_batch(
            "INSERT INTO stock_tag_groups (id, name) VALUES ('g1', 'Strategy'), ('g2', 'Region');
             INSERT INTO stock_tags (id, name, group_id) VALUES
                ('a', 'Alfa', 'g1'), ('b', 'Beta', 'g1'), ('c', 'Gama', 'g2'),
                ('d', 'Delta', NULL), ('f', 'Foxtrot', NULL);
             INSERT INTO stock_investments (id, ticker, company_name, quantity, currency) VALUES
                ('i1', 'AAA', 'AAA Inc', '1', 'USD'), ('i2', 'BBB', 'BBB Inc', '1', 'USD');",
        )
        .expect("seed");
        conn
    }

    fn assigned(conn: &Connection, investment_id: &str) -> Vec<String> {
        conn.prepare(
            "SELECT tag_id FROM stock_investment_tags WHERE investment_id = ?1 ORDER BY tag_id",
        )
        .expect("select")
        .query_map([investment_id], |r| r.get(0))
        .expect("rows")
        .collect::<std::result::Result<Vec<String>, _>>()
        .expect("rows")
    }

    fn assign(conn: &Connection, investment_id: &str, tag_id: &str, created_at: i64) {
        conn.execute(
            "INSERT INTO stock_investment_tags (investment_id, tag_id, created_at) VALUES (?1, ?2, ?3)",
            params![investment_id, tag_id, created_at],
        )
        .expect("assign");
    }

    fn tag_input(name: &str, group_id: Option<&str>) -> InsertStockTag {
        InsertStockTag {
            name: name.to_string(),
            color: None,
            group_id: group_id.map(str::to_string),
        }
    }

    fn group_of(conn: &Connection, tag_id: &str) -> Option<String> {
        conn.query_row(
            "SELECT group_id FROM stock_tags WHERE id = ?1",
            [tag_id],
            |r| r.get(0),
        )
        .expect("tag")
    }

    fn validation_key(result: Result<()>) -> String {
        match result {
            Err(AppError::Validation(key)) => key,
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    // ---- set_investment_tags ----

    #[test]
    fn set_tags_rejects_two_tags_of_one_group() {
        let conn = seeded();
        assign(&conn, "i1", "d", 100);

        let key = validation_key(set_investment_tags(
            &conn,
            "i1",
            &["a".to_string(), "b".to_string()],
        ));

        assert_eq!(key, "validation.oneTagPerGroup");
        assert_eq!(assigned(&conn, "i1"), vec!["d"], "nothing written");
    }

    #[test]
    fn set_tags_keeps_created_at_of_kept_assignments() {
        let conn = seeded();
        assign(&conn, "i1", "a", 100);

        set_investment_tags(&conn, "i1", &["a".to_string(), "d".to_string()]).expect("set");

        let created = |tag: &str| -> i64 {
            conn.query_row(
                "SELECT created_at FROM stock_investment_tags WHERE investment_id = 'i1' AND tag_id = ?1",
                [tag],
                |r| r.get(0),
            )
            .expect("row")
        };
        assert_eq!(created("a"), 100, "kept assignment keeps its time");
        assert!(created("d") > 100, "new assignment is stamped now");
        assert_eq!(assigned(&conn, "i1"), vec!["a", "d"]);
    }

    #[test]
    fn set_tags_removes_unselected() {
        let conn = seeded();
        assign(&conn, "i1", "a", 100);
        assign(&conn, "i1", "d", 100);
        assign(&conn, "i2", "a", 100);

        set_investment_tags(&conn, "i1", &["d".to_string()]).expect("set");
        assert_eq!(assigned(&conn, "i1"), vec!["d"]);
        assert_eq!(
            assigned(&conn, "i2"),
            vec!["a"],
            "other positions untouched"
        );

        set_investment_tags(&conn, "i1", &[]).expect("clear");
        assert!(assigned(&conn, "i1").is_empty());
    }

    #[test]
    fn set_tags_unknown_tag_fails() {
        let conn = seeded();
        assign(&conn, "i1", "a", 100);

        let key = validation_key(set_investment_tags(
            &conn,
            "i1",
            &["d".to_string(), "ghost".to_string()],
        ));

        assert_eq!(key, "validation.tagNotFound");
        assert_eq!(assigned(&conn, "i1"), vec!["a"], "nothing written");
    }

    #[test]
    fn set_tags_unknown_investment_is_not_found() {
        let conn = seeded();
        let result = set_investment_tags(&conn, "ghost", &["d".to_string()]);
        assert!(matches!(result, Err(AppError::NotFound(_))), "{result:?}");
    }

    #[test]
    fn set_tags_allows_free_tags_to_overlap_and_ignores_duplicate_ids() {
        let conn = seeded();
        set_investment_tags(
            &conn,
            "i1",
            &[
                "d".to_string(),
                "f".to_string(),
                "d".to_string(),
                "a".to_string(),
                "c".to_string(),
            ],
        )
        .expect("set");
        assert_eq!(assigned(&conn, "i1"), vec!["a", "c", "d", "f"]);
    }

    // ---- update_tag ----

    #[test]
    fn moving_a_tag_into_a_group_with_a_conflict_fails() {
        let conn = seeded();
        assign(&conn, "i1", "a", 100);
        assign(&conn, "i1", "d", 100);

        let result = update_tag(&conn, "d", tag_input("Delta", Some("g1")));

        match result {
            Err(AppError::Validation(key)) => assert_eq!(key, "validation.tagGroupConflict"),
            other => panic!("expected a conflict, got {other:?}"),
        }
        assert_eq!(group_of(&conn, "d"), None, "group unchanged");
    }

    #[test]
    fn moving_a_tag_without_conflict_succeeds() {
        let conn = seeded();
        // i1 carries d, i2 carries a: no position would carry two tags of g1.
        assign(&conn, "i1", "d", 100);
        assign(&conn, "i2", "a", 100);

        let moved = update_tag(&conn, "d", tag_input("Delta", Some("g1"))).expect("move");

        assert_eq!(moved.group_id.as_deref(), Some("g1"));
        assert_eq!(group_of(&conn, "d").as_deref(), Some("g1"));
    }

    #[test]
    fn renaming_keeps_group_check_off() {
        let conn = seeded();
        // Legacy data with two tags of g1 on one position (the migration removes it in the
        // app); a rename inside the same group must not trip over it.
        assign(&conn, "i1", "a", 100);
        assign(&conn, "i1", "b", 100);

        let renamed = update_tag(&conn, "a", tag_input("  Alpha  ", Some("g1"))).expect("rename");

        assert_eq!(renamed.name, "Alpha", "stored trimmed");
        assert_eq!(renamed.group_id.as_deref(), Some("g1"));
    }

    #[test]
    fn moving_a_tag_out_of_its_group_never_conflicts() {
        let conn = seeded();
        assign(&conn, "i1", "a", 100);
        assign(&conn, "i1", "d", 100);

        let moved = update_tag(&conn, "a", tag_input("Alfa", None)).expect("ungroup");

        assert_eq!(moved.group_id, None);
    }

    #[test]
    fn update_unknown_tag_is_not_found() {
        let conn = seeded();
        let result = update_tag(&conn, "ghost", tag_input("Ghost", None));
        assert!(matches!(result, Err(AppError::NotFound(_))), "{result:?}");
    }

    // ---- CRUD moved from the commands ----

    #[test]
    fn groups_round_trip() {
        let conn = setup_test_db();
        let created = create_group(
            &conn,
            InsertStockTagGroup {
                name: "  Region ".to_string(),
                description: Some("where".to_string()),
            },
        )
        .expect("create");
        assert_eq!(created.name, "Region");

        let updated = update_group(
            &conn,
            &created.id,
            InsertStockTagGroup {
                name: "Geography".to_string(),
                description: None,
            },
        )
        .expect("update");
        assert_eq!(updated.name, "Geography");
        assert_eq!(updated.description, None);
        assert_eq!(updated.created_at, created.created_at);
        assert_eq!(list_groups(&conn).expect("list").len(), 1);

        delete_group(&conn, &created.id).expect("delete");
        assert!(list_groups(&conn).expect("list").is_empty());
        assert!(matches!(
            delete_group(&conn, &created.id),
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            update_group(
                &conn,
                "ghost",
                InsertStockTagGroup {
                    name: "x".to_string(),
                    description: None
                }
            ),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn tags_round_trip_and_investment_tags_are_listed_by_name() {
        let conn = seeded();
        let created = create_tag(&conn, tag_input(" Echo ", Some("g2"))).expect("create");
        assert_eq!(created.name, "Echo");
        assert_eq!(created.group_id.as_deref(), Some("g2"));
        assert_eq!(list_tags(&conn).expect("list").len(), 6);

        set_investment_tags(&conn, "i1", &[created.id.clone(), "d".to_string()]).expect("set");
        let names: Vec<String> = investment_tags(&conn, "i1")
            .expect("tags")
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(names, vec!["Delta", "Echo"]);

        delete_tag(&conn, &created.id).expect("delete");
        assert_eq!(
            assigned(&conn, "i1"),
            vec!["d"],
            "assignments go with the tag"
        );
        assert!(matches!(
            delete_tag(&conn, &created.id),
            Err(AppError::NotFound(_))
        ));
    }

    // ---- validate() ----

    #[test]
    fn insert_tag_validate() {
        assert!(tag_input("Growth", None).validate().is_ok());
        assert!(tag_input(&"x".repeat(60), None).validate().is_ok());

        for blank in ["", "   ", "\t\n"] {
            let key = validation_key(tag_input(blank, None).validate());
            assert_eq!(key, "validation.tagNameRequired", "{blank:?}");
        }
        let key = validation_key(tag_input(&"x".repeat(61), None).validate());
        assert_eq!(key, "validation.tagNameTooLong");
        // Characters, not bytes; surrounding whitespace does not count.
        assert!(tag_input(&"ž".repeat(60), None).validate().is_ok());
        assert!(tag_input(&format!("  {}  ", "x".repeat(60)), None)
            .validate()
            .is_ok());
    }

    #[test]
    fn insert_group_validate() {
        let group = |name: &str| InsertStockTagGroup {
            name: name.to_string(),
            description: None,
        };
        assert!(group("Strategy").validate().is_ok());
        assert_eq!(
            validation_key(group("  ").validate()),
            "validation.tagNameRequired"
        );
        assert_eq!(
            validation_key(group(&"x".repeat(61)).validate()),
            "validation.tagNameTooLong"
        );
    }
}

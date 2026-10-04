//! Persistence for the embedded MCP server's stable token and fixed port.
//! Columns live on the singleton user_profile row (id = 1), inside the
//! encrypted database — the token is never written to plaintext disk.

use crate::error::{AppError, Result};
use rusqlite::{Connection, OptionalExtension};

pub const DEFAULT_PORT: u16 = 41414;

/// Read the stable MCP bearer token (None until the user first enables MCP).
pub fn get_token(conn: &Connection) -> Result<Option<String>> {
    let row: Option<Option<String>> = conn
        .query_row(
            "SELECT mcp_server_token FROM user_profile WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    Ok(row.flatten())
}

/// Return the stored token, generating and persisting one if absent.
pub fn get_or_create_token(conn: &Connection) -> Result<String> {
    if let Some(token) = get_token(conn)? {
        return Ok(token);
    }
    write_new_token(conn)
}

/// Replace the token unconditionally, invalidating existing client configs.
pub fn regenerate_token(conn: &Connection) -> Result<String> {
    write_new_token(conn)
}

fn write_new_token(conn: &Connection) -> Result<String> {
    let token = uuid::Uuid::new_v4().to_string();
    let updated = conn.execute(
        "UPDATE user_profile SET mcp_server_token = ?1 WHERE id = 1",
        [&token],
    )?;
    if updated == 0 {
        return Err(AppError::NotFound("User profile not found".into()));
    }
    Ok(token)
}

/// Read the configured port, falling back to DEFAULT_PORT when unset or out of range.
pub fn get_port(conn: &Connection) -> Result<u16> {
    let row: Option<Option<i64>> = conn
        .query_row(
            "SELECT mcp_server_port FROM user_profile WHERE id = 1",
            [],
            |row| row.get(0),
        )
        .optional()?;
    Ok(match row.flatten() {
        Some(p) if (1024..=65535).contains(&p) => p as u16,
        _ => DEFAULT_PORT,
    })
}

/// Persist a new port. Ports below 1024 are privileged and rejected.
pub fn set_port(conn: &Connection, port: u16) -> Result<()> {
    if port < 1024 {
        return Err(AppError::Validation("validation.mcpPortRange".into()));
    }
    let updated = conn.execute(
        "UPDATE user_profile SET mcp_server_port = ?1 WHERE id = 1",
        [port as i64],
    )?;
    if updated == 0 {
        return Err(AppError::NotFound("User profile not found".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE user_profile (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                mcp_server_token TEXT,
                mcp_server_port INTEGER
            );
            INSERT INTO user_profile (id, name) VALUES (1, 'Test');
        "#,
        )
        .expect("schema");
        conn
    }

    #[test]
    fn get_token_is_none_before_first_enable() {
        let conn = setup_test_db();
        assert_eq!(get_token(&conn).unwrap(), None);
    }

    #[test]
    fn get_or_create_token_creates_once_and_persists() {
        let conn = setup_test_db();
        let t1 = get_or_create_token(&conn).unwrap();
        assert!(!t1.is_empty());
        let t2 = get_or_create_token(&conn).unwrap();
        assert_eq!(t1, t2, "second call must return the stored token");
        assert_eq!(get_token(&conn).unwrap(), Some(t1));
    }

    #[test]
    fn regenerate_token_replaces_and_persists() {
        let conn = setup_test_db();
        let t1 = get_or_create_token(&conn).unwrap();
        let t2 = regenerate_token(&conn).unwrap();
        assert_ne!(t1, t2);
        assert_eq!(get_token(&conn).unwrap(), Some(t2));
    }

    #[test]
    fn port_defaults_to_41414() {
        let conn = setup_test_db();
        assert_eq!(get_port(&conn).unwrap(), 41414);
    }

    #[test]
    fn set_port_persists() {
        let conn = setup_test_db();
        set_port(&conn, 55555).unwrap();
        assert_eq!(get_port(&conn).unwrap(), 55555);
    }

    #[test]
    fn set_port_rejects_below_1024() {
        let conn = setup_test_db();
        let err = set_port(&conn, 1023).unwrap_err();
        assert!(matches!(err, AppError::Validation(_)));
        assert_eq!(
            get_port(&conn).unwrap(),
            41414,
            "invalid port must not persist"
        );
    }

    #[test]
    fn set_port_accepts_boundaries() {
        let conn = setup_test_db();
        set_port(&conn, 1024).unwrap();
        assert_eq!(get_port(&conn).unwrap(), 1024);
        set_port(&conn, 65535).unwrap();
        assert_eq!(get_port(&conn).unwrap(), 65535);
    }

    #[test]
    fn get_port_falls_back_on_out_of_range_stored_value() {
        let conn = setup_test_db();
        conn.execute(
            "UPDATE user_profile SET mcp_server_port = 80 WHERE id = 1",
            [],
        )
        .unwrap();
        assert_eq!(get_port(&conn).unwrap(), DEFAULT_PORT);
        conn.execute(
            "UPDATE user_profile SET mcp_server_port = 700000 WHERE id = 1",
            [],
        )
        .unwrap();
        assert_eq!(get_port(&conn).unwrap(), DEFAULT_PORT);
    }

    #[test]
    fn token_ops_error_without_profile_row() {
        let conn = setup_test_db();
        conn.execute("DELETE FROM user_profile", []).unwrap();
        assert!(matches!(
            get_or_create_token(&conn).unwrap_err(),
            AppError::NotFound(_)
        ));
        assert!(matches!(
            regenerate_token(&conn).unwrap_err(),
            AppError::NotFound(_)
        ));
    }
}

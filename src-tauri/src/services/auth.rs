//! Authentication service
//!
//! Handles user setup, unlock, recovery, and session management.
//! Uses file-based key storage compatible with Moony-local.

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::user::{normalize_country, validate_main_currency, DEFAULT_MAIN_CURRENCY};
use crate::models::{InsertUserProfile, MenuPreferences, UpdateUserProfile, UserProfile};
use crate::services::crypto;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

/// Global session state
static IS_AUTHENTICATED: AtomicBool = AtomicBool::new(false);

/// Check if user is currently authenticated
pub fn is_authenticated() -> bool {
    IS_AUTHENTICATED.load(Ordering::SeqCst)
}

/// Set authentication state
fn set_authenticated(value: bool) {
    IS_AUTHENTICATED.store(value, Ordering::SeqCst);
}

/// Minimum password length, counted in characters (not bytes).
///
/// Keep in sync with `MIN_PASSWORD_LENGTH` in `shared/schema.ts`.
pub const MIN_PASSWORD_LENGTH: usize = 8;

/// Validate a new password against the policy.
///
/// The frontend enforces the same rule in its zod schemas, but the backend is
/// the only real enforcement point (MCP/API clients and buggy UIs bypass it).
pub fn validate_password(password: &str) -> Result<()> {
    if password.chars().count() < MIN_PASSWORD_LENGTH {
        return Err(AppError::Validation("validation.passwordMinLength".into()));
    }
    Ok(())
}

/// File names for key storage
const SALT_FILE: &str = "salt";
const PASSWORD_KEY_FILE: &str = "key.enc";
const RECOVERY_KEY_FILE: &str = "recovery.enc";

/// Get paths for all key files
fn get_key_paths(data_dir: &Path) -> (PathBuf, PathBuf, PathBuf) {
    (
        data_dir.join(SALT_FILE),
        data_dir.join(PASSWORD_KEY_FILE),
        data_dir.join(RECOVERY_KEY_FILE),
    )
}

/// Check if database and key files exist (user has set up account)
pub fn database_exists(db_path: &Path) -> bool {
    if !db_path.exists() {
        return false;
    }

    // Also check for key files
    if let Some(data_dir) = db_path.parent() {
        let (salt_path, key_path, recovery_path) = get_key_paths(data_dir);
        return salt_path.exists() && key_path.exists() && recovery_path.exists();
    }

    false
}

/// Setup a new user account
///
/// Creates encrypted database, stores user profile, returns recovery key.
/// Key architecture:
/// 1. Generate random master key (32 bytes)
/// 2. Generate random salt (32 bytes)
/// 3. Encrypt master key with password-derived key -> key.enc
/// 4. Encrypt master key with recovery-derived key -> recovery.enc
/// 5. Use master key to encrypt SQLCipher database
pub fn setup_account(
    db: &Database,
    db_path: PathBuf,
    data: InsertUserProfile,
    password: &str,
) -> Result<String> {
    validate_password(password)?;
    data.validate()?;

    let data_dir = db_path
        .parent()
        .ok_or_else(|| AppError::Internal("Invalid database path".into()))?;

    // Ensure data directory exists
    fs::create_dir_all(data_dir)
        .map_err(|e| AppError::Database(format!("Failed to create data directory: {}", e)))?;

    let (salt_path, key_path, recovery_path) = get_key_paths(data_dir);

    // Generate cryptographic keys
    let master_key = crypto::generate_master_key();
    let salt = crypto::generate_salt();
    let recovery_key = crypto::generate_recovery_key();

    // Store salt
    crypto::write_salt(&salt_path, &salt)?;

    // Store master key encrypted with password
    crypto::store_password_encrypted_key(&master_key, password, &salt, &key_path)?;

    // Store master key encrypted with recovery key
    crypto::store_recovery_encrypted_key(&master_key, &recovery_key, &salt, &recovery_path)?;

    // Create encrypted database using master key
    let master_key_hex = crypto::master_key_to_hex(&master_key);
    db.create_with_key(db_path.clone(), &master_key_hex)?;

    // Create user profile
    let menu_prefs = data
        .menu_preferences
        .unwrap_or_else(MenuPreferences::all_enabled);
    let menu_prefs_json = serde_json::to_string(&menu_prefs)?;
    let currency =
        validate_main_currency(data.currency.as_deref().unwrap_or(DEFAULT_MAIN_CURRENCY))?;
    let language = data.language.unwrap_or_else(|| "en".to_string());
    let exclude_re = data.exclude_personal_real_estate.unwrap_or(false);

    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO user_profile (name, surname, email, menu_preferences, currency, language, exclude_personal_real_estate)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                data.name,
                data.surname,
                data.email,
                menu_prefs_json,
                currency,
                language,
                exclude_re as i32,
            ],
        )?;
        Ok(())
    })?;

    set_authenticated(true);

    Ok(recovery_key)
}

/// Unlock existing database with password
pub fn unlock(db: &Database, db_path: PathBuf, password: &str) -> Result<UserProfile> {
    let data_dir = db_path
        .parent()
        .ok_or_else(|| AppError::Internal("Invalid database path".into()))?;

    let (salt_path, key_path, _) = get_key_paths(data_dir);

    // Older profiles and restored backups may carry 0o644 key files.
    crypto::restrict_key_file_permissions(data_dir);

    // Read salt
    let salt = crypto::read_salt(&salt_path)?;

    // Decrypt master key using password
    let master_key = crypto::decrypt_master_key_with_password(password, &salt, &key_path)?;

    // Open database with master key
    let master_key_hex = crypto::master_key_to_hex(&master_key);
    db.open_with_key(db_path, &master_key_hex)?;

    // Get user profile
    let profile =
        get_user_profile(db)?.ok_or_else(|| AppError::Auth("No user profile found".into()))?;

    set_authenticated(true);

    Ok(profile)
}

/// Prepared setup data - returned by prepare_setup, passed to confirm_setup
#[derive(Debug, Clone)]
pub struct PreparedSetup {
    pub master_key_hex: String,
    pub recovery_key: String,
    pub salt: Vec<u8>,
}

/// The keys generated for a first-run setup, held in Rust between the two phases so the master
/// key and the salt never reach the webview: only the recovery key goes out, for the user to write
/// down. Every prepare replaces the keys; a confirm that succeeds clears them.
#[derive(Default)]
pub struct PendingSetup(Mutex<Option<PreparedSetup>>);

impl PendingSetup {
    /// Generate and keep fresh keys; returns the recovery key to show the user.
    pub fn prepare(&self) -> Result<String> {
        let prepared = prepare_setup()?;
        let recovery_key = prepared.recovery_key.clone();
        *self.lock() = Some(prepared);
        Ok(recovery_key)
    }

    /// Run `confirm` with the kept keys. They are cleared once it succeeds and kept when it fails,
    /// so the user can fix the form and confirm again with the recovery key already written down.
    /// The lock is held throughout, so a prepare cannot swap the keys mid-confirm.
    pub fn confirm<T>(&self, confirm: impl FnOnce(&PreparedSetup) -> Result<T>) -> Result<T> {
        let mut slot = self.lock();
        let prepared = slot
            .as_ref()
            .ok_or_else(|| AppError::Auth("auth.setupNotPrepared".into()))?;
        let out = confirm(prepared)?;
        *slot = None;
        Ok(out)
    }

    fn lock(&self) -> MutexGuard<'_, Option<PreparedSetup>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// Phase 1: Prepare setup - generates keys but doesn't persist anything
/// Returns the recovery key so user can save it before we create the account
pub fn prepare_setup() -> Result<PreparedSetup> {
    let master_key = crypto::generate_master_key();
    let salt = crypto::generate_salt();
    let recovery_key = crypto::generate_recovery_key();
    let master_key_hex = crypto::master_key_to_hex(&master_key);

    Ok(PreparedSetup {
        master_key_hex,
        recovery_key,
        salt: salt.to_vec(),
    })
}

/// Phase 2: Confirm setup - persists all keys and creates the account
/// Only called after user confirms they saved the recovery key
pub fn confirm_setup(
    db: &Database,
    db_path: PathBuf,
    data: InsertUserProfile,
    password: &str,
    prepared: &PreparedSetup,
) -> Result<UserProfile> {
    validate_password(password)?;
    data.validate()?;

    let data_dir = db_path
        .parent()
        .ok_or_else(|| AppError::Internal("Invalid database path".into()))?;

    // Ensure data directory exists
    fs::create_dir_all(data_dir)
        .map_err(|e| AppError::Database(format!("Failed to create data directory: {}", e)))?;

    let (salt_path, key_path, recovery_path) = get_key_paths(data_dir);

    // Convert hex back to master key bytes
    let master_key = crypto::hex_to_master_key(&prepared.master_key_hex)?;

    // Convert salt vec to array
    if prepared.salt.len() != 32 {
        return Err(AppError::Encryption("Invalid salt length".into()));
    }
    let mut salt = [0u8; 32];
    salt.copy_from_slice(&prepared.salt);

    // Store salt
    crypto::write_salt(&salt_path, &salt)?;

    // Store master key encrypted with password
    crypto::store_password_encrypted_key(&master_key, password, &salt, &key_path)?;

    // Store master key encrypted with recovery key
    crypto::store_recovery_encrypted_key(
        &master_key,
        &prepared.recovery_key,
        &salt,
        &recovery_path,
    )?;

    // Create encrypted database using master key
    db.create_with_key(db_path.clone(), &prepared.master_key_hex)?;

    // Create user profile
    let menu_prefs = data
        .menu_preferences
        .unwrap_or_else(MenuPreferences::all_enabled);
    let menu_prefs_json = serde_json::to_string(&menu_prefs)?;
    let currency =
        validate_main_currency(data.currency.as_deref().unwrap_or(DEFAULT_MAIN_CURRENCY))?;
    let language = data.language.unwrap_or_else(|| "en".to_string());
    let exclude_re = data.exclude_personal_real_estate.unwrap_or(false);

    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO user_profile (name, surname, email, menu_preferences, currency, language, exclude_personal_real_estate)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                data.name,
                data.surname,
                data.email,
                menu_prefs_json,
                currency,
                language,
                exclude_re as i32,
            ],
        )?;
        Ok(())
    })?;

    set_authenticated(true);

    // Return the created profile
    get_user_profile(db)?
        .ok_or_else(|| AppError::Internal("Failed to get profile after setup".into()))
}

// ============================================================================
// 2-Phase Change Password Flow
// ============================================================================

/// Phase 1: Prepare password change - verifies current password and generates new recovery key
/// Returns the new recovery key so user can save it before we make changes
pub fn prepare_change_password(db_path: &Path, current_password: &str) -> Result<String> {
    let data_dir = db_path
        .parent()
        .ok_or_else(|| AppError::Internal("Invalid database path".into()))?;

    let (salt_path, key_path, _) = get_key_paths(data_dir);
    let salt = crypto::read_salt(&salt_path)?;

    // Verify current password by attempting to decrypt master key. Typed errors
    // (`auth.wrongPassword` / `auth.keyFilesCorrupted`) propagate unchanged.
    let _master_key = crypto::decrypt_master_key_with_password(current_password, &salt, &key_path)?;

    // Generate new recovery key (don't persist yet, just return it)
    let new_recovery_key = crypto::generate_recovery_key();
    Ok(new_recovery_key)
}

/// Phase 2: Confirm password change - actually persists the new password and recovery key
/// Only called after user confirms they saved the new recovery key
pub fn confirm_change_password(
    db_path: &Path,
    current_password: &str,
    new_password: &str,
    new_recovery_key: &str,
) -> Result<()> {
    validate_password(new_password)?;

    let data_dir = db_path
        .parent()
        .ok_or_else(|| AppError::Internal("Invalid database path".into()))?;

    let (salt_path, key_path, recovery_path) = get_key_paths(data_dir);
    let salt = crypto::read_salt(&salt_path)?;

    // Decrypt master key with current password
    let master_key = crypto::decrypt_master_key_with_password(current_password, &salt, &key_path)?;

    // Re-encrypt master key with new password
    crypto::store_password_encrypted_key(&master_key, new_password, &salt, &key_path)?;

    // Re-encrypt master key with new recovery key
    crypto::store_recovery_encrypted_key(&master_key, new_recovery_key, &salt, &recovery_path)?;

    Ok(())
}

// ============================================================================
// 2-Phase Recovery Flow
// ============================================================================

/// Phase 1: Prepare recovery - verifies recovery key and generates new one
/// Returns new recovery key but doesn't persist any changes
///
/// `new_password` is validated here already so the user is not asked to save a
/// new recovery key for a recovery that `confirm_recover` would reject anyway.
pub fn prepare_recover(db_path: &Path, recovery_key: &str, new_password: &str) -> Result<String> {
    validate_password(new_password)?;

    let data_dir = db_path
        .parent()
        .ok_or_else(|| AppError::Internal("Invalid database path".into()))?;

    let (salt_path, _, recovery_path) = get_key_paths(data_dir);
    let salt = crypto::read_salt(&salt_path)?;

    // Verify recovery key by attempting to decrypt master key. The key is
    // normalized by crypto (case, spaces, dashes); typed errors
    // (`auth.invalidRecoveryKey` / `auth.keyFilesCorrupted`) propagate unchanged.
    let _master_key =
        crypto::decrypt_master_key_with_recovery(recovery_key, &salt, &recovery_path)?;

    // Generate new recovery key (don't persist yet, just return it)
    let new_recovery_key = crypto::generate_recovery_key();
    Ok(new_recovery_key)
}

/// Phase 2: Confirm recovery - actually persists new password and recovery key
/// Only called after user confirms they saved the new recovery key
pub fn confirm_recover(
    db: &Database,
    db_path: PathBuf,
    old_recovery_key: &str,
    new_password: &str,
    new_recovery_key: &str,
) -> Result<UserProfile> {
    validate_password(new_password)?;

    let data_dir = db_path
        .parent()
        .ok_or_else(|| AppError::Internal("Invalid database path".into()))?;

    let (salt_path, key_path, recovery_path) = get_key_paths(data_dir);
    let salt = crypto::read_salt(&salt_path)?;

    // Decrypt master key using old recovery key (normalized by crypto)
    let master_key =
        crypto::decrypt_master_key_with_recovery(old_recovery_key, &salt, &recovery_path)?;

    // Re-encrypt master key with new password
    crypto::store_password_encrypted_key(&master_key, new_password, &salt, &key_path)?;

    // Re-encrypt master key with new recovery key
    crypto::store_recovery_encrypted_key(&master_key, new_recovery_key, &salt, &recovery_path)?;

    // Open database with master key
    let master_key_hex = crypto::master_key_to_hex(&master_key);
    db.open_with_key(db_path, &master_key_hex)?;

    // Get user profile
    let profile =
        get_user_profile(db)?.ok_or_else(|| AppError::Auth("No user profile found".into()))?;

    set_authenticated(true);

    Ok(profile)
}

/// Lock the app (close database)
pub fn logout(db: &Database) {
    db.close();
    set_authenticated(false);
}

/// Get user profile from database
pub fn get_user_profile(db: &Database) -> Result<Option<UserProfile>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, surname, email, menu_preferences, currency,
                    language, exclude_personal_real_estate, coingecko_modal_dismissed,
                    mcp_server_enabled, created_at, country, insurance_notice_days
             FROM user_profile LIMIT 1",
        )?;

        let result = stmt.query_row([], |row| {
            let menu_prefs_json: String = row.get(4)?;
            let menu_prefs: MenuPreferences =
                serde_json::from_str(&menu_prefs_json).unwrap_or_default();
            let country: Option<String> = row.get(11)?;
            let stored_notice: Option<i64> = row.get(12)?;

            Ok(UserProfile {
                id: row.get(0)?,
                name: row.get(1)?,
                surname: row.get(2)?,
                email: row.get(3)?,
                menu_preferences: menu_prefs,
                currency: row.get(5)?,
                language: row.get(6)?,
                exclude_personal_real_estate: row.get::<_, i32>(7)? != 0,
                coingecko_modal_dismissed: row.get::<_, i32>(8)? != 0,
                mcp_server_enabled: row.get::<_, i32>(9)? != 0,
                created_at: row.get(10)?,
                insurance_notice_days: crate::services::milestones::effective_insurance_notice_days(
                    stored_notice,
                ),
                country,
            })
        });

        match result {
            Ok(profile) => Ok(Some(profile)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    })
}

/// Update user profile
pub fn update_user_profile(db: &Database, updates: UpdateUserProfile) -> Result<UserProfile> {
    db.with_conn(|conn| apply_profile_update(conn, &updates))?;

    get_user_profile(db)?.ok_or_else(|| AppError::NotFound("User profile not found".into()))
}

/// Validate and write a partial profile update (the main currency is stored normalised).
fn apply_profile_update(conn: &rusqlite::Connection, updates: &UpdateUserProfile) -> Result<()> {
    updates.validate()?;
    let mut set_clauses = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(ref name) = updates.name {
        set_clauses.push("name = ?".to_string());
        params.push(Box::new(name.clone()));
    }

    if let Some(ref surname) = updates.surname {
        set_clauses.push("surname = ?".to_string());
        params.push(Box::new(surname.clone()));
    }

    if let Some(ref email) = updates.email {
        set_clauses.push("email = ?".to_string());
        params.push(Box::new(email.clone()));
    }

    if let Some(ref prefs) = updates.menu_preferences {
        set_clauses.push("menu_preferences = ?".to_string());
        params.push(Box::new(serde_json::to_string(prefs)?));
    }

    if let Some(ref currency) = updates.currency {
        set_clauses.push("currency = ?".to_string());
        params.push(Box::new(validate_main_currency(currency)?));
    }

    if let Some(ref language) = updates.language {
        set_clauses.push("language = ?".to_string());
        params.push(Box::new(language.clone()));
    }

    if let Some(exclude) = updates.exclude_personal_real_estate {
        set_clauses.push("exclude_personal_real_estate = ?".to_string());
        params.push(Box::new(exclude as i32));
    }

    if let Some(dismissed) = updates.coingecko_modal_dismissed {
        set_clauses.push("coingecko_modal_dismissed = ?".to_string());
        params.push(Box::new(dismissed as i32));
    }

    if let Some(enabled) = updates.mcp_server_enabled {
        set_clauses.push("mcp_server_enabled = ?".to_string());
        params.push(Box::new(enabled as i32));
    }

    if let Some(ref country) = updates.country {
        set_clauses.push("country = ?".to_string());
        params.push(Box::new(normalize_country(country)?));
    }

    if let Some(days) = updates.insurance_notice_days {
        set_clauses.push("insurance_notice_days = ?".to_string());
        params.push(Box::new(days));
    }

    if set_clauses.is_empty() {
        return Ok(());
    }

    let sql = format!(
        "UPDATE user_profile SET {} WHERE id = 1",
        set_clauses.join(", ")
    );

    let params_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    conn.execute(&sql, params_refs.as_slice())?;

    Ok(())
}

/// Delete entire account (database and key files)
pub fn delete_account(db: &Database, db_path: &Path) -> Result<()> {
    set_authenticated(false);
    db.close();

    // Delete database file
    if db_path.exists() {
        fs::remove_file(db_path)
            .map_err(|e| AppError::Database(format!("Failed to delete database: {}", e)))?;
    }

    // Delete key files and resource directories
    if let Some(data_dir) = db_path.parent() {
        let (salt_path, key_path, recovery_path) = get_key_paths(data_dir);

        // Delete key files and resource directories, reporting what could not
        // be removed instead of silently leaving it behind.
        let mut failures: Vec<String> = Vec::new();
        for path in [salt_path, key_path, recovery_path] {
            if path.exists() {
                if let Err(e) = fs::remove_file(&path) {
                    failures.push(format!("{}: {e}", path.display()));
                }
            }
        }

        let resource_dirs = [
            data_dir.join("real_estate_photos"),
            data_dir.join("real_estate_documents"),
            data_dir.join("insurance_documents"),
        ];
        for dir in resource_dirs {
            if dir.exists() {
                if let Err(e) = fs::remove_dir_all(&dir) {
                    failures.push(format!("{}: {e}", dir.display()));
                }
            }
        }

        if !failures.is_empty() {
            return Err(AppError::Database(format!(
                "The account was deleted but some files could not be removed: {}",
                failures.join("; ")
            )));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_database_exists_returns_false_when_no_files() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("moony.db");
        assert!(!database_exists(&db_path));
    }

    #[test]
    fn test_database_exists_returns_false_when_only_db_file_present() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("moony.db");
        fs::write(&db_path, b"fake db").unwrap();
        assert!(!database_exists(&db_path));
    }

    #[test]
    fn test_database_exists_returns_false_when_only_some_key_files_present() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("moony.db");
        fs::write(&db_path, b"fake db").unwrap();
        fs::write(dir.path().join("salt"), b"fake salt").unwrap();
        // Missing key.enc and recovery.enc
        assert!(!database_exists(&db_path));
    }

    #[test]
    fn test_database_exists_returns_true_when_all_files_present() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("moony.db");
        fs::write(&db_path, b"fake db").unwrap();
        fs::write(dir.path().join("salt"), b"fake salt").unwrap();
        fs::write(dir.path().join("key.enc"), b"fake key").unwrap();
        fs::write(dir.path().join("recovery.enc"), b"fake recovery").unwrap();
        assert!(database_exists(&db_path));
    }

    #[test]
    fn test_is_authenticated_can_be_reset() {
        use std::sync::atomic::Ordering;
        IS_AUTHENTICATED.store(false, Ordering::SeqCst);
        assert!(!is_authenticated());
    }

    // ------------------------------------------------------------------
    // Password policy enforced in the backend
    // ------------------------------------------------------------------

    const RECOVERY_KEY: &str = "ABCD-EFGH-JKLM-NPQR-STUV-WXYZ";

    fn assert_password_too_short(err: AppError) {
        match err {
            AppError::Validation(msg) => assert_eq!(msg, "validation.passwordMinLength"),
            other => panic!("expected password length validation error, got {other:?}"),
        }
    }

    fn assert_auth_key(err: AppError, expected: &str) {
        match err {
            AppError::Auth(msg) => assert_eq!(msg, expected),
            other => panic!("expected AppError::Auth({expected:?}), got {other:?}"),
        }
    }

    /// Creates real key files (salt, key.enc, recovery.enc) and returns the db path.
    fn write_key_files(dir: &Path, password: &str) -> PathBuf {
        let master = crypto::generate_master_key();
        let salt = crypto::generate_salt();
        crypto::write_salt(&dir.join("salt"), &salt).unwrap();
        crypto::store_password_encrypted_key(&master, password, &salt, &dir.join("key.enc"))
            .unwrap();
        crypto::store_recovery_encrypted_key(
            &master,
            RECOVERY_KEY,
            &salt,
            &dir.join("recovery.enc"),
        )
        .unwrap();
        dir.join("moony.db")
    }

    fn profile_data() -> InsertUserProfile {
        InsertUserProfile {
            name: "Test".into(),
            surname: "User".into(),
            email: String::new(),
            menu_preferences: None,
            currency: None,
            language: None,
            exclude_personal_real_estate: None,
        }
    }

    fn dir_is_empty(dir: &Path) -> bool {
        fs::read_dir(dir).unwrap().next().is_none()
    }

    #[test]
    fn min_password_length_is_eight() {
        assert_eq!(MIN_PASSWORD_LENGTH, 8);
    }

    #[test]
    fn validate_password_rejects_empty_and_short_passwords() {
        assert_password_too_short(validate_password("").unwrap_err());
        assert_password_too_short(validate_password("1234567").unwrap_err());
    }

    #[test]
    fn validate_password_accepts_minimum_length() {
        assert!(validate_password("12345678").is_ok());
        assert!(validate_password("a much longer passphrase").is_ok());
    }

    #[test]
    fn validate_password_counts_characters_not_bytes() {
        // 7 characters but 14 bytes in UTF-8: must still be rejected.
        assert_eq!("ěščřžýá".len(), 14);
        assert_password_too_short(validate_password("ěščřžýá").unwrap_err());
        // 8 characters: accepted.
        assert!(validate_password("ěščřžýáí").is_ok());
    }

    #[test]
    fn setup_account_rejects_short_password_before_writing_anything() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("moony.db");
        let err = setup_account(&Database::new(), db_path, profile_data(), "short").unwrap_err();
        assert_password_too_short(err);
        assert!(dir_is_empty(dir.path()), "no key files may be written");
    }

    #[test]
    fn confirm_setup_rejects_empty_password_before_writing_anything() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("moony.db");
        let prepared = prepare_setup().unwrap();
        let err =
            confirm_setup(&Database::new(), db_path, profile_data(), "", &prepared).unwrap_err();
        assert_password_too_short(err);
        assert!(dir_is_empty(dir.path()), "no key files may be written");
    }

    #[test]
    fn pending_setup_hands_out_only_the_recovery_key_and_confirms_with_the_kept_keys() {
        let pending = PendingSetup::default();
        let recovery_key = pending.prepare().unwrap();
        pending
            .confirm(|prepared| {
                assert_eq!(prepared.recovery_key, recovery_key);
                assert_eq!(prepared.salt.len(), 32);
                crypto::hex_to_master_key(&prepared.master_key_hex).map(|_| ())
            })
            .unwrap();
    }

    #[test]
    fn pending_setup_cannot_confirm_before_prepare() {
        let err = PendingSetup::default().confirm(|_| Ok(())).unwrap_err();
        assert!(matches!(err, AppError::Auth(ref key) if key == "auth.setupNotPrepared"));
    }

    #[test]
    fn pending_setup_is_cleared_by_a_successful_confirm() {
        let pending = PendingSetup::default();
        pending.prepare().unwrap();
        pending.confirm(|_| Ok(())).unwrap();
        assert!(pending.confirm(|_| Ok(())).is_err());
    }

    #[test]
    fn pending_setup_keeps_the_keys_when_confirm_fails() {
        let pending = PendingSetup::default();
        let recovery_key = pending.prepare().unwrap();
        let err = pending
            .confirm(|_| -> Result<()> {
                Err(AppError::Validation("validation.passwordMinLength".into()))
            })
            .unwrap_err();
        assert!(matches!(err, AppError::Validation(_)));
        let kept = pending.confirm(|p| Ok(p.recovery_key.clone())).unwrap();
        assert_eq!(kept, recovery_key);
    }

    #[test]
    fn pending_setup_is_replaced_by_every_prepare() {
        let pending = PendingSetup::default();
        let first = pending.prepare().unwrap();
        let second = pending.prepare().unwrap();
        assert_ne!(first, second);
        let kept = pending.confirm(|p| Ok(p.recovery_key.clone())).unwrap();
        assert_eq!(kept, second);
    }

    #[test]
    fn confirm_recover_rejects_short_new_password_and_leaves_key_files_untouched() {
        let dir = tempdir().expect("tempdir");
        let db_path = write_key_files(dir.path(), "old-password");
        let before = fs::read(dir.path().join("key.enc")).unwrap();
        let err = confirm_recover(
            &Database::new(),
            db_path,
            RECOVERY_KEY,
            "",
            "WXYZ-STUV-NPQR-JKLM-EFGH-ABCD",
        )
        .unwrap_err();
        assert_password_too_short(err);
        assert_eq!(fs::read(dir.path().join("key.enc")).unwrap(), before);
    }

    #[test]
    fn prepare_recover_rejects_short_new_password_before_issuing_a_new_key() {
        let dir = tempdir().expect("tempdir");
        let db_path = write_key_files(dir.path(), "old-password");
        let err = prepare_recover(&db_path, RECOVERY_KEY, "1234567").unwrap_err();
        assert_password_too_short(err);
    }

    #[test]
    fn confirm_change_password_rejects_short_new_password_and_leaves_key_files_untouched() {
        let dir = tempdir().expect("tempdir");
        let db_path = write_key_files(dir.path(), "old-password");
        let key_before = fs::read(dir.path().join("key.enc")).unwrap();
        let recovery_before = fs::read(dir.path().join("recovery.enc")).unwrap();
        let err = confirm_change_password(
            &db_path,
            "old-password",
            "",
            "WXYZ-STUV-NPQR-JKLM-EFGH-ABCD",
        )
        .unwrap_err();
        assert_password_too_short(err);
        assert_eq!(fs::read(dir.path().join("key.enc")).unwrap(), key_before);
        assert_eq!(
            fs::read(dir.path().join("recovery.enc")).unwrap(),
            recovery_before
        );
    }

    #[test]
    fn confirm_change_password_accepts_minimum_length_and_switches_password() {
        let dir = tempdir().expect("tempdir");
        let db_path = write_key_files(dir.path(), "old-password");
        let new_key = "WXYZ-STUV-NPQR-JKLM-EFGH-ABCD";
        confirm_change_password(&db_path, "old-password", "12345678", new_key).unwrap();

        let salt = crypto::read_salt(&dir.path().join("salt")).unwrap();
        // New password works, old one no longer does.
        assert!(crypto::decrypt_master_key_with_password(
            "12345678",
            &salt,
            &dir.path().join("key.enc")
        )
        .is_ok());
        assert_auth_key(
            crypto::decrypt_master_key_with_password(
                "old-password",
                &salt,
                &dir.path().join("key.enc"),
            )
            .unwrap_err(),
            "auth.wrongPassword",
        );
        // New recovery key works (also when typed lowercase with spaces), old one does not.
        assert!(crypto::decrypt_master_key_with_recovery(
            "wxyz stuv npqr jklm efgh abcd",
            &salt,
            &dir.path().join("recovery.enc")
        )
        .is_ok());
        assert_auth_key(
            crypto::decrypt_master_key_with_recovery(
                RECOVERY_KEY,
                &salt,
                &dir.path().join("recovery.enc"),
            )
            .unwrap_err(),
            "auth.invalidRecoveryKey",
        );
    }

    /// a profile whose key.enc predates the KDF header (60 bytes, N=2^14)
    /// still unlocks, and a password change re-wraps it with the current cost.
    #[test]
    fn change_password_upgrades_a_legacy_key_file_to_the_current_kdf_cost() {
        let dir = tempdir().expect("tempdir");
        let db_path = write_key_files(dir.path(), "old-password");
        let salt = crypto::read_salt(&dir.path().join("salt")).unwrap();
        let key_path = dir.path().join("key.enc");

        // Rebuild key.enc in the pre-header format (60 bytes, scrypt N = 2^14).
        let master =
            crypto::decrypt_master_key_with_password("old-password", &salt, &key_path).unwrap();
        let legacy_key = crypto::derive_key("old-password", &salt).unwrap();
        let legacy_file = crypto::encrypt_with_key(&master, &legacy_key).unwrap();
        fs::write(&key_path, &legacy_file).unwrap();
        assert_eq!(fs::read(&key_path).unwrap().len(), 60);

        // It still unlocks...
        prepare_change_password(&db_path, "old-password").unwrap();
        // ...and changing the password re-wraps it with the headered format.
        confirm_change_password(
            &db_path,
            "old-password",
            "new-password",
            "WXYZ-STUV-NPQR-JKLM-EFGH-ABCD",
        )
        .unwrap();
        assert_eq!(fs::read(&key_path).unwrap().len(), 68);
        let again =
            crypto::decrypt_master_key_with_password("new-password", &salt, &key_path).unwrap();
        assert_eq!(again, master);
    }

    // ------------------------------------------------------------------
    // Typed auth errors and recovery key normalization
    // ------------------------------------------------------------------

    #[test]
    fn prepare_change_password_wrong_current_password_returns_stable_key() {
        let dir = tempdir().expect("tempdir");
        let db_path = write_key_files(dir.path(), "old-password");
        let err = prepare_change_password(&db_path, "nope-nope").unwrap_err();
        assert_auth_key(err, "auth.wrongPassword");
    }

    #[test]
    fn confirm_change_password_wrong_current_password_returns_stable_key() {
        let dir = tempdir().expect("tempdir");
        let db_path = write_key_files(dir.path(), "old-password");
        let err = confirm_change_password(
            &db_path,
            "nope-nope",
            "brand-new-password",
            "WXYZ-STUV-NPQR-JKLM-EFGH-ABCD",
        )
        .unwrap_err();
        assert_auth_key(err, "auth.wrongPassword");
    }

    #[test]
    fn prepare_recover_wrong_key_returns_invalid_recovery_key() {
        let dir = tempdir().expect("tempdir");
        let db_path = write_key_files(dir.path(), "old-password");
        let err =
            prepare_recover(&db_path, "AAAA-AAAA-AAAA-AAAA-AAAA-AAAA", "new-password").unwrap_err();
        assert_auth_key(err, "auth.invalidRecoveryKey");
    }

    #[test]
    fn prepare_recover_accepts_lowercase_key_with_spaces() {
        let dir = tempdir().expect("tempdir");
        let db_path = write_key_files(dir.path(), "old-password");
        let new_key =
            prepare_recover(&db_path, "abcd efgh jklm npqr stuv wxyz", "new-password").unwrap();
        // A fresh canonical key was issued (24 chars + 5 dashes).
        assert_eq!(new_key.len(), 29);
    }

    #[test]
    fn confirm_recover_wrong_key_returns_invalid_recovery_key() {
        let dir = tempdir().expect("tempdir");
        let db_path = write_key_files(dir.path(), "old-password");
        let err = confirm_recover(
            &Database::new(),
            db_path,
            "AAAA-AAAA-AAAA-AAAA-AAAA-AAAA",
            "new-password",
            "WXYZ-STUV-NPQR-JKLM-EFGH-ABCD",
        )
        .unwrap_err();
        assert_auth_key(err, "auth.invalidRecoveryKey");
    }

    #[test]
    fn unlock_with_missing_key_files_reports_corrupted_key_files() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("moony.db");
        let err = unlock(&Database::new(), db_path, "whatever-password").unwrap_err();
        assert_auth_key(err, "auth.keyFilesCorrupted");
    }

    #[test]
    fn unlock_with_wrong_password_reports_wrong_password() {
        let dir = tempdir().expect("tempdir");
        let db_path = write_key_files(dir.path(), "old-password");
        let err = unlock(&Database::new(), db_path, "not-the-password").unwrap_err();
        assert_auth_key(err, "auth.wrongPassword");
    }

    /// profiles created before the fix carry 0o644 key files; unlocking
    /// tightens them (even when the password turns out to be wrong).
    #[cfg(unix)]
    #[test]
    fn unlock_tightens_permissive_key_file_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempdir().expect("tempdir");
        let db_path = write_key_files(dir.path(), "old-password");
        for name in crypto::SECRET_FILE_NAMES {
            fs::set_permissions(dir.path().join(name), fs::Permissions::from_mode(0o644)).unwrap();
        }

        let _ = unlock(&Database::new(), db_path, "not-the-password");

        for name in crypto::SECRET_FILE_NAMES {
            let mode = fs::metadata(dir.path().join(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "{name}");
        }
    }

    // ------------------------------------------------------------------
    // Main currency must have an exchange-rate source
    // ------------------------------------------------------------------

    fn profile_conn() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE user_profile (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL DEFAULT '',
                surname TEXT NOT NULL DEFAULT '',
                email TEXT NOT NULL DEFAULT '',
                menu_preferences TEXT,
                currency TEXT NOT NULL DEFAULT 'CZK',
                exclude_personal_real_estate INTEGER NOT NULL DEFAULT 0,
                language TEXT NOT NULL DEFAULT 'en',
                coingecko_modal_dismissed INTEGER NOT NULL DEFAULT 0,
                mcp_server_enabled INTEGER NOT NULL DEFAULT 0,
                country TEXT,
                insurance_notice_days INTEGER
            );
            INSERT INTO user_profile (id, currency) VALUES (1, 'CZK');
            "#,
        )
        .expect("schema");
        conn
    }

    /// An update that changes nothing; tests set the one field they care about.
    fn empty_update() -> UpdateUserProfile {
        UpdateUserProfile {
            name: None,
            surname: None,
            email: None,
            menu_preferences: None,
            currency: None,
            language: None,
            exclude_personal_real_estate: None,
            coingecko_modal_dismissed: None,
            mcp_server_enabled: None,
            country: None,
            insurance_notice_days: None,
        }
    }

    fn currency_update(code: &str) -> UpdateUserProfile {
        UpdateUserProfile {
            currency: Some(code.to_string()),
            ..empty_update()
        }
    }

    fn stored_country(conn: &rusqlite::Connection) -> Option<String> {
        conn.query_row("SELECT country FROM user_profile WHERE id = 1", [], |r| {
            r.get(0)
        })
        .expect("country")
    }

    fn stored_currency(conn: &rusqlite::Connection) -> String {
        conn.query_row("SELECT currency FROM user_profile WHERE id = 1", [], |r| {
            r.get(0)
        })
        .expect("currency")
    }

    #[test]
    fn update_rejects_main_currency_without_rate_source() {
        let conn = profile_conn();
        let err = apply_profile_update(&conn, &currency_update("KWD")).unwrap_err();
        match err {
            AppError::Validation(msg) => assert_eq!(msg, "validation.currencyUnsupported"),
            other => panic!("expected validation error, got {other:?}"),
        }
        assert_eq!(stored_currency(&conn), "CZK");
    }

    #[test]
    fn update_stores_supported_main_currency_normalised() {
        let conn = profile_conn();
        apply_profile_update(&conn, &currency_update("aed")).unwrap();
        assert_eq!(stored_currency(&conn), "AED");
    }

    #[test]
    fn the_insurance_notice_is_stored_and_a_zero_is_kept() {
        let conn = profile_conn();
        let set = |days: i64| UpdateUserProfile {
            insurance_notice_days: Some(days),
            ..empty_update()
        };
        let stored = |conn: &rusqlite::Connection| -> Option<i64> {
            conn.query_row(
                "SELECT insurance_notice_days FROM user_profile WHERE id = 1",
                [],
                |r| r.get(0),
            )
            .unwrap()
        };
        apply_profile_update(&conn, &set(60)).unwrap();
        assert_eq!(stored(&conn), Some(60));
        apply_profile_update(&conn, &set(0)).unwrap();
        assert_eq!(
            stored(&conn),
            Some(0),
            "0 means no reminder and is stored as such"
        );
        assert!(apply_profile_update(&conn, &set(400)).is_err());
        assert_eq!(stored(&conn), Some(0));
    }

    #[test]
    fn the_country_is_stored_upper_case_and_cleared_by_an_empty_string() {
        let conn = profile_conn();
        let set = |code: &str| UpdateUserProfile {
            country: Some(code.to_string()),
            ..empty_update()
        };
        apply_profile_update(&conn, &set("cz")).unwrap();
        assert_eq!(stored_country(&conn).as_deref(), Some("CZ"));
        apply_profile_update(&conn, &set("")).unwrap();
        assert_eq!(stored_country(&conn), None);
        let err = apply_profile_update(&conn, &set("Narnia")).unwrap_err();
        assert!(matches!(err, AppError::Validation(key) if key == "validation.countryInvalid"));
        // A failed update changed nothing.
        assert_eq!(stored_country(&conn), None);
    }

    /// The wizard and Settings write the country through `update_user_profile` and the app reads it
    /// back through `get_user_profile`: on a real (SQLCipher, fully migrated) database, so the
    /// hand-written SELECT and its column index are exercised too.
    #[test]
    fn the_saved_country_comes_back_with_the_profile_and_clearing_it_gives_null() {
        let dir = tempdir().expect("tempdir");
        let db = Database::new();
        let key = crypto::master_key_to_hex(&[7u8; 32]);
        db.create_with_key(dir.path().join("moony.db"), &key)
            .expect("create db");
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO user_profile (name, surname, email) VALUES ('A', 'B', 'a@b.c')",
                [],
            )?;
            Ok(())
        })
        .expect("profile row");
        let country_of =
            |updates: UpdateUserProfile| update_user_profile(&db, updates).expect("update").country;
        let set = |code: &str| UpdateUserProfile {
            country: Some(code.to_string()),
            ..empty_update()
        };

        assert_eq!(get_user_profile(&db).unwrap().unwrap().country, None);
        assert_eq!(country_of(set("gb")).as_deref(), Some("GB"));
        assert_eq!(
            get_user_profile(&db).unwrap().unwrap().country.as_deref(),
            Some("GB")
        );
        // An update that does not mention the country keeps it.
        assert_eq!(
            country_of(UpdateUserProfile {
                name: Some("C".into()),
                ..empty_update()
            })
            .as_deref(),
            Some("GB")
        );
        assert_eq!(country_of(set("")), None);
        assert_eq!(get_user_profile(&db).unwrap().unwrap().country, None);
    }

    /// The profile reports the notice the milestones use: the country default until the user sets
    /// one, then the stored value, `0` meaning no reminder (a real, fully migrated database).
    #[test]
    fn the_profile_reports_the_effective_insurance_notice() {
        let dir = tempdir().expect("tempdir");
        let db = Database::new();
        let key = crypto::master_key_to_hex(&[7u8; 32]);
        db.create_with_key(dir.path().join("moony.db"), &key)
            .expect("create db");
        db.with_conn(|conn| {
            conn.execute(
                "INSERT INTO user_profile (name, surname, email) VALUES ('A', 'B', 'a@b.c')",
                [],
            )?;
            Ok(())
        })
        .expect("profile row");
        let notice = || {
            get_user_profile(&db)
                .unwrap()
                .unwrap()
                .insurance_notice_days
        };
        let update =
            |updates: UpdateUserProfile| update_user_profile(&db, updates).expect("update");
        let country = |code: &str| UpdateUserProfile {
            country: Some(code.to_string()),
            ..empty_update()
        };
        let days = |days: i64| UpdateUserProfile {
            insurance_notice_days: Some(days),
            ..empty_update()
        };

        // Every profile defaults to 50 days, whatever its country.
        assert_eq!(notice(), Some(50));
        assert_eq!(update(country("cz")).insurance_notice_days, Some(50));
        assert_eq!(update(country("us")).insurance_notice_days, Some(50));
        // A stored value wins; 0 switches the reminder off.
        assert_eq!(update(days(30)).insurance_notice_days, Some(30));
        assert_eq!(update(country("cz")).insurance_notice_days, Some(30));
        assert_eq!(update(days(0)).insurance_notice_days, None);
        // An update that does not mention the notice keeps it.
        let renamed = UpdateUserProfile {
            name: Some("C".into()),
            ..empty_update()
        };
        assert_eq!(update(renamed).insurance_notice_days, None);
        assert_eq!(notice(), None);
    }

    #[test]
    fn setup_account_rejects_unsupported_main_currency_before_writing_anything() {
        let dir = tempdir().expect("tempdir");
        let db_path = dir.path().join("moony.db");
        let mut data = profile_data();
        data.currency = Some("KWD".into());
        let err =
            setup_account(&Database::new(), db_path, data, "long-enough-password").unwrap_err();
        assert!(matches!(err, AppError::Validation(_)));
        assert!(dir_is_empty(dir.path()), "no key files may be written");
    }
}

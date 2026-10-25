//! Authentication service
//!
//! Handles user setup, unlock, recovery, and session management.
//! Uses file-based key storage compatible with Moony-local.

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::user::{normalize_country, validate_main_currency, DEFAULT_MAIN_CURRENCY};
use crate::models::{InsertUserProfile, MenuPreferences, UpdateUserProfile, UserProfile};
use crate::services::biometric_unlock;
use crate::services::crypto;
use crate::services::session_key::SessionKey;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use zeroize::Zeroizing;

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
pub fn unlock(
    db: &Database,
    session: &SessionKey,
    db_path: PathBuf,
    password: &str,
) -> Result<UserProfile> {
    unlock_with(&db_path, password, |master_key| {
        unlock_with_master_key(db, session, db_path.clone(), master_key)
    })
}

/// The part of every unlock after the master key is known (password and biometrics alike): open
/// the database with it, read the profile, keep the key for the session on the phone (biometric
/// unlock is switched on with it) and mark the session authenticated.
pub fn unlock_with_master_key(
    db: &Database,
    session: &SessionKey,
    db_path: PathBuf,
    master_key: &[u8; 32],
) -> Result<UserProfile> {
    let master_key_hex = Zeroizing::new(crypto::master_key_to_hex(master_key));
    db.open_with_key(db_path, &master_key_hex)?;
    let profile =
        get_user_profile(db)?.ok_or_else(|| AppError::Auth("No user profile found".into()))?;
    keep_session_key(session, master_key);
    set_authenticated(true);
    Ok(profile)
}

/// Keep the master key in the session after an unlock or the first-run setup.
fn keep_session_key(session: &SessionKey, master_key: &[u8; 32]) {
    // R22: only the phone needs the key after unlock (biometric enrolment).
    #[cfg(mobile)]
    session.set(master_key);
    #[cfg(not(mobile))]
    let _ = (session, master_key);
}

/// The key-file side of [`unlock`]: unwrap the master key with the password, hand it to `open`
/// (which opens the database), and only once that succeeds move a weaker `key.enc` to the current
/// cost.
fn unlock_with<T>(
    db_path: &Path,
    password: &str,
    open: impl FnOnce(&[u8; 32]) -> Result<T>,
) -> Result<T> {
    let data_dir = db_path
        .parent()
        .ok_or_else(|| AppError::Internal("Invalid database path".into()))?;

    let (salt_path, key_path, _) = get_key_paths(data_dir);

    // Older profiles and restored backups may carry 0o644 key files.
    crypto::restrict_key_file_permissions(data_dir);

    // Read salt
    let salt = crypto::read_salt(&salt_path)?;

    // Decrypt master key using password
    let master_key = Zeroizing::new(crypto::decrypt_master_key_with_password(
        password, &salt, &key_path,
    )?);

    let opened = open(&master_key)?;

    // A key.enc from before ADR 0009 (N=2^14) moves to the current cost now, once.
    upgrade_key_file_cost(&master_key, password, &salt, &key_path);

    Ok(opened)
}

/// Move `key.enc` to the current scrypt cost after a successful unlock (ADR 0018). Best effort:
/// a failure is logged without paths or secrets and the unlock goes on, because the old file
/// still opens the profile.
fn upgrade_key_file_cost(master_key: &[u8; 32], password: &str, salt: &[u8], key_path: &Path) {
    match crypto::upgrade_password_key_file(master_key, password, salt, key_path) {
        Ok(true) => log::info!("[AUTH] Re-wrapped key.enc with the current key derivation cost"),
        Ok(false) => {}
        Err(e) => {
            log::warn!("[AUTH] Could not re-wrap key.enc with the current key derivation cost: {e}")
        }
    }
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
    session: &SessionKey,
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
    let master_key = Zeroizing::new(crypto::hex_to_master_key(&prepared.master_key_hex)?);

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

    keep_session_key(session, &master_key);
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

    // The new recovery key first: each file is replaced atomically, the pair is not. Stopping
    // between the two writes then leaves the old password and the new recovery key — the one the
    // person just saved — rather than a saved key that opens nothing.
    crypto::store_recovery_encrypted_key(&master_key, new_recovery_key, &salt, &recovery_path)?;

    // Re-encrypt master key with new password
    crypto::store_password_encrypted_key(&master_key, new_password, &salt, &key_path)?;

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
    session: &SessionKey,
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
    let master_key = Zeroizing::new(crypto::decrypt_master_key_with_recovery(
        old_recovery_key,
        &salt,
        &recovery_path,
    )?);

    // The new recovery key first, as in a password change: stopping between the two writes
    // leaves the new recovery key the person just saved working.
    crypto::store_recovery_encrypted_key(&master_key, new_recovery_key, &salt, &recovery_path)?;

    // Re-encrypt master key with new password
    crypto::store_password_encrypted_key(&master_key, new_password, &salt, &key_path)?;

    // Open the database with the master key, as every unlock does
    unlock_with_master_key(db, session, db_path, &master_key)
}

/// Lock the app (close database) and forget the session's master key
pub fn logout(db: &Database, session: &SessionKey) {
    db.close();
    session.clear();
    set_authenticated(false);
}

/// Get user profile from database
pub fn get_user_profile(db: &Database) -> Result<Option<UserProfile>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, surname, email, menu_preferences, currency,
                    language, exclude_personal_real_estate,
                    mcp_server_enabled, created_at, country, insurance_notice_days, theme
             FROM user_profile LIMIT 1",
        )?;

        let result = stmt.query_row([], |row| {
            let menu_prefs_json: String = row.get("menu_preferences")?;
            let menu_prefs: MenuPreferences =
                serde_json::from_str(&menu_prefs_json).unwrap_or_default();
            let country: Option<String> = row.get("country")?;
            let stored_notice: Option<i64> = row.get("insurance_notice_days")?;
            let theme: String = row.get("theme")?;

            Ok(UserProfile {
                id: row.get("id")?,
                name: row.get("name")?,
                surname: row.get("surname")?,
                email: row.get("email")?,
                menu_preferences: menu_prefs,
                currency: row.get("currency")?,
                language: row.get("language")?,
                exclude_personal_real_estate: row.get::<_, i32>("exclude_personal_real_estate")?
                    != 0,
                mcp_server_enabled: row.get::<_, i32>("mcp_server_enabled")? != 0,
                created_at: row.get("created_at")?,
                insurance_notice_days: crate::services::milestones::effective_insurance_notice_days(
                    stored_notice,
                ),
                country,
                theme,
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

    if let Some(ref theme) = updates.theme {
        set_clauses.push("theme = ?".to_string());
        params.push(Box::new(theme.clone()));
    }

    if let Some(exclude) = updates.exclude_personal_real_estate {
        set_clauses.push("exclude_personal_real_estate = ?".to_string());
        params.push(Box::new(exclude as i32));
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

/// Delete entire account: the database, the attachments store, the key files and every other file
/// of the account in its data folder
pub fn delete_account(db: &Database, session: &SessionKey, db_path: &Path) -> Result<()> {
    set_authenticated(false);
    db.close();
    session.clear();

    // Delete database file
    if db_path.exists() {
        fs::remove_file(db_path)
            .map_err(|e| AppError::Database(format!("Failed to delete database: {}", e)))?;
    }

    if let Some(data_dir) = db_path.parent() {
        // Report what could not be removed instead of silently leaving it behind.
        let failures = remove_account_files(data_dir);
        if !failures.is_empty() {
            return Err(AppError::Database(format!(
                "The account was deleted but some files could not be removed: {}",
                failures.join("; ")
            )));
        }
    }

    Ok(())
}

/// Remove the files of an account from its data folder: both databases with their side files and
/// pre-migration copies, the key files, the plain attachment folders of earlier versions and the
/// conversion's quarantine. Anything else in the folder is left alone. Returns what could not be
/// removed.
pub fn remove_account_files(data_dir: &Path) -> Vec<String> {
    let mut failures: Vec<String> = Vec::new();
    let mut remove = |path: PathBuf| {
        let result = if path.is_dir() {
            fs::remove_dir_all(&path)
        } else if path.exists() {
            fs::remove_file(&path)
        } else {
            return;
        };
        if let Err(e) = result {
            failures.push(format!("{}: {e}", path.display()));
        }
    };

    let (salt_path, key_path, recovery_path) = get_key_paths(data_dir);
    // The key files, the staging copies an interrupted key-file write leaves behind, and the
    // phone's biometric unlock marker.
    let staging = [
        crypto::staging_path(&key_path),
        crypto::staging_path(&recovery_path),
    ];
    for path in [salt_path, key_path, recovery_path]
        .into_iter()
        .chain(staging)
        .chain([data_dir.join(biometric_unlock::MARKER_FILE)])
    {
        remove(path);
    }
    let databases = [
        crate::services::backup::DB_FILE_NAME,
        crate::db::files_schema::FILE_NAME,
    ];
    if let Ok(entries) = fs::read_dir(data_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            // The database files, their -wal/-shm/-journal side files and `.bak-<ts>` copies.
            if databases.iter().any(|db| name.starts_with(db)) {
                remove(entry.path());
            }
        }
    }
    for dir in crate::services::backup::ATTACHMENT_DIRS
        .into_iter()
        .chain([crate::services::attachment_conversion::QUARANTINE_DIR])
    {
        remove(data_dir.join(dir));
    }
    failures
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
        let err = confirm_setup(
            &Database::new(),
            &SessionKey::default(),
            db_path,
            profile_data(),
            "",
            &prepared,
        )
        .unwrap_err();
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
            &SessionKey::default(),
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

    /// A password change that stops between its two key-file writes (a crash, a dead battery, the
    /// system killing the app) must leave the recovery key the person was just shown working:
    /// the new recovery key is written first, so a failed password write leaves the old password
    /// and the new recovery key, never a saved key that opens nothing.
    #[test]
    fn change_password_writes_the_new_recovery_key_before_the_password() {
        let dir = tempdir().expect("tempdir");
        let db_path = write_key_files(dir.path(), "old-password");
        let key_path = dir.path().join("key.enc");
        let recovery_path = dir.path().join("recovery.enc");
        // The password write fails: a folder takes its staging path.
        fs::create_dir(crypto::staging_path(&key_path)).unwrap();
        let new_key = "WXYZ-STUV-NPQR-JKLM-EFGH-ABCD";

        assert!(
            confirm_change_password(&db_path, "old-password", "new-password", new_key).is_err()
        );

        let salt = crypto::read_salt(&dir.path().join("salt")).unwrap();
        assert!(crypto::decrypt_master_key_with_recovery(new_key, &salt, &recovery_path).is_ok());
        assert!(crypto::decrypt_master_key_with_password("old-password", &salt, &key_path).is_ok());
    }

    /// The same order for a recovery: when the password write fails, the new recovery key the
    /// person was just shown opens the profile (the old password, forgotten, is still the one in
    /// `key.enc`).
    #[test]
    fn recover_writes_the_new_recovery_key_before_the_password() {
        let dir = tempdir().expect("tempdir");
        let db_path = write_key_files(dir.path(), "old-password");
        let key_path = dir.path().join("key.enc");
        let recovery_path = dir.path().join("recovery.enc");
        fs::create_dir(crypto::staging_path(&key_path)).unwrap();
        let new_key = "WXYZ-STUV-NPQR-JKLM-EFGH-ABCD";

        assert!(confirm_recover(
            &Database::new(),
            &SessionKey::default(),
            db_path,
            RECOVERY_KEY,
            "new-password",
            new_key,
        )
        .is_err());

        let salt = crypto::read_salt(&dir.path().join("salt")).unwrap();
        assert!(crypto::decrypt_master_key_with_recovery(new_key, &salt, &recovery_path).is_ok());
        assert!(crypto::decrypt_master_key_with_password("old-password", &salt, &key_path).is_ok());
    }

    /// The phone's password change while biometric unlock is on (ruling 14): the change re-wraps
    /// the same master key, so the copy biometric unlock stored in the keychain still opens the
    /// profile and nothing needs re-enrolling. The new password opens it, the old one is refused,
    /// the old recovery key no longer opens it and the new one does; the marker is left alone and
    /// nothing is written beside the key files.
    #[test]
    fn change_password_keeps_the_master_key_biometric_unlock_holds() {
        let dir = tempdir().expect("tempdir");
        let db_path = write_key_files(dir.path(), "old-password");
        let salt = crypto::read_salt(&dir.path().join("salt")).unwrap();
        let key_path = dir.path().join("key.enc");
        let recovery_path = dir.path().join("recovery.enc");
        // What biometric unlock stored when it was switched on: the session's master key.
        let stored =
            crypto::decrypt_master_key_with_password("old-password", &salt, &key_path).unwrap();
        biometric_unlock::write_marker(dir.path(), true).unwrap();

        let new_recovery_key = prepare_change_password(&db_path, "old-password").unwrap();
        confirm_change_password(&db_path, "old-password", "new-password", &new_recovery_key)
            .unwrap();

        // The new password unwraps the very key biometric unlock holds.
        assert_eq!(
            crypto::decrypt_master_key_with_password("new-password", &salt, &key_path).unwrap(),
            stored
        );
        unlock_with(&db_path, "new-password", open_with(stored)).unwrap();
        // The old password is refused, before and after the key step alike.
        assert_auth_key(
            prepare_change_password(&db_path, "old-password").unwrap_err(),
            "auth.wrongPassword",
        );
        assert_auth_key(
            unlock_with(&db_path, "old-password", open_with(stored)).unwrap_err(),
            "auth.wrongPassword",
        );
        // The old recovery key no longer opens the profile; the new one opens the same key.
        assert_auth_key(
            crypto::decrypt_master_key_with_recovery(RECOVERY_KEY, &salt, &recovery_path)
                .unwrap_err(),
            "auth.invalidRecoveryKey",
        );
        assert_eq!(
            crypto::decrypt_master_key_with_recovery(&new_recovery_key, &salt, &recovery_path)
                .unwrap(),
            stored
        );
        // Biometric unlock stays on, and the change wrote nothing but the two key files.
        assert_eq!(
            biometric_unlock::read_marker(dir.path()),
            biometric_unlock::Marker::Enabled
        );
        let mut files: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        files.sort();
        assert_eq!(
            files,
            [
                biometric_unlock::MARKER_FILE,
                "key.enc",
                "recovery.enc",
                "salt"
            ]
        );
    }

    /// a profile whose key.enc predates the KDF header (60 bytes, N=2^14)
    /// still unlocks, and a password change re-wraps it with the current cost.
    #[test]
    fn change_password_upgrades_a_legacy_key_file_to_the_current_kdf_cost() {
        let dir = tempdir().expect("tempdir");
        let (salt, key_path, master) = write_legacy_key_files(dir.path(), "old-password");
        let db_path = dir.path().join("moony.db");

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

    /// Rebuilds `key.enc` in the pre-header format (60 bytes, scrypt N = 2^14) and returns the
    /// salt, the key path and the master key.
    fn write_legacy_key_files(dir: &Path, password: &str) -> ([u8; 32], PathBuf, [u8; 32]) {
        write_key_files(dir, password);
        let salt = crypto::read_salt(&dir.join("salt")).unwrap();
        let key_path = dir.join("key.enc");
        let master = crypto::decrypt_master_key_with_password(password, &salt, &key_path).unwrap();
        let legacy_key = crypto::derive_key(password, &salt).unwrap();
        fs::write(
            &key_path,
            crypto::encrypt_with_key(&master, &legacy_key).unwrap(),
        )
        .unwrap();
        assert_eq!(fs::read(&key_path).unwrap().len(), 60);
        (salt, key_path, master)
    }

    /// The database step of `unlock`, minus SQLCipher: checks it got the right key.
    fn open_with(master: [u8; 32]) -> impl FnOnce(&[u8; 32]) -> Result<()> {
        move |key| {
            assert_eq!(key, &master);
            Ok(())
        }
    }

    /// ADR 0018: a successful unlock re-wraps a legacy key.enc, the same password opens it
    /// again, and the next unlock leaves it alone.
    #[test]
    fn unlocking_upgrades_a_legacy_key_file_and_the_same_password_still_opens_it() {
        let dir = tempdir().expect("tempdir");
        let (_, key_path, master) = write_legacy_key_files(dir.path(), "old-password");
        let db_path = dir.path().join("moony.db");

        unlock_with(&db_path, "old-password", open_with(master)).unwrap();

        let upgraded = fs::read(&key_path).unwrap();
        assert_eq!(upgraded.len(), 68);
        unlock_with(&db_path, "old-password", open_with(master)).unwrap();
        assert_eq!(
            fs::read(&key_path).unwrap(),
            upgraded,
            "not rewritten again"
        );
        // recovery.enc is left as ADR 0009 decided: headerless, legacy cost.
        assert_eq!(fs::read(dir.path().join("recovery.enc")).unwrap().len(), 60);
    }

    /// A failing re-wrap must not fail the unlock; the old file stays and still opens.
    #[test]
    fn a_failing_key_file_upgrade_does_not_fail_unlock_and_keeps_the_old_file() {
        let dir = tempdir().expect("tempdir");
        let (_, key_path, master) = write_legacy_key_files(dir.path(), "old-password");
        let db_path = dir.path().join("moony.db");
        let before = fs::read(&key_path).unwrap();
        fs::create_dir(dir.path().join("key.enc.tmp")).unwrap();

        unlock_with(&db_path, "old-password", open_with(master)).unwrap();

        assert_eq!(fs::read(&key_path).unwrap(), before);
        unlock_with(&db_path, "old-password", open_with(master)).unwrap();
    }

    /// Nothing is written unless the whole unlock succeeds.
    #[test]
    fn an_unlock_that_fails_to_open_the_database_leaves_a_legacy_key_file_alone() {
        let dir = tempdir().expect("tempdir");
        let (_, key_path, _) = write_legacy_key_files(dir.path(), "old-password");
        let db_path = dir.path().join("moony.db");
        let before = fs::read(&key_path).unwrap();

        let err = unlock_with(&db_path, "old-password", |_| -> Result<()> {
            Err(AppError::Database("file is not a database".into()))
        })
        .unwrap_err();

        assert!(matches!(err, AppError::Database(_)));
        assert_eq!(fs::read(&key_path).unwrap(), before);
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
            &SessionKey::default(),
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
        let err = unlock(
            &Database::new(),
            &SessionKey::default(),
            db_path,
            "whatever-password",
        )
        .unwrap_err();
        assert_auth_key(err, "auth.keyFilesCorrupted");
    }

    #[test]
    fn unlock_with_wrong_password_reports_wrong_password() {
        let dir = tempdir().expect("tempdir");
        let db_path = write_key_files(dir.path(), "old-password");
        let session = SessionKey::default();
        let err = unlock(&Database::new(), &session, db_path, "not-the-password").unwrap_err();
        assert_auth_key(err, "auth.wrongPassword");
        assert!(session.hex().is_none(), "a failed unlock keeps no key");
    }

    /// R22: the phone keeps the key after an unlock (biometric enrolment needs it), the desktop
    /// keeps none.
    #[test]
    fn the_session_key_is_kept_on_the_phone_only() {
        let session = SessionKey::default();
        keep_session_key(&session, &[9u8; 32]);
        assert_eq!(session.hex().is_some(), cfg!(mobile));
    }

    /// The session key goes with the session (biometric unlock can only be switched on while
    /// unlocked).
    #[test]
    fn logout_forgets_the_session_key() {
        let session = SessionKey::default();
        session.set(&[9u8; 32]);
        logout(&Database::new(), &session);
        assert!(session.hex().is_none());
    }

    #[test]
    fn deleting_the_account_forgets_the_session_key() {
        let dir = tempdir().expect("tempdir");
        let session = SessionKey::default();
        session.set(&[9u8; 32]);
        delete_account(&Database::new(), &session, &dir.path().join("moony.db")).expect("delete");
        assert!(session.hex().is_none());
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

        let _ = unlock(
            &Database::new(),
            &SessionKey::default(),
            db_path,
            "not-the-password",
        );

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
                mcp_server_enabled INTEGER NOT NULL DEFAULT 0,
                country TEXT,
                insurance_notice_days INTEGER,
                theme TEXT NOT NULL DEFAULT 'system'
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
            mcp_server_enabled: None,
            country: None,
            insurance_notice_days: None,
            theme: None,
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
    fn theme_is_written_and_read_back() {
        let conn = profile_conn();
        let stored = |conn: &rusqlite::Connection| -> String {
            conn.query_row("SELECT theme FROM user_profile WHERE id = 1", [], |r| {
                r.get(0)
            })
            .expect("theme")
        };
        let set = |theme: &str| UpdateUserProfile {
            theme: Some(theme.to_string()),
            ..empty_update()
        };

        apply_profile_update(&conn, &set("dark")).unwrap();
        assert_eq!(stored(&conn), "dark");
        // An update that does not mention the theme keeps it.
        apply_profile_update(
            &conn,
            &UpdateUserProfile {
                name: Some("C".into()),
                ..empty_update()
            },
        )
        .unwrap();
        assert_eq!(stored(&conn), "dark");
        // A value outside the three is refused and the stored one stays.
        match apply_profile_update(&conn, &set("blue")).unwrap_err() {
            AppError::Validation(key) => assert_eq!(key, "validation.themeInvalid"),
            other => panic!("expected validation error, got {other:?}"),
        }
        assert_eq!(stored(&conn), "dark");
    }

    /// A profile that never chose a theme follows the system, and the chosen one comes back through
    /// the profile read (a real, fully migrated database).
    #[test]
    fn a_profile_without_a_theme_reads_system() {
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

        assert_eq!(get_user_profile(&db).unwrap().unwrap().theme, "system");
        let chosen = update_user_profile(
            &db,
            UpdateUserProfile {
                theme: Some("light".into()),
                ..empty_update()
            },
        )
        .expect("update");
        assert_eq!(chosen.theme, "light");
        assert_eq!(get_user_profile(&db).unwrap().unwrap().theme, "light");
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

    #[test]
    fn removing_an_account_takes_every_account_file_and_nothing_else() {
        let dir = tempdir().expect("tempdir");
        let d = dir.path();
        for name in [
            "moony.db",
            "moony.db-journal",
            "moony.db.bak-1791652876",
            "attachments.db",
            "attachments.db-journal",
            "attachments.db.bak-1792064756",
            "salt",
            "key.enc",
            "recovery.enc",
            // Staging files an interrupted key-file write leaves behind (ADR 0018).
            "key.enc.tmp",
            "recovery.enc.tmp",
            // The phone's biometric unlock marker.
            "biometric-unlock",
            "unrelated.txt",
        ] {
            fs::write(d.join(name), b"x").expect("file");
        }
        for dir in [
            "real_estate_photos/re/b",
            "insurance_documents/pol",
            "attachments-converted/real_estate_documents/re",
            "Backup/insurance_documents",
        ] {
            fs::create_dir_all(d.join(dir)).expect("dir");
            fs::write(d.join(dir).join("f.pdf"), b"x").expect("file");
        }

        assert!(remove_account_files(d).is_empty());

        let mut left: Vec<String> = fs::read_dir(d)
            .expect("dir")
            .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(left, vec!["Backup", "unrelated.txt"]);
    }
}

//! Cryptographic utilities for SQLCipher encryption
//!
//! Architecture (matching Moony-local):
//! - Master key: Random 32-byte key used for SQLCipher encryption
//! - Master key is encrypted by BOTH password AND recovery key
//! - When password/recovery key is used, it decrypts the master key
//! - Master key then opens the encrypted database
//!
//! File format for key.enc and recovery.enc:
//! IV (12 bytes) + AuthTag (16 bytes) + Ciphertext (32 bytes) = 60 bytes total
//!
//! `key.enc` carries the scrypt cost it was derived with in
//! an 8-byte header in front of those 60 bytes (see [`KdfParams`]); files without
//! the header (every profile created earlier, and every `recovery.enc`) use
//! [`KdfParams::LEGACY`]. See ADR 0009. A weaker `key.enc` is re-wrapped at
//! [`KdfParams::CURRENT`] after a successful unlock, and key files are always
//! replaced atomically. See ADR 0018.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use rand::Rng;
use scrypt::{scrypt, Params};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use crate::error::{AppError, Result};

const KEY_LENGTH: usize = 32; // 256 bits
const SALT_LENGTH: usize = 32;
const IV_LENGTH: usize = 12; // GCM recommended nonce length
const AUTH_TAG_LENGTH: usize = 16;
/// Size of the encrypted master key blob (the whole of a legacy key file, the
/// part after the header in a new one): IV + AuthTag + encrypted 32-byte master key
const ENCRYPTED_KEY_LENGTH: usize = IV_LENGTH + AUTH_TAG_LENGTH + KEY_LENGTH;
/// Magic + format version + log2(N) + r + p
const KDF_HEADER_LENGTH: usize = 8;
const KDF_HEADER_MAGIC: [u8; 4] = *b"MNYK";
const KDF_HEADER_VERSION: u8 = 1;

/// Stable, key-like auth error messages. They cross IPC inside
/// `AppError::Auth(..)` and the frontend maps them to translated text via
/// `translateApiError` (see `common.json` -> `auth.*`). They must never carry
/// file paths or other implementation details.
pub const AUTH_WRONG_PASSWORD: &str = "auth.wrongPassword";
pub const AUTH_INVALID_RECOVERY_KEY: &str = "auth.invalidRecoveryKey";
pub const AUTH_KEY_FILES_CORRUPTED: &str = "auth.keyFilesCorrupted";

/// Normalize a user-entered recovery key to its canonical secret form.
///
/// Recovery keys are shown as `XXXX-XXXX-XXXX-XXXX-XXXX-XXXX` (alphabet
/// `ABCDEFGHJKLMNPQRSTUVWXYZ23456789`), but people retype them in lowercase,
/// with spaces, or paste them through apps that swap `-` for a typographic
/// dash. Uppercases and strips all whitespace and dash characters. Every place
/// that derives a key from a recovery key (storing and decrypting) must go
/// through this function so both sides always agree.
pub fn normalize_recovery_key(input: &str) -> String {
    input
        .chars()
        .filter(|c| {
            !c.is_whitespace()
                && !matches!(
                    c,
                    '-' | '\u{2010}'
                        | '\u{2011}'
                        | '\u{2012}'
                        | '\u{2013}'
                        | '\u{2014}'
                        | '\u{2212}'
                )
        })
        .flat_map(char::to_uppercase)
        .collect()
}

/// scrypt cost parameters (the output length is always 32 bytes).
///
/// Values read from a key file are range-checked so a damaged or hostile file
/// cannot make the app allocate gigabytes or spin for minutes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KdfParams {
    pub log_n: u8,
    pub r: u8,
    pub p: u8,
}

impl KdfParams {
    /// N=2^14, r=8, p=1: the Node.js `crypto.scrypt` defaults used by every
    /// profile created before the header existed, and by every `recovery.enc`
    /// (a 120-bit random recovery key does not need a slow KDF).
    pub const LEGACY: KdfParams = KdfParams {
        log_n: 14,
        r: 8,
        p: 1,
    };
    /// N=2^17 (128 MiB), r=8, p=1: the OWASP minimum for scrypt, measured at
    /// ~170 ms on an M4 Pro (ADR 0009). Used for new and re-wrapped `key.enc`.
    pub const CURRENT: KdfParams = KdfParams {
        log_n: 17,
        r: 8,
        p: 1,
    };

    const MIN_LOG_N: u8 = 14;
    /// 2^18 * r=16 * 128 B = 512 MiB, the most a key file may ask for.
    const MAX_LOG_N: u8 = 18;
    const MAX_R: u8 = 16;
    const MAX_P: u8 = 16;

    fn to_header(self) -> [u8; KDF_HEADER_LENGTH] {
        let m = KDF_HEADER_MAGIC;
        [
            m[0],
            m[1],
            m[2],
            m[3],
            KDF_HEADER_VERSION,
            self.log_n,
            self.r,
            self.p,
        ]
    }

    /// `None` for a wrong magic/version or out-of-range parameters.
    fn from_header(header: &[u8]) -> Option<KdfParams> {
        let header: &[u8; KDF_HEADER_LENGTH] = header.try_into().ok()?;
        if header[..4] != KDF_HEADER_MAGIC || header[4] != KDF_HEADER_VERSION {
            return None;
        }
        let params = KdfParams {
            log_n: header[5],
            r: header[6],
            p: header[7],
        };
        let in_range = (Self::MIN_LOG_N..=Self::MAX_LOG_N).contains(&params.log_n)
            && (1..=Self::MAX_R).contains(&params.r)
            && (1..=Self::MAX_P).contains(&params.p);
        in_range.then_some(params)
    }

    /// Memory one derivation needs, in units of 128 bytes (scrypt uses 128 * r * N bytes).
    fn memory_cost(self) -> u64 {
        u64::from(self.r) << self.log_n
    }

    /// Work one derivation takes, in the same units (p sequential passes over that memory).
    fn work_cost(self) -> u64 {
        self.memory_cost() * u64::from(self.p)
    }

    /// Whether one password guess against `other` needs at least as much memory and work as
    /// against these parameters, and more of one. A `key.enc` weaker than
    /// [`KdfParams::CURRENT`] is re-wrapped after unlock; one that is stronger, or stronger on
    /// either axis, is left alone so re-wrapping never cuts an attacker's cost (ADR 0018).
    pub fn is_weaker_than(self, other: KdfParams) -> bool {
        let (memory, work) = (self.memory_cost(), self.work_cost());
        let (other_memory, other_work) = (other.memory_cost(), other.work_cost());
        memory <= other_memory && work <= other_work && (memory, work) != (other_memory, other_work)
    }
}

// Compile-time guards for the parameter constants.
const _: () = assert!(KdfParams::CURRENT.log_n > KdfParams::LEGACY.log_n);
const _: () = assert!(KdfParams::CURRENT.log_n <= KdfParams::MAX_LOG_N);
const _: () = assert!(KdfParams::LEGACY.log_n >= KdfParams::MIN_LOG_N);

/// Split a key file into the KDF parameters it was written with and the
/// 60-byte encrypted master key. `None` means damaged (wrong size or header).
fn split_key_file(data: &[u8]) -> Option<(KdfParams, &[u8])> {
    if data.len() == ENCRYPTED_KEY_LENGTH {
        return Some((KdfParams::LEGACY, data));
    }
    if data.len() == KDF_HEADER_LENGTH + ENCRYPTED_KEY_LENGTH {
        let (header, payload) = data.split_at(KDF_HEADER_LENGTH);
        return Some((KdfParams::from_header(header)?, payload));
    }
    None
}

/// Derive encryption key from password/recovery key using scrypt with the
/// [`KdfParams::LEGACY`] cost.
/// Matches Node.js: scrypt(password, salt, 32)
pub fn derive_key(secret: &str, salt: &[u8]) -> Result<[u8; KEY_LENGTH]> {
    derive_key_with(secret, salt, KdfParams::LEGACY)
}

/// Derive encryption key from password/recovery key using scrypt with explicit
/// cost parameters. Output length is taken from the buffer passed to scrypt() (32 bytes).
pub fn derive_key_with(secret: &str, salt: &[u8], cost: KdfParams) -> Result<[u8; KEY_LENGTH]> {
    let params = Params::new(cost.log_n, u32::from(cost.r), u32::from(cost.p))
        .map_err(|e| AppError::Encryption(format!("Invalid scrypt params: {}", e)))?;

    let mut key = [0u8; KEY_LENGTH];
    scrypt(secret.as_bytes(), salt, &params, &mut key)
        .map_err(|e| AppError::Encryption(format!("scrypt failed: {}", e)))?;

    Ok(key)
}

/// Generate random master key for SQLCipher (32 bytes)
pub fn generate_master_key() -> [u8; KEY_LENGTH] {
    let mut key = [0u8; KEY_LENGTH];
    rand::rng().fill_bytes(&mut key);
    key
}

/// Generate random salt (32 bytes)
pub fn generate_salt() -> [u8; SALT_LENGTH] {
    let mut salt = [0u8; SALT_LENGTH];
    rand::rng().fill_bytes(&mut salt);
    salt
}

/// Encrypt data with key using AES-256-GCM
/// Returns: IV (12 bytes) + AuthTag (16 bytes) + Ciphertext
/// This matches Node.js format: createCipheriv with getAuthTag()
pub fn encrypt_with_key(data: &[u8], key: &[u8; KEY_LENGTH]) -> Result<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| AppError::Encryption(format!("Failed to create cipher: {}", e)))?;

    // Generate random IV
    let mut iv = [0u8; IV_LENGTH];
    rand::rng().fill_bytes(&mut iv);
    let nonce = Nonce::from(iv);

    // Encrypt (ciphertext includes auth tag appended by aes-gcm)
    let ciphertext_with_tag = cipher
        .encrypt(&nonce, data)
        .map_err(|e| AppError::Encryption(format!("Encryption failed: {}", e)))?;

    // aes-gcm appends auth tag at the end, we need to reformat to match Node.js:
    // Node.js format: IV + AuthTag + Ciphertext
    // aes-gcm format: Ciphertext + AuthTag
    let ciphertext_len = ciphertext_with_tag.len() - AUTH_TAG_LENGTH;
    let ciphertext = &ciphertext_with_tag[..ciphertext_len];
    let auth_tag = &ciphertext_with_tag[ciphertext_len..];

    // Combine: IV + AuthTag + Ciphertext
    let mut result = Vec::with_capacity(IV_LENGTH + AUTH_TAG_LENGTH + ciphertext_len);
    result.extend_from_slice(&iv);
    result.extend_from_slice(auth_tag);
    result.extend_from_slice(ciphertext);

    Ok(result)
}

/// Decrypt data with key using AES-256-GCM
/// Input format: IV (12 bytes) + AuthTag (16 bytes) + Ciphertext
pub fn decrypt_with_key(encrypted_data: &[u8], key: &[u8; KEY_LENGTH]) -> Result<Vec<u8>> {
    if encrypted_data.len() < IV_LENGTH + AUTH_TAG_LENGTH {
        return Err(AppError::Encryption("Encrypted data too short".into()));
    }

    let iv = &encrypted_data[..IV_LENGTH];
    let auth_tag = &encrypted_data[IV_LENGTH..IV_LENGTH + AUTH_TAG_LENGTH];
    let ciphertext = &encrypted_data[IV_LENGTH + AUTH_TAG_LENGTH..];

    // Reconstruct aes-gcm format: Ciphertext + AuthTag
    let mut ciphertext_with_tag = Vec::with_capacity(ciphertext.len() + AUTH_TAG_LENGTH);
    ciphertext_with_tag.extend_from_slice(ciphertext);
    ciphertext_with_tag.extend_from_slice(auth_tag);

    let cipher = Aes256Gcm::new_from_slice(key)
        .map_err(|e| AppError::Encryption(format!("Failed to create cipher: {}", e)))?;

    let nonce =
        Nonce::try_from(iv).map_err(|_| AppError::Encryption("Invalid IV length".into()))?;

    cipher
        .decrypt(&nonce, ciphertext_with_tag.as_ref())
        .map_err(|_| AppError::Auth("Invalid password or corrupted key file".into()))
}

/// Generate a recovery key in readable format (XXXX-XXXX-XXXX-XXXX-XXXX-XXXX)
/// Uses unambiguous characters to avoid confusion
pub fn generate_recovery_key() -> String {
    const CHARS: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789"; // No 0, O, 1, I
    let mut key = String::with_capacity(29); // 24 chars + 5 dashes
    let mut rng = rand::rng();
    let mut bytes = [0u8; 24];
    rng.fill_bytes(&mut bytes);

    for (i, &byte) in bytes.iter().enumerate() {
        if i > 0 && i % 4 == 0 {
            key.push('-');
        }
        key.push(CHARS[(byte as usize) % CHARS.len()] as char);
    }

    key
}

/// Convert master key to hex string for SQLCipher PRAGMA key
pub fn master_key_to_hex(master_key: &[u8]) -> String {
    format!("'{}'", hex::encode(master_key))
}

/// Convert hex string back to master key bytes
/// Handles the format from master_key_to_hex (with surrounding quotes)
pub fn hex_to_master_key(hex_str: &str) -> Result<[u8; KEY_LENGTH]> {
    // Remove surrounding quotes if present
    let clean_hex = hex_str.trim_matches('\'');

    let bytes = hex::decode(clean_hex)
        .map_err(|e| AppError::Encryption(format!("Invalid hex key: {}", e)))?;

    if bytes.len() != KEY_LENGTH {
        return Err(AppError::Encryption(format!(
            "Invalid key length: expected {}, got {}",
            KEY_LENGTH,
            bytes.len()
        )));
    }

    let mut key = [0u8; KEY_LENGTH];
    key.copy_from_slice(&bytes);
    Ok(key)
}

/// Read salt from file
///
/// A missing, unreadable or wrong-sized salt means the key files are damaged;
/// the (user-facing) error is the stable `auth.keyFilesCorrupted` key and the
/// path only goes to the log.
pub fn read_salt(path: &Path) -> Result<[u8; SALT_LENGTH]> {
    let data = fs::read(path).map_err(|e| {
        log::warn!("[AUTH] Failed to read salt file {}: {}", path.display(), e);
        AppError::Auth(AUTH_KEY_FILES_CORRUPTED.into())
    })?;

    if data.len() != SALT_LENGTH {
        log::warn!(
            "[AUTH] Salt file {} has invalid size {} (expected {})",
            path.display(),
            data.len(),
            SALT_LENGTH
        );
        return Err(AppError::Auth(AUTH_KEY_FILES_CORRUPTED.into()));
    }

    let mut salt = [0u8; SALT_LENGTH];
    salt.copy_from_slice(&data);
    Ok(salt)
}

/// Names of the secret files kept next to the database (salt, password-wrapped
/// master key, recovery-wrapped master key).
pub const SECRET_FILE_NAMES: [&str; 3] = ["salt", "key.enc", "recovery.enc"];

/// Create (or truncate) a secret file readable and writable by its owner only.
///
/// On Unix the file is created with mode 0o600 so it is never visible to other
/// local users, not even briefly. `mode` only applies when the file is created,
/// so a pre-existing, more permissive file is tightened explicitly before the
/// new content goes in. On Windows the file inherits the per-user ACL of the
/// app data directory, which is already private to the user.
fn create_secret_file(path: &Path) -> std::io::Result<fs::File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

        let file = fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        Ok(file)
    }
    #[cfg(not(unix))]
    {
        fs::File::create(path)
    }
}

/// Write a secret file readable and writable by its owner only (see [`create_secret_file`]).
fn write_secret_file(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    create_secret_file(path)?.write_all(data)
}

/// Where [`replace_secret_file`] stages the new content of `path`: `<name>.tmp` in the same
/// folder, so the rename never crosses file systems. A crash can leave it behind; the next write
/// of the same file overwrites it and deleting the account removes it.
pub fn staging_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

/// Serialises [`replace_secret_file`]: two writes of one file share its staging path, and one
/// would otherwise truncate the other's staged content or rename it away.
static SECRET_FILE_WRITES: Mutex<()> = Mutex::new(());

/// Replace a secret file atomically (ADR 0018): the new content goes to [`staging_path`]
/// (owner-only), is flushed to disk and renamed over `path`, then the folder is flushed so the
/// rename survives a power cut. A crash at any point leaves either the old or the new file, never
/// a truncated one. On an error `path` is untouched and the staging file is removed.
fn replace_secret_file(path: &Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;

    // A panic while holding the lock leaves no state behind worth refusing to write over.
    let _writing = SECRET_FILE_WRITES
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let staging = staging_path(path);
    let staged = create_secret_file(&staging)
        .and_then(|mut file| {
            file.write_all(data)?;
            file.sync_all()
        })
        .and_then(|()| fs::rename(&staging, path));
    if let Err(e) = staged {
        let _ = fs::remove_file(&staging);
        return Err(e);
    }
    // The new file is in place from here on, so a caller must not see an error (a password
    // change would then report failure with the new password already active).
    if let Err(e) = sync_parent_dir(path) {
        log::warn!("[AUTH] Could not flush the data folder after replacing a key file: {e}");
    }
    Ok(())
}

/// Flush a folder's entries (the rename that just happened in it) to disk. `std` offers no
/// directory handle on Windows, where the rename replaces the target in one step.
fn sync_parent_dir(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        match path.parent() {
            Some(dir) if !dir.as_os_str().is_empty() => fs::File::open(dir)?.sync_all(),
            _ => Ok(()),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

/// Tighten the secret files in `data_dir` to 0o600 when they are more
/// permissive. Profiles created before the permissions fix, and files
/// extracted from a backup archive, carry the process umask (typically 0o644).
///
/// Best effort: a failure is logged and never blocks unlocking. Returns the
/// number of files that were changed. Always 0 on non-Unix platforms.
pub fn restrict_key_file_permissions(data_dir: &Path) -> usize {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut tightened = 0;
        for name in SECRET_FILE_NAMES {
            let path = data_dir.join(name);
            let Ok(meta) = fs::metadata(&path) else {
                continue; // missing file: nothing to tighten
            };
            if meta.permissions().mode() & 0o077 == 0 {
                continue;
            }
            match fs::set_permissions(&path, fs::Permissions::from_mode(0o600)) {
                Ok(()) => tightened += 1,
                Err(e) => log::warn!("[AUTH] Could not restrict permissions of {name}: {e}"),
            }
        }
        tightened
    }
    #[cfg(not(unix))]
    {
        let _ = data_dir;
        0
    }
}

/// Write salt to file
pub fn write_salt(path: &Path, salt: &[u8; SALT_LENGTH]) -> Result<()> {
    write_secret_file(path, salt)
        .map_err(|e| AppError::Encryption(format!("Failed to write salt: {}", e)))
}

/// Read encrypted key from file
pub fn read_encrypted_key(path: &Path) -> Result<Vec<u8>> {
    fs::read(path).map_err(|e| AppError::Encryption(format!("Failed to read encrypted key: {}", e)))
}

/// Write encrypted key to file, replacing an existing one atomically
pub fn write_encrypted_key(path: &Path, encrypted_key: &[u8]) -> Result<()> {
    replace_secret_file(path, encrypted_key)
        .map_err(|e| AppError::Encryption(format!("Failed to write encrypted key: {}", e)))
}

/// Store master key encrypted with password, with the cost parameters recorded
/// in the file header ([`KdfParams::CURRENT`]).
pub fn store_password_encrypted_key(
    master_key: &[u8; KEY_LENGTH],
    password: &str,
    salt: &[u8],
    key_file_path: &Path,
) -> Result<()> {
    let cost = KdfParams::CURRENT;
    let derived_key = derive_key_with(password, salt, cost)?;
    let encrypted = encrypt_with_key(master_key, &derived_key)?;
    let mut file = Vec::with_capacity(KDF_HEADER_LENGTH + encrypted.len());
    file.extend_from_slice(&cost.to_header());
    file.extend_from_slice(&encrypted);
    write_encrypted_key(key_file_path, &file)
}

/// Re-wrap `key.enc` with [`KdfParams::CURRENT`] when it was written with weaker parameters (a
/// headerless legacy file, or a weaker header). `master_key` must be what the file was just
/// unlocked to with `password`. Returns whether the file was rewritten; a file at the current
/// cost or a stronger one is left alone. The rewrite is atomic, so an error leaves the old file,
/// which still opens the profile. ADR 0018.
pub fn upgrade_password_key_file(
    master_key: &[u8; KEY_LENGTH],
    password: &str,
    salt: &[u8],
    key_file_path: &Path,
) -> Result<bool> {
    let file = read_encrypted_key(key_file_path)?;
    let Some((cost, _)) = split_key_file(&file) else {
        return Err(AppError::Auth(AUTH_KEY_FILES_CORRUPTED.into()));
    };
    if !cost.is_weaker_than(KdfParams::CURRENT) {
        return Ok(false);
    }
    store_password_encrypted_key(master_key, password, salt, key_file_path)?;
    Ok(true)
}

/// Store master key encrypted with recovery key. Stays in the headerless
/// 60-byte layout at [`KdfParams::LEGACY`]: the key has 120 bits of entropy, so
/// a slow KDF adds nothing, and older builds can still read it.
pub fn store_recovery_encrypted_key(
    master_key: &[u8; KEY_LENGTH],
    recovery_key: &str,
    salt: &[u8],
    recovery_file_path: &Path,
) -> Result<()> {
    let normalized = normalize_recovery_key(recovery_key);
    let derived_key = derive_key(&normalized, salt)?;
    let encrypted = encrypt_with_key(master_key, &derived_key)?;
    write_encrypted_key(recovery_file_path, &encrypted)
}

/// Shared implementation of the password / recovery-key decryption paths.
///
/// Distinguishes between damaged key files (missing, wrong size) and a wrong
/// secret (AES-GCM authentication failure). Returns stable `auth.*` keys and
/// logs the file path instead of putting it into the error.
fn decrypt_master_key_file(
    secret: &str,
    salt: &[u8],
    key_file_path: &Path,
    wrong_secret_error: &str,
) -> Result<[u8; KEY_LENGTH]> {
    let encrypted = read_encrypted_key(key_file_path).map_err(|e| {
        log::warn!(
            "[AUTH] Failed to read key file {}: {}",
            key_file_path.display(),
            e
        );
        AppError::Auth(AUTH_KEY_FILES_CORRUPTED.into())
    })?;

    let Some((cost, payload)) = split_key_file(&encrypted) else {
        log::warn!(
            "[AUTH] Key file {} has invalid size {} or header (expected {} or {} bytes)",
            key_file_path.display(),
            encrypted.len(),
            ENCRYPTED_KEY_LENGTH,
            KDF_HEADER_LENGTH + ENCRYPTED_KEY_LENGTH
        );
        return Err(AppError::Auth(AUTH_KEY_FILES_CORRUPTED.into()));
    };

    let derived_key = derive_key_with(secret, salt, cost)?;
    let decrypted = decrypt_with_key(payload, &derived_key).map_err(|_| {
        log::warn!(
            "[AUTH] Decryption of {} failed (wrong secret, or 'salt' and key files are from different backups)",
            key_file_path.display()
        );
        AppError::Auth(wrong_secret_error.into())
    })?;

    if decrypted.len() != KEY_LENGTH {
        log::warn!(
            "[AUTH] Key file {} decrypted to {} bytes (expected {})",
            key_file_path.display(),
            decrypted.len(),
            KEY_LENGTH
        );
        return Err(AppError::Auth(AUTH_KEY_FILES_CORRUPTED.into()));
    }

    let mut master_key = [0u8; KEY_LENGTH];
    master_key.copy_from_slice(&decrypted);
    Ok(master_key)
}

/// Decrypt master key using password
pub fn decrypt_master_key_with_password(
    password: &str,
    salt: &[u8],
    key_file_path: &Path,
) -> Result<[u8; KEY_LENGTH]> {
    decrypt_master_key_file(password, salt, key_file_path, AUTH_WRONG_PASSWORD)
}

/// Decrypt master key using recovery key
///
/// The key is normalized first (uppercase, whitespace and dashes removed) so
/// that a key typed in lowercase or with spaces is accepted.
pub fn decrypt_master_key_with_recovery(
    recovery_key: &str,
    salt: &[u8],
    recovery_file_path: &Path,
) -> Result<[u8; KEY_LENGTH]> {
    let normalized = normalize_recovery_key(recovery_key);
    decrypt_master_key_file(
        &normalized,
        salt,
        recovery_file_path,
        AUTH_INVALID_RECOVERY_KEY,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_derivation() {
        let salt = generate_salt();
        let key1 = derive_key("password123", &salt).unwrap();
        let key2 = derive_key("password123", &salt).unwrap();
        assert_eq!(key1, key2);

        let key3 = derive_key("different", &salt).unwrap();
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_encrypt_decrypt_roundtrip() {
        let key = generate_master_key();
        let data = b"Hello, World!";

        let encrypted = encrypt_with_key(data, &key).unwrap();
        assert_ne!(encrypted.as_slice(), data);

        let decrypted = decrypt_with_key(&encrypted, &key).unwrap();
        assert_eq!(decrypted, data);
    }

    #[test]
    fn test_recovery_key_format() {
        let key = generate_recovery_key();
        assert_eq!(key.len(), 29); // 24 chars + 5 dashes
        assert_eq!(key.matches('-').count(), 5);
    }

    #[test]
    fn test_master_key_hex_round_trip() {
        let key = generate_master_key();
        let hex = master_key_to_hex(&key);
        // master_key_to_hex returns format: 'hexstring' (with single quotes)
        // 32 bytes = 64 hex chars + 2 surrounding single quotes = 66 chars total
        assert_eq!(hex.len(), 66);
        assert!(hex.starts_with('\''));
        assert!(hex.ends_with('\''));
    }

    #[test]
    fn test_generate_master_key_produces_unique_keys() {
        let k1 = generate_master_key();
        let k2 = generate_master_key();
        assert_ne!(k1, k2);
    }

    #[test]
    fn test_generate_salt_produces_unique_salts() {
        let s1 = generate_salt();
        let s2 = generate_salt();
        assert_ne!(s1, s2);
    }

    // ------------------------------------------------------------------
    // Recovery key normalization
    // ------------------------------------------------------------------

    const CANONICAL_KEY: &str = "ABCD-EFGH-JKLM-NPQR-STUV-WXYZ";

    #[test]
    fn normalize_recovery_key_canonical_key_loses_only_dashes() {
        assert_eq!(
            normalize_recovery_key(CANONICAL_KEY),
            "ABCDEFGHJKLMNPQRSTUVWXYZ"
        );
    }

    #[test]
    fn normalize_recovery_key_uppercases_and_strips_spaces() {
        assert_eq!(
            normalize_recovery_key("abcd-efgh jklm npqr-stuv wxyz"),
            "ABCDEFGHJKLMNPQRSTUVWXYZ"
        );
    }

    #[test]
    fn normalize_recovery_key_strips_surrounding_and_inner_whitespace_of_any_kind() {
        assert_eq!(
            normalize_recovery_key("  abcd efgh\tjklm\nnpqr\u{a0}stuv wxyz \r\n"),
            "ABCDEFGHJKLMNPQRSTUVWXYZ"
        );
    }

    #[test]
    fn normalize_recovery_key_strips_unicode_dashes_from_pasted_text() {
        // Word processors and chat apps replace '-' with typographic dashes.
        assert_eq!(
            normalize_recovery_key("abcd\u{2013}efgh\u{2014}jklm\u{2011}npqr\u{2212}stuv-wxyz"),
            "ABCDEFGHJKLMNPQRSTUVWXYZ"
        );
    }

    #[test]
    fn normalize_recovery_key_is_idempotent() {
        let once = normalize_recovery_key("abcd-efgh jklm");
        assert_eq!(normalize_recovery_key(&once), once);
    }

    #[test]
    fn normalize_recovery_key_matches_generated_keys() {
        let key = generate_recovery_key();
        let normalized = normalize_recovery_key(&key.to_lowercase());
        assert_eq!(normalized, key.replace('-', ""));
        assert_eq!(normalized.chars().count(), 24);
    }

    // ------------------------------------------------------------------
    // Typed auth errors instead of leaking file paths
    // ------------------------------------------------------------------

    fn assert_auth_error(err: AppError, expected: &str) {
        match err {
            AppError::Auth(msg) => assert_eq!(msg, expected),
            other => panic!("expected AppError::Auth({expected:?}), got {other:?}"),
        }
    }

    /// Writes a salt + password key file + recovery key file into a temp dir.
    fn write_key_files(
        dir: &Path,
        password: &str,
        recovery_key: &str,
    ) -> ([u8; KEY_LENGTH], [u8; SALT_LENGTH]) {
        let master = generate_master_key();
        let salt = generate_salt();
        write_salt(&dir.join("salt"), &salt).unwrap();
        store_password_encrypted_key(&master, password, &salt, &dir.join("key.enc")).unwrap();
        store_recovery_encrypted_key(&master, recovery_key, &salt, &dir.join("recovery.enc"))
            .unwrap();
        (master, salt)
    }

    #[test]
    fn decrypt_with_password_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let (master, salt) = write_key_files(dir.path(), "correct horse", CANONICAL_KEY);
        let got =
            decrypt_master_key_with_password("correct horse", &salt, &dir.path().join("key.enc"))
                .unwrap();
        assert_eq!(got, master);
    }

    #[test]
    fn wrong_password_returns_stable_key_without_file_path() {
        let dir = tempfile::tempdir().unwrap();
        let (_, salt) = write_key_files(dir.path(), "correct horse", CANONICAL_KEY);
        let err = decrypt_master_key_with_password("wrong", &salt, &dir.path().join("key.enc"))
            .unwrap_err();
        assert!(
            !err.to_string()
                .contains(dir.path().to_string_lossy().as_ref()),
            "error must not leak the data directory: {err}"
        );
        assert_auth_error(err, "auth.wrongPassword");
    }

    #[test]
    fn password_key_file_with_wrong_length_is_reported_as_corrupted_not_wrong_password() {
        let dir = tempfile::tempdir().unwrap();
        let (_, salt) = write_key_files(dir.path(), "pw-12345", CANONICAL_KEY);
        fs::write(dir.path().join("key.enc"), b"too short").unwrap();
        let err = decrypt_master_key_with_password("pw-12345", &salt, &dir.path().join("key.enc"))
            .unwrap_err();
        assert_auth_error(err, "auth.keyFilesCorrupted");
    }

    #[test]
    fn missing_password_key_file_is_reported_as_corrupted() {
        let dir = tempfile::tempdir().unwrap();
        let salt = generate_salt();
        let err = decrypt_master_key_with_password("pw-12345", &salt, &dir.path().join("key.enc"))
            .unwrap_err();
        assert!(
            !err.to_string()
                .contains(dir.path().to_string_lossy().as_ref()),
            "error must not leak the data directory: {err}"
        );
        assert_auth_error(err, "auth.keyFilesCorrupted");
    }

    #[test]
    fn wrong_recovery_key_returns_invalid_recovery_key() {
        let dir = tempfile::tempdir().unwrap();
        let (_, salt) = write_key_files(dir.path(), "pw-12345", CANONICAL_KEY);
        let err = decrypt_master_key_with_recovery(
            "AAAA-AAAA-AAAA-AAAA-AAAA-AAAA",
            &salt,
            &dir.path().join("recovery.enc"),
        )
        .unwrap_err();
        assert!(
            !err.to_string()
                .contains(dir.path().to_string_lossy().as_ref()),
            "error must not leak the data directory: {err}"
        );
        assert_auth_error(err, "auth.invalidRecoveryKey");
    }

    #[test]
    fn recovery_key_file_with_wrong_length_is_reported_as_corrupted() {
        let dir = tempfile::tempdir().unwrap();
        let (_, salt) = write_key_files(dir.path(), "pw-12345", CANONICAL_KEY);
        fs::write(dir.path().join("recovery.enc"), [0u8; 10]).unwrap();
        let err = decrypt_master_key_with_recovery(
            CANONICAL_KEY,
            &salt,
            &dir.path().join("recovery.enc"),
        )
        .unwrap_err();
        assert_auth_error(err, "auth.keyFilesCorrupted");
    }

    #[test]
    fn recovery_key_typed_in_lowercase_with_spaces_decrypts() {
        let dir = tempfile::tempdir().unwrap();
        let (master, salt) = write_key_files(dir.path(), "pw-12345", CANONICAL_KEY);
        let typed = "abcd efgh jklm npqr stuv wxyz";
        let got = decrypt_master_key_with_recovery(typed, &salt, &dir.path().join("recovery.enc"))
            .unwrap();
        assert_eq!(got, master);
    }

    #[test]
    fn recovery_key_stored_from_messy_input_can_be_read_back_canonically() {
        // store and decrypt must normalize identically, otherwise a key saved
        // from a pasted/lowercase value could never be used again.
        let dir = tempfile::tempdir().unwrap();
        let master = generate_master_key();
        let salt = generate_salt();
        let path = dir.path().join("recovery.enc");
        store_recovery_encrypted_key(&master, "abcd-efgh jklm-npqr-stuv-wxyz", &salt, &path)
            .unwrap();
        let got = decrypt_master_key_with_recovery(CANONICAL_KEY, &salt, &path).unwrap();
        assert_eq!(got, master);
    }

    #[test]
    fn read_salt_with_wrong_size_is_reported_as_corrupted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("salt");
        fs::write(&path, b"short").unwrap();
        assert_auth_error(read_salt(&path).unwrap_err(), "auth.keyFilesCorrupted");
    }

    #[test]
    fn read_salt_missing_file_is_reported_as_corrupted_without_path() {
        let dir = tempfile::tempdir().unwrap();
        let err = read_salt(&dir.path().join("salt")).unwrap_err();
        assert!(
            !err.to_string()
                .contains(dir.path().to_string_lossy().as_ref()),
            "error must not leak the data directory: {err}"
        );
        assert_auth_error(err, "auth.keyFilesCorrupted");
    }

    // ------------------------------------------------------------------
    // Versioned scrypt parameters in key.enc (ADR 0009)
    // ------------------------------------------------------------------

    /// A `key.enc` produced by the pre-header code (60 bytes, no header,
    /// N=2^14) for salt `[1; 32]`, password `legacy-pass-123`, master key `[9; 32]`.
    /// Existing profiles must keep unlocking with it forever.
    const LEGACY_KEY_FILE_HEX: &str = "b858249f49221cfbe2bc4a24f0e96b11e5bb8e86d27967fe73598f6579ea67c609be788b4f386a03341d13745f1461485cbff433dc4bf89ead96b4d7";

    #[test]
    fn a_key_file_written_before_the_header_existed_still_unlocks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key.enc");
        let bytes = hex::decode(LEGACY_KEY_FILE_HEX).unwrap();
        assert_eq!(bytes.len(), ENCRYPTED_KEY_LENGTH);
        fs::write(&path, &bytes).unwrap();

        let got = decrypt_master_key_with_password("legacy-pass-123", &[1u8; 32], &path).unwrap();
        assert_eq!(got, [9u8; 32]);

        let err =
            decrypt_master_key_with_password("wrong-pass-123", &[1u8; 32], &path).unwrap_err();
        assert_auth_error(err, "auth.wrongPassword");
    }

    #[test]
    fn new_password_key_files_carry_the_current_parameters() {
        let dir = tempfile::tempdir().unwrap();
        let (master, salt) = write_key_files(dir.path(), "pw-12345", CANONICAL_KEY);

        let file = fs::read(dir.path().join("key.enc")).unwrap();
        assert_eq!(file.len(), KDF_HEADER_LENGTH + ENCRYPTED_KEY_LENGTH);
        assert_eq!(&file[..4], b"MNYK");
        assert_eq!(
            KdfParams::from_header(&file[..KDF_HEADER_LENGTH]),
            Some(KdfParams::CURRENT)
        );

        let got = decrypt_master_key_with_password("pw-12345", &salt, &dir.path().join("key.enc"))
            .unwrap();
        assert_eq!(got, master);
        let err = decrypt_master_key_with_password("pw-54321", &salt, &dir.path().join("key.enc"))
            .unwrap_err();
        assert_auth_error(err, "auth.wrongPassword");
    }

    #[test]
    fn recovery_key_files_stay_legacy_and_headerless() {
        let dir = tempfile::tempdir().unwrap();
        write_key_files(dir.path(), "pw-12345", CANONICAL_KEY);
        let file = fs::read(dir.path().join("recovery.enc")).unwrap();
        assert_eq!(file.len(), ENCRYPTED_KEY_LENGTH);
    }

    #[test]
    fn header_round_trips_for_legacy_and_current_parameters() {
        for params in [KdfParams::LEGACY, KdfParams::CURRENT] {
            assert_eq!(KdfParams::from_header(&params.to_header()), Some(params));
        }
    }

    #[test]
    fn headers_with_a_bad_magic_version_or_out_of_range_cost_are_rejected() {
        let good = KdfParams::CURRENT.to_header();
        assert!(KdfParams::from_header(&good).is_some());

        let mut bad_magic = good;
        bad_magic[0] = b'X';
        assert!(KdfParams::from_header(&bad_magic).is_none());

        let mut bad_version = good;
        bad_version[4] = 2;
        assert!(KdfParams::from_header(&bad_version).is_none());

        for (index, value) in [(5, 13), (5, 19), (5, 40), (6, 0), (6, 17), (7, 0), (7, 17)] {
            let mut header = good;
            header[index] = value;
            assert!(
                KdfParams::from_header(&header).is_none(),
                "header byte {index} = {value} must be rejected"
            );
        }
        assert!(KdfParams::from_header(&good[..7]).is_none());
    }

    #[test]
    fn a_key_file_with_a_hostile_header_is_reported_as_corrupted_without_deriving() {
        let dir = tempfile::tempdir().unwrap();
        let (_, salt) = write_key_files(dir.path(), "pw-12345", CANONICAL_KEY);
        let path = dir.path().join("key.enc");
        let mut file = fs::read(&path).unwrap();
        file[5] = 40; // would ask scrypt for 2^40 iterations
        fs::write(&path, &file).unwrap();

        let started = std::time::Instant::now();
        let err = decrypt_master_key_with_password("pw-12345", &salt, &path).unwrap_err();
        assert_auth_error(err, "auth.keyFilesCorrupted");
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }

    #[test]
    fn a_wrong_sized_headered_file_is_reported_as_corrupted() {
        let dir = tempfile::tempdir().unwrap();
        let (_, salt) = write_key_files(dir.path(), "pw-12345", CANONICAL_KEY);
        let path = dir.path().join("key.enc");
        let mut file = fs::read(&path).unwrap();
        file.push(0);
        fs::write(&path, &file).unwrap();
        let err = decrypt_master_key_with_password("pw-12345", &salt, &path).unwrap_err();
        assert_auth_error(err, "auth.keyFilesCorrupted");
    }

    /// Re-wrapping a legacy file (what a password change does) upgrades it.
    #[test]
    fn rewrapping_a_legacy_key_file_moves_it_to_the_current_parameters() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key.enc");
        let salt = [1u8; 32];
        fs::write(&path, hex::decode(LEGACY_KEY_FILE_HEX).unwrap()).unwrap();

        let master = decrypt_master_key_with_password("legacy-pass-123", &salt, &path).unwrap();
        store_password_encrypted_key(&master, "new-pass-456", &salt, &path).unwrap();

        assert_eq!(
            fs::read(&path).unwrap().len(),
            KDF_HEADER_LENGTH + ENCRYPTED_KEY_LENGTH
        );
        let again = decrypt_master_key_with_password("new-pass-456", &salt, &path).unwrap();
        assert_eq!(again, master);
    }

    // ------------------------------------------------------------------
    // Re-wrapping key.enc at the current cost after unlock (ADR 0018)
    // ------------------------------------------------------------------

    fn legacy_key_file(dir: &Path) -> PathBuf {
        let path = dir.join("key.enc");
        fs::write(&path, hex::decode(LEGACY_KEY_FILE_HEX).unwrap()).unwrap();
        path
    }

    #[test]
    fn only_parameters_current_beats_on_memory_and_work_are_weaker() {
        let current = KdfParams::CURRENT;
        assert!(KdfParams::LEGACY.is_weaker_than(current));
        assert!(!current.is_weaker_than(current));
        let p = |log_n, r, p| KdfParams { log_n, r, p };
        assert!(!p(18, 8, 1).is_weaker_than(current), "stronger");
        assert!(
            !p(16, 16, 1).is_weaker_than(current),
            "same memory and work"
        );
        assert!(
            p(17, 4, 2).is_weaker_than(current),
            "half the memory, same work"
        );
        assert!(p(17, 8, 1).is_weaker_than(p(17, 8, 2)), "less work");
        // Less memory but more work: re-wrapping would cut the work per guess.
        assert!(!p(16, 8, 4).is_weaker_than(current), "mixed");
        assert!(!p(14, 8, 16).is_weaker_than(current), "mixed");
    }

    #[test]
    fn upgrading_a_legacy_key_file_moves_it_to_the_current_cost_under_the_same_password() {
        let dir = tempfile::tempdir().unwrap();
        let path = legacy_key_file(dir.path());
        let salt = [1u8; 32];
        let master = decrypt_master_key_with_password("legacy-pass-123", &salt, &path).unwrap();

        assert!(upgrade_password_key_file(&master, "legacy-pass-123", &salt, &path).unwrap());

        let file = fs::read(&path).unwrap();
        assert_eq!(file.len(), KDF_HEADER_LENGTH + ENCRYPTED_KEY_LENGTH);
        assert_eq!(
            KdfParams::from_header(&file[..KDF_HEADER_LENGTH]),
            Some(KdfParams::CURRENT)
        );
        let again = decrypt_master_key_with_password("legacy-pass-123", &salt, &path).unwrap();
        assert_eq!(again, [9u8; 32]);
        let err = decrypt_master_key_with_password("wrong-pass-123", &salt, &path).unwrap_err();
        assert_auth_error(err, "auth.wrongPassword");
        assert!(!staging_path(&path).exists(), "no staging file is left");
    }

    #[test]
    fn a_key_file_at_the_current_cost_is_not_rewritten() {
        let dir = tempfile::tempdir().unwrap();
        let (master, salt) = write_key_files(dir.path(), "pw-12345", CANONICAL_KEY);
        let path = dir.path().join("key.enc");
        let before = fs::read(&path).unwrap();

        assert!(!upgrade_password_key_file(&master, "pw-12345", &salt, &path).unwrap());

        // A rewrite would use a fresh IV, so equal bytes mean untouched.
        assert_eq!(fs::read(&path).unwrap(), before);
    }

    #[test]
    fn a_key_file_stronger_than_current_is_not_downgraded() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key.enc");
        let stronger = KdfParams {
            log_n: 18,
            r: 8,
            p: 1,
        };
        let mut file = stronger.to_header().to_vec();
        file.extend_from_slice(&[7u8; ENCRYPTED_KEY_LENGTH]);
        fs::write(&path, &file).unwrap();

        assert!(!upgrade_password_key_file(&[9u8; 32], "pw-12345", &[1u8; 32], &path).unwrap());
        assert_eq!(fs::read(&path).unwrap(), file);
    }

    #[test]
    fn a_failed_upgrade_leaves_the_legacy_key_file_intact() {
        let dir = tempfile::tempdir().unwrap();
        let path = legacy_key_file(dir.path());
        let before = fs::read(&path).unwrap();
        // A directory where the staging file goes makes the write fail.
        fs::create_dir(staging_path(&path)).unwrap();

        let result = upgrade_password_key_file(&[9u8; 32], "legacy-pass-123", &[1u8; 32], &path);

        assert!(result.is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        let master = decrypt_master_key_with_password("legacy-pass-123", &[1u8; 32], &path);
        assert_eq!(master.unwrap(), [9u8; 32]);
    }

    #[test]
    fn a_staging_file_left_by_an_interrupted_write_does_not_block_the_upgrade() {
        let dir = tempfile::tempdir().unwrap();
        let path = legacy_key_file(dir.path());
        fs::write(staging_path(&path), b"half-written").unwrap();

        assert!(
            upgrade_password_key_file(&[9u8; 32], "legacy-pass-123", &[1u8; 32], &path).unwrap()
        );

        assert_eq!(
            fs::read(&path).unwrap().len(),
            KDF_HEADER_LENGTH + ENCRYPTED_KEY_LENGTH
        );
        assert!(!staging_path(&path).exists());
    }

    /// Two writes of one key file (a double-submitted unlock, say) share its staging path; they
    /// must not truncate each other's staged content or rename it away.
    #[test]
    fn concurrent_writes_of_one_key_file_all_succeed_and_leave_a_whole_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key.enc");
        let writers: Vec<_> = (1..=4u8)
            .map(|n| {
                let path = path.clone();
                std::thread::spawn(move || {
                    (0..20)
                        .map(|_| write_encrypted_key(&path, &[n; ENCRYPTED_KEY_LENGTH]))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        for writer in writers {
            for result in writer.join().unwrap() {
                result.unwrap();
            }
        }

        let file = fs::read(&path).unwrap();
        assert_eq!(file.len(), ENCRYPTED_KEY_LENGTH);
        assert!(file.iter().all(|&b| b == file[0]), "one write, not a mix");
        assert!(!staging_path(&path).exists());
    }

    #[test]
    fn upgrading_a_damaged_key_file_is_refused_and_leaves_it_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("key.enc");
        fs::write(&path, b"too short").unwrap();

        let err = upgrade_password_key_file(&[9u8; 32], "pw-12345", &[1u8; 32], &path).unwrap_err();

        assert_auth_error(err, "auth.keyFilesCorrupted");
        assert_eq!(fs::read(&path).unwrap(), b"too short");
    }

    // ------------------------------------------------------------------
    // Owner-only permissions for secret files
    // ------------------------------------------------------------------

    #[cfg(unix)]
    mod permissions {
        use super::*;
        use std::os::unix::fs::PermissionsExt;

        fn mode(path: &Path) -> u32 {
            fs::metadata(path).unwrap().permissions().mode() & 0o777
        }

        #[test]
        fn new_key_files_are_created_owner_only() {
            let dir = tempfile::tempdir().unwrap();
            write_key_files(dir.path(), "pw-12345", CANONICAL_KEY);
            for name in SECRET_FILE_NAMES {
                assert_eq!(mode(&dir.path().join(name)), 0o600, "{name}");
            }
        }

        #[test]
        fn rewriting_a_permissive_file_tightens_it() {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("key.enc");
            fs::write(&path, b"old").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

            write_encrypted_key(&path, &[7u8; ENCRYPTED_KEY_LENGTH]).unwrap();

            assert_eq!(mode(&path), 0o600);
            assert_eq!(fs::read(&path).unwrap(), vec![7u8; ENCRYPTED_KEY_LENGTH]);
        }

        #[test]
        fn an_upgraded_key_file_is_owner_only() {
            let dir = tempfile::tempdir().unwrap();
            let path = legacy_key_file(dir.path());
            fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();

            assert!(
                upgrade_password_key_file(&[9u8; 32], "legacy-pass-123", &[1u8; 32], &path)
                    .unwrap()
            );

            assert_eq!(mode(&path), 0o600);
        }

        #[test]
        fn restrict_tightens_permissive_files_and_reports_the_count() {
            let dir = tempfile::tempdir().unwrap();
            for name in SECRET_FILE_NAMES {
                fs::write(dir.path().join(name), b"x").unwrap();
            }
            fs::set_permissions(dir.path().join("salt"), fs::Permissions::from_mode(0o644))
                .unwrap();
            fs::set_permissions(
                dir.path().join("key.enc"),
                fs::Permissions::from_mode(0o664),
            )
            .unwrap();
            fs::set_permissions(
                dir.path().join("recovery.enc"),
                fs::Permissions::from_mode(0o600),
            )
            .unwrap();

            assert_eq!(restrict_key_file_permissions(dir.path()), 2);
            for name in SECRET_FILE_NAMES {
                assert_eq!(mode(&dir.path().join(name)), 0o600, "{name}");
            }
            // Idempotent: nothing left to change.
            assert_eq!(restrict_key_file_permissions(dir.path()), 0);
        }

        #[test]
        fn restrict_leaves_stricter_modes_alone_and_ignores_missing_files() {
            let dir = tempfile::tempdir().unwrap();
            let salt = dir.path().join("salt");
            fs::write(&salt, b"x").unwrap();
            fs::set_permissions(&salt, fs::Permissions::from_mode(0o400)).unwrap();

            assert_eq!(restrict_key_file_permissions(dir.path()), 0);
            assert_eq!(mode(&salt), 0o400);
        }
    }
}

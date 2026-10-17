//! The session's master key, kept in memory while the phone is unlocked (spec §8.1).
//!
//! After every unlock (password, recovery key, first-run setup, biometrics) the phone keeps a copy
//! of the master key in [`SessionKey`] until the app locks again, so biometric unlock can be
//! switched on without asking for the password a second time (`auth::keep_session_key`; the
//! desktop never fills it, ruling R22). The key never crosses into the webview: biometric unlock
//! (`services::biometric_unlock`) stores it in the phone keychain and reads it back from Rust only.

use std::sync::{Mutex, MutexGuard, PoisonError};

use zeroize::Zeroizing;

/// The master key's length in bytes.
pub const KEY_LEN: usize = 32;

/// The master key of the unlocked session, wiped from memory when cleared or replaced.
#[derive(Default)]
pub struct SessionKey(Mutex<Option<Zeroizing<[u8; KEY_LEN]>>>);

impl SessionKey {
    /// Keep `master_key` for the session, replacing (and wiping) an earlier one.
    pub fn set(&self, master_key: &[u8; KEY_LEN]) {
        let mut slot = self.lock();
        // Written in place, so no unwiped copy of the key is left behind on the stack.
        let key = slot.insert(Zeroizing::new([0u8; KEY_LEN]));
        key.copy_from_slice(master_key);
    }

    /// Hand the key out and forget it.
    pub fn take(&self) -> Option<Zeroizing<[u8; KEY_LEN]>> {
        self.lock().take()
    }

    /// Forget the key (logout, account deletion, restore); the memory is wiped.
    pub fn clear(&self) {
        drop(self.lock().take());
    }

    /// The key as the keychain stores it: lowercase hex, wiped when dropped.
    pub fn hex(&self) -> Option<Zeroizing<String>> {
        self.lock()
            .as_ref()
            .map(|key| Zeroizing::new(hex::encode(key.as_slice())))
    }

    fn lock(&self) -> MutexGuard<'_, Option<Zeroizing<[u8; KEY_LEN]>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [
        0x00, 0x11, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88, 0x99, 0xaa, 0xbb, 0xcc, 0xdd, 0xee,
        0xff, 0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54,
        0x32, 0x10,
    ];
    const KEY_HEX: &str = "00112233445566778899aabbccddeeff0123456789abcdeffedcba9876543210";

    #[test]
    fn session_key_starts_empty() {
        let session = SessionKey::default();
        assert!(session.hex().is_none());
        assert!(session.take().is_none());
    }

    #[test]
    fn set_keeps_the_key_and_hex_spells_it_in_lowercase() {
        let session = SessionKey::default();
        session.set(&KEY);
        assert_eq!(session.hex().as_deref().map(String::as_str), Some(KEY_HEX));
        // Reading does not consume.
        assert!(session.hex().is_some());
    }

    #[test]
    fn take_hands_the_key_out_once() {
        let session = SessionKey::default();
        session.set(&KEY);
        assert_eq!(session.take().as_deref(), Some(&KEY));
        assert!(session.take().is_none());
        assert!(session.hex().is_none());
    }

    #[test]
    fn clear_forgets_the_key() {
        let session = SessionKey::default();
        session.set(&KEY);
        session.clear();
        assert!(session.hex().is_none());
        // Clearing an empty session is fine.
        session.clear();
    }

    #[test]
    fn set_replaces_an_earlier_key() {
        let session = SessionKey::default();
        session.set(&[7u8; 32]);
        session.set(&KEY);
        assert_eq!(session.take().as_deref(), Some(&KEY));
    }
}

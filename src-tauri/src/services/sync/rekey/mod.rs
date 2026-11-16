//! Changing the vault key (phase 5 spec §6, §7): the signed key-change bundle (`bundle`) that a
//! device stores on the sync server with the token rotation, which every remaining device opens
//! to pick the new key up and a removed device opens only as its removal notice.

pub mod bundle;

#[cfg(test)]
mod bundle_tests;

//! What DPAPI's answers mean for device sync's binding (`windows.rs`). Pure, so it is compiled and
//! tested on every platform, like `hello_wrap`.
//!
//! The binding is sealed with `CRYPTPROTECT_LOCAL_MACHINE`: the blob names the machine master key
//! it was sealed with, and only that machine has it. On another machine `CryptUnprotectData`
//! answers `NTE_BAD_KEY_STATE` ("Key not valid for use in specified state"); a blob that is not
//! one this machine sealed, or that was altered, answers `NTE_BAD_DATA`, `NTE_BAD_KEY`,
//! `ERROR_INVALID_DATA` or `ERROR_INVALID_PARAMETER`. Those are "not this machine".
//!
//! Only answers about this process or this moment are failures that let the check pass for this
//! start: no rights to reach DPAPI (`ERROR_ACCESS_DENIED`), memory or system resources running out,
//! the local security authority not answering over RPC (it does the work), busy, timed out. Any
//! answer this table does not know is read as "not this machine": the check exists to catch a
//! moved profile, so an unknown answer costs a needless re-join (a fresh device ID), never a check
//! that silently passes. Sealing has no "not this machine": every failure there is a failure.

use crate::KeychainError;

/// `CryptUnprotectData` failures that say nothing about the blob or the machine (as HRESULTs:
/// Win32 codes as `HRESULT_FROM_WIN32`, which is how `windows::core::Error` carries them).
const TRANSIENT: [u32; 9] = [
    0x8007_0005, // ERROR_ACCESS_DENIED
    0x8007_0008, // ERROR_NOT_ENOUGH_MEMORY
    0x8007_000E, // ERROR_OUTOFMEMORY (E_OUTOFMEMORY)
    0x8007_00AA, // ERROR_BUSY
    0x8007_05AA, // ERROR_NO_SYSTEM_RESOURCES
    0x8007_05B4, // ERROR_TIMEOUT
    0x8007_06BA, // RPC_S_SERVER_UNAVAILABLE
    0x8007_06BB, // RPC_S_SERVER_TOO_BUSY
    0x8007_06BE, // RPC_S_CALL_FAILED
];

/// A failed `CryptUnprotectData` of the binding blob, by its HRESULT.
pub(super) fn open_failure(hresult: i32) -> KeychainError {
    let code = hresult as u32;
    if TRANSIENT.contains(&code) {
        KeychainError::Failed(format!("DPAPI could not open the binding: 0x{code:08X}"))
    } else {
        KeychainError::Invalidated
    }
}

/// A failed `CryptProtectData` of the binding, by its HRESULT.
pub(super) fn seal_failure(hresult: i32) -> KeychainError {
    KeychainError::Failed(format!(
        "DPAPI could not seal the binding: 0x{:08X}",
        hresult as u32
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    const NTE_BAD_KEY_STATE: i32 = 0x8009_000B_u32 as i32;
    const NTE_BAD_DATA: i32 = 0x8009_0005_u32 as i32;
    const NTE_BAD_KEY: i32 = 0x8009_0003_u32 as i32;
    const INVALID_DATA: i32 = 0x8007_000D_u32 as i32;
    const INVALID_PARAMETER: i32 = 0x8007_0057_u32 as i32;
    const ACCESS_DENIED: i32 = 0x8007_0005_u32 as i32;
    const OUT_OF_MEMORY: i32 = 0x8007_000E_u32 as i32;
    const NOT_ENOUGH_MEMORY: i32 = 0x8007_0008_u32 as i32;
    const NO_SYSTEM_RESOURCES: i32 = 0x8007_05AA_u32 as i32;
    const RPC_SERVER_UNAVAILABLE: i32 = 0x8007_06BA_u32 as i32;
    const RPC_SERVER_TOO_BUSY: i32 = 0x8007_06BB_u32 as i32;
    const RPC_CALL_FAILED: i32 = 0x8007_06BE_u32 as i32;
    const BUSY: i32 = 0x8007_00AA_u32 as i32;
    const TIMEOUT: i32 = 0x8007_05B4_u32 as i32;

    /// What `CryptUnprotectData` answers for a machine-scope blob from another machine (its
    /// machine master key is not here) or a blob that is not one this machine sealed.
    #[test]
    fn another_machines_blob_is_invalidated() {
        for code in [
            NTE_BAD_KEY_STATE,
            NTE_BAD_DATA,
            NTE_BAD_KEY,
            INVALID_DATA,
            INVALID_PARAMETER,
        ] {
            assert_eq!(
                open_failure(code),
                KeychainError::Invalidated,
                "0x{:08X}",
                code as u32
            );
        }
    }

    #[test]
    fn a_transient_failure_to_open_is_failed_not_invalidated() {
        for code in [
            ACCESS_DENIED,
            OUT_OF_MEMORY,
            NOT_ENOUGH_MEMORY,
            NO_SYSTEM_RESOURCES,
            RPC_SERVER_UNAVAILABLE,
            RPC_SERVER_TOO_BUSY,
            RPC_CALL_FAILED,
            BUSY,
            TIMEOUT,
        ] {
            assert_eq!(
                open_failure(code),
                KeychainError::Failed(format!(
                    "DPAPI could not open the binding: 0x{:08X}",
                    code as u32
                )),
            );
        }
    }

    /// The check exists to catch a profile on another machine: an answer this table does not
    /// know is read as "not this machine" (a needless re-join), never as a pass.
    #[test]
    fn an_unknown_failure_to_open_is_invalidated() {
        const E_FAIL: i32 = 0x8000_4005_u32 as i32;
        const NTE_BAD_KEYSET: i32 = 0x8009_0016_u32 as i32;
        assert_eq!(open_failure(E_FAIL), KeychainError::Invalidated);
        assert_eq!(open_failure(NTE_BAD_KEYSET), KeychainError::Invalidated);
    }

    #[test]
    fn a_failure_to_seal_is_always_failed() {
        for code in [
            NTE_BAD_KEY_STATE,
            ACCESS_DENIED,
            OUT_OF_MEMORY,
            INVALID_DATA,
        ] {
            assert_eq!(
                seal_failure(code),
                KeychainError::Failed(format!(
                    "DPAPI could not seal the binding: 0x{:08X}",
                    code as u32
                )),
            );
        }
    }
}

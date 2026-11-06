//! Hybrid logical clock: `hlc = (ms << 16) | counter`, compared together with the device id.

use crate::error::{AppError, Result};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rand::Rng;

/// A device's identity: 16 random bytes, compared as bytes.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct DeviceId(pub [u8; 16]);

impl DeviceId {
    pub const ZERO: Self = DeviceId([0; 16]);

    pub fn random() -> Self {
        let mut bytes = [0u8; 16];
        rand::rng().fill_bytes(&mut bytes);
        DeviceId(bytes)
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn from_hex(s: &str) -> Result<Self> {
        let bytes = hex::decode(s)
            .map_err(|e| AppError::Internal(format!("invalid device id hex: {e}")))?;
        let arr: [u8; 16] = bytes
            .try_into()
            .map_err(|_| AppError::Internal("device id must be 16 bytes".to_string()))?;
        Ok(DeviceId(arr))
    }

    /// base64url without padding (16 bytes -> 22 characters).
    pub fn to_b64(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }

    /// Strict: exactly 22 characters, no padding, canonical trailing bits.
    pub fn from_b64(s: &str) -> Result<Self> {
        let bad = || AppError::Internal("invalid device id base64url".to_string());
        if s.len() != 22 {
            return Err(bad());
        }
        let bytes = URL_SAFE_NO_PAD.decode(s).map_err(|_| bad())?;
        let arr: [u8; 16] = bytes.try_into().map_err(|_| bad())?;
        Ok(DeviceId(arr))
    }
}

/// A row's clock. Ordered by `(hlc, device)`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Clock {
    pub hlc: u64,
    pub device: DeviceId,
}

impl Clock {
    /// The clock of a row that never had one.
    pub const ZERO: Self = Clock {
        hlc: 0,
        device: DeviceId::ZERO,
    };
}

pub const FUTURE_WARNING_MS: u64 = 86_400_000;
/// Cap of the ms part so a packed clock stays a positive i64.
pub const MAX_MS: u64 = (1 << 47) - 1;

/// The highest clock a device can issue; anything above is clamped to it.
const MAX_CLOCK: u64 = (MAX_MS << 16) | u16::MAX as u64;

pub fn pack(ms: u64, counter: u16) -> u64 {
    (ms << 16) | counter as u64
}

pub fn unpack(hlc: u64) -> (u64, u16) {
    (hlc >> 16, (hlc & 0xFFFF) as u16)
}

pub struct HlcState {
    last: u64,
}

#[derive(Debug)]
pub enum Observation {
    Ok,
    FarFuture { ahead_ms: u64 },
}

impl HlcState {
    pub fn new(last: u64) -> Self {
        Self {
            last: last.min(MAX_CLOCK),
        }
    }

    pub fn last(&self) -> u64 {
        self.last
    }

    /// Issue the next clock, strictly greater than every clock issued or observed so far.
    pub fn next(&mut self, now_ms: u64) -> u64 {
        let (last_ms, counter) = unpack(self.last);
        let ms = now_ms.max(last_ms).min(MAX_MS);
        let next = if ms != last_ms {
            pack(ms, 0)
        } else if counter == u16::MAX {
            if ms < MAX_MS {
                pack(ms + 1, 0)
            } else {
                log::warn!("hybrid logical clock saturated at its maximum");
                pack(ms, u16::MAX)
            }
        } else {
            pack(ms, counter + 1)
        };
        self.last = next;
        next
    }

    /// Take a remote clock into account.
    pub fn observe(&mut self, remote: u64, now_ms: u64) -> Observation {
        let remote = remote.min(MAX_CLOCK);
        self.last = self.last.max(remote);
        let remote_ms = unpack(remote).0;
        match remote_ms.checked_sub(now_ms) {
            Some(ahead_ms) if ahead_ms > FUTURE_WARNING_MS => Observation::FarFuture { ahead_ms },
            _ => Observation::Ok,
        }
    }
}

pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packing_is_48_bits_of_ms_and_16_of_counter() {
        assert_eq!(pack(1_777_000_000_000, 123), 116_457_472_000_000_123);
        assert_eq!(unpack(116_457_472_000_000_123), (1_777_000_000_000, 123));
    }

    #[test]
    fn every_clock_the_device_issues_is_a_positive_sqlite_integer() {
        assert!((pack(MAX_MS, u16::MAX) as i64) > 0); // 6429-10-17T02:45:55.327Z
        assert!((pack(MAX_MS, u16::MAX) as i64) > (pack(1_777_000_000_000, 0) as i64));
        assert!((pack(MAX_MS + 1, 0) as i64) < 0); // why the cap exists
    }

    #[test]
    fn next_never_passes_the_ms_cap() {
        let mut s = HlcState::new(0);
        assert_eq!(unpack(s.next(u64::MAX >> 16)).0, MAX_MS);
        let mut s = HlcState::new(pack(MAX_MS, u16::MAX));
        let a = s.next(MAX_MS);
        assert_eq!(unpack(a), (MAX_MS, u16::MAX));
    }

    #[test]
    fn next_is_strictly_increasing_even_when_the_wall_clock_goes_back() {
        let mut s = HlcState::new(0);
        let a = s.next(1_000);
        let b = s.next(1_000);
        let c = s.next(500);
        assert_eq!(a, pack(1_000, 0));
        assert_eq!(b, pack(1_000, 1));
        assert_eq!(c, pack(1_000, 2));
        let d = s.next(2_000);
        assert_eq!(d, pack(2_000, 0));
        assert_eq!(s.last(), d);
    }

    #[test]
    fn the_counter_overflow_moves_the_ms_part() {
        let mut s = HlcState::new(pack(5, u16::MAX));
        assert_eq!(s.next(5), pack(6, 0));
    }

    #[test]
    fn observe_moves_the_clock_to_the_highest_seen() {
        let mut s = HlcState::new(pack(10, 0));
        assert!(matches!(s.observe(pack(20, 3), 15), Observation::Ok));
        assert_eq!(s.last(), pack(20, 3));
        s.observe(pack(5, 0), 15);
        assert_eq!(s.last(), pack(20, 3));
        assert_eq!(s.next(15), pack(20, 4));
    }

    #[test]
    fn a_clock_more_than_24_hours_ahead_is_reported() {
        let now = 1_000_000_000;
        let mut s = HlcState::new(0);
        assert!(matches!(
            s.observe(pack(now + 86_400_000, 0), now),
            Observation::Ok
        ));
        assert!(matches!(
            s.observe(pack(now + 86_400_001, 0), now),
            Observation::FarFuture {
                ahead_ms: 86_400_001
            }
        ));
    }

    #[test]
    fn equal_hlcs_are_ordered_by_device_bytes() {
        let mut lo = [0u8; 16];
        let mut hi = [0u8; 16];
        lo[15] = 1;
        hi[0] = 1;
        let a = Clock {
            hlc: 7,
            device: DeviceId(lo),
        };
        let b = Clock {
            hlc: 7,
            device: DeviceId(hi),
        };
        assert!(a < b);
        assert!(
            Clock {
                hlc: 8,
                device: DeviceId(lo)
            } > b
        );
    }

    #[test]
    fn the_zero_clock_is_below_every_issued_clock() {
        let mut s = HlcState::new(0);
        let issued = Clock {
            hlc: s.next(0),
            device: DeviceId::ZERO,
        };
        assert!(Clock::ZERO < issued);
        assert_eq!(
            Clock::ZERO,
            Clock {
                hlc: 0,
                device: DeviceId([0; 16])
            }
        );
    }

    #[test]
    fn device_ids_round_trip_hex_and_base64url() {
        let id = DeviceId([0xfb, 0xff, 0xfe, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
        assert_eq!(id.to_hex(), "fbfffe000102030405060708090a0b0c");
        assert_eq!(DeviceId::from_hex(&id.to_hex()).unwrap(), id);
        assert_eq!(id.to_b64(), "-__-AAECAwQFBgcICQoLDA");
        assert_eq!(DeviceId::from_b64(&id.to_b64()).unwrap(), id);
        let r = DeviceId::random();
        assert_eq!(DeviceId::from_b64(&r.to_b64()).unwrap(), r);
        assert_ne!(r, DeviceId::random());
        assert!(DeviceId::from_hex("zz").is_err());
        assert!(DeviceId::from_hex("00").is_err());
        assert!(DeviceId::from_b64("AAAA").is_err());
        assert!(DeviceId::from_b64("-__-AAECAwQFBgcICQoLDA==").is_err());
    }

    #[test]
    fn device_id_base64_is_strict() {
        // 21 and 23 characters, padding, a non-alphabet character, non-canonical trailing bits
        assert!(DeviceId::from_b64("-__-AAECAwQFBgcICQoLD").is_err());
        assert!(DeviceId::from_b64("-__-AAECAwQFBgcICQoLDAA").is_err());
        assert!(DeviceId::from_b64("-__-AAECAwQFBgcICQoLD=").is_err());
        assert!(DeviceId::from_b64("+__-AAECAwQFBgcICQoLDA").is_err());
        assert!(DeviceId::from_b64("-__-AAECAwQFBgcICQoLDB").is_err());
        assert!(DeviceId::from_b64("-__-AAECAwQFBgcICQoLDA").is_ok());
    }

    #[test]
    fn a_clock_above_the_cap_cannot_make_next_go_backwards() {
        let cap = pack(MAX_MS, u16::MAX);
        let s = HlcState::new(u64::MAX);
        assert_eq!(s.last(), cap);
        let mut s = HlcState::new(0);
        s.observe(i64::MAX as u64, 0);
        let last = s.last();
        assert_eq!(last, cap);
        assert!(s.next(1_000) >= last);
        s.observe(u64::MAX, 0);
        assert_eq!(s.last(), cap);
    }
}

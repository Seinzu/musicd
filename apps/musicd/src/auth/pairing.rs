//! Device pairing: a client asks for a short code, shows it to the user, and
//! polls until an admin enters that code in the web UI. Approval mints a
//! `client` token that the next poll hands over exactly once.
//!
//! Requests live in memory only; a restart just means pairing again.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;

pub(crate) const PAIRING_TTL_SECS: i64 = 10 * 60;
pub(crate) const PAIRING_POLL_INTERVAL_SECS: i64 = 2;
const MAX_PENDING_PAIRINGS: usize = 32;
/// No 0/O, 1/I/L, so codes survive being read aloud or retyped.
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789";
pub(crate) const CODE_LENGTH: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PairingStatus {
    Pending,
    Approved { token: String },
    Denied,
}

#[derive(Debug, Clone)]
pub(crate) struct PairingRequest {
    pub(crate) code: String,
    pub(crate) device_name: String,
    pub(crate) peer: Option<IpAddr>,
    pub(crate) created_unix: i64,
    pub(crate) status: PairingStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PollOutcome {
    Pending,
    Approved { token: String },
    Denied,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum StartError {
    TooManyPending,
    Random,
}

/// Keyed by the hash of the client's pairing id, so the registry never holds
/// the secret the client polls with.
#[derive(Debug, Default)]
pub(crate) struct PairingRegistry {
    requests: Mutex<HashMap<String, PairingRequest>>,
}

impl PairingRegistry {
    /// Registers a request. `code_source` yields random bytes for the code
    /// (`None` if the random source failed) and is retried on the unlikely
    /// collision with a live code.
    pub(crate) fn start(
        &self,
        id_hash: String,
        device_name: &str,
        peer: Option<IpAddr>,
        now_unix: i64,
        mut code_source: impl FnMut() -> Option<[u8; CODE_LENGTH]>,
    ) -> Result<String, StartError> {
        let mut requests = self.lock();
        prune_expired(&mut requests, now_unix);
        if requests.len() >= MAX_PENDING_PAIRINGS {
            return Err(StartError::TooManyPending);
        }
        let code = loop {
            let code = code_from_bytes(code_source().ok_or(StartError::Random)?);
            if !requests.values().any(|request| request.code == code) {
                break code;
            }
        };
        requests.insert(
            id_hash,
            PairingRequest {
                code: code.clone(),
                device_name: device_name.to_string(),
                peer,
                created_unix: now_unix,
                status: PairingStatus::Pending,
            },
        );
        Ok(code)
    }

    /// Finds a pending request by the code the user typed.
    pub(crate) fn find_pending_by_code(
        &self,
        code: &str,
        now_unix: i64,
    ) -> Option<(String, PairingRequest)> {
        let code = normalize_code(code)?;
        let mut requests = self.lock();
        prune_expired(&mut requests, now_unix);
        requests
            .iter()
            .find(|(_, request)| request.code == code && request.status == PairingStatus::Pending)
            .map(|(id_hash, request)| (id_hash.clone(), request.clone()))
    }

    /// Records the outcome for a pending request. Returns false if it expired
    /// or was already resolved in the meantime.
    pub(crate) fn resolve(&self, id_hash: &str, status: PairingStatus, now_unix: i64) -> bool {
        let mut requests = self.lock();
        prune_expired(&mut requests, now_unix);
        match requests.get_mut(id_hash) {
            Some(request) if request.status == PairingStatus::Pending => {
                request.status = status;
                true
            }
            _ => false,
        }
    }

    /// Reports a request's state; resolved requests are removed once reported
    /// so a token is handed out only once.
    pub(crate) fn poll(&self, id_hash: &str, now_unix: i64) -> PollOutcome {
        let mut requests = self.lock();
        prune_expired(&mut requests, now_unix);
        let Some(request) = requests.get(id_hash) else {
            return PollOutcome::Unknown;
        };
        match &request.status {
            PairingStatus::Pending => PollOutcome::Pending,
            PairingStatus::Approved { .. } | PairingStatus::Denied => {
                match requests.remove(id_hash).map(|request| request.status) {
                    Some(PairingStatus::Approved { token }) => PollOutcome::Approved { token },
                    _ => PollOutcome::Denied,
                }
            }
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, PairingRequest>> {
        self.requests.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn prune_expired(requests: &mut HashMap<String, PairingRequest>, now_unix: i64) {
    requests.retain(|_, request| now_unix - request.created_unix < PAIRING_TTL_SECS);
}

fn code_from_bytes(bytes: [u8; CODE_LENGTH]) -> String {
    let raw: String = bytes
        .iter()
        .map(|byte| CODE_ALPHABET[usize::from(*byte) % CODE_ALPHABET.len()] as char)
        .collect();
    format!("{}-{}", &raw[..4], &raw[4..])
}

/// Accepts what a person types: any case, with or without the dash or spaces.
pub(crate) fn normalize_code(input: &str) -> Option<String> {
    let raw: String = input
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    (raw.len() == CODE_LENGTH).then(|| format!("{}-{}", &raw[..4], &raw[4..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_code(seed: u8) -> impl FnMut() -> Option<[u8; CODE_LENGTH]> {
        move || Some([seed; CODE_LENGTH])
    }

    #[test]
    fn normalizes_typed_codes() {
        assert_eq!(normalize_code("abcd-efgh"), Some("ABCD-EFGH".to_string()));
        assert_eq!(
            normalize_code(" ab cd ef gh "),
            Some("ABCD-EFGH".to_string())
        );
        assert_eq!(normalize_code("ABCD-EFG"), None);
    }

    #[test]
    fn codes_use_unambiguous_characters() {
        let code = code_from_bytes([0, 1, 2, 3, 250, 251, 252, 253]);
        assert_eq!(code.len(), 9);
        assert!(
            code.chars()
                .all(|c| c == '-' || CODE_ALPHABET.contains(&(c as u8)))
        );
    }

    #[test]
    fn approval_hands_out_the_token_once() {
        let registry = PairingRegistry::default();
        let code = registry
            .start("id-hash".to_string(), "Pixel", None, 100, fixed_code(3))
            .expect("start");
        assert_eq!(registry.poll("id-hash", 101), PollOutcome::Pending);

        let (id_hash, request) = registry
            .find_pending_by_code(&code.to_lowercase(), 102)
            .expect("found");
        assert_eq!(request.device_name, "Pixel");
        assert!(registry.resolve(
            &id_hash,
            PairingStatus::Approved {
                token: "mdt_x".to_string()
            },
            103
        ));
        assert!(registry.find_pending_by_code(&code, 103).is_none());
        assert_eq!(
            registry.poll("id-hash", 104),
            PollOutcome::Approved {
                token: "mdt_x".to_string()
            }
        );
        assert_eq!(registry.poll("id-hash", 105), PollOutcome::Unknown);
    }

    #[test]
    fn requests_expire_and_are_capped() {
        let registry = PairingRegistry::default();
        for index in 0..MAX_PENDING_PAIRINGS {
            // Two positions so every request gets a distinct code.
            let index_byte = u8::try_from(index).expect("small index");
            let bytes = [index_byte, index_byte / 31, 0, 0, 0, 0, 0, 0];
            registry
                .start(format!("id-{index}"), "device", None, 0, move || {
                    Some(bytes)
                })
                .expect("start");
        }
        assert_eq!(
            registry.start("extra".to_string(), "device", None, 0, fixed_code(200)),
            Err(StartError::TooManyPending)
        );
        assert_eq!(
            registry.poll("id-0", PAIRING_TTL_SECS),
            PollOutcome::Unknown
        );
        assert!(
            registry
                .start(
                    "extra".to_string(),
                    "device",
                    None,
                    PAIRING_TTL_SECS,
                    fixed_code(200)
                )
                .is_ok()
        );
    }

    #[test]
    fn denial_is_reported_once() {
        let registry = PairingRegistry::default();
        registry
            .start("id".to_string(), "Laptop", None, 0, fixed_code(9))
            .expect("start");
        assert!(registry.resolve("id", PairingStatus::Denied, 1));
        assert!(!registry.resolve("id", PairingStatus::Denied, 1));
        assert_eq!(registry.poll("id", 2), PollOutcome::Denied);
        assert_eq!(registry.poll("id", 3), PollOutcome::Unknown);
    }
}

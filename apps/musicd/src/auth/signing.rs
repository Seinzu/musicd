//! Signed media URLs. In `required` mode, the stream and artwork URLs the
//! server hands out carry `sig=<hmac of the path>`, so UPnP renderers (which
//! can't send credentials) can still fetch them.
//!
//! Signatures are deterministic and never expire: the queue tracker compares
//! the URL a renderer reports against the one it would generate.

use hmac::{Hmac, Mac};
use sha2::Sha256;

pub(crate) const SIGNATURE_PARAM: &str = "sig";
/// 128 bits of the HMAC, hex encoded.
const SIGNATURE_HEX_LENGTH: usize = 32;

#[derive(Clone)]
pub(crate) struct UrlSigner {
    key: Vec<u8>,
}

impl std::fmt::Debug for UrlSigner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UrlSigner").finish_non_exhaustive()
    }
}

impl UrlSigner {
    pub(crate) fn new(key: &[u8]) -> Self {
        Self { key: key.to_vec() }
    }

    fn mac(&self, path: &str) -> Hmac<Sha256> {
        let mut mac =
            Hmac::<Sha256>::new_from_slice(&self.key).expect("HMAC accepts keys of any length");
        mac.update(path.as_bytes());
        mac
    }

    pub(crate) fn signature(&self, path: &str) -> String {
        let digest = self.mac(path).finalize().into_bytes();
        let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        hex[..SIGNATURE_HEX_LENGTH].to_string()
    }

    pub(crate) fn verify(&self, path: &str, signature: &str) -> bool {
        let Some(bytes) = decode_hex(signature) else {
            return false;
        };
        if bytes.len() * 2 != SIGNATURE_HEX_LENGTH {
            return false;
        }
        self.mac(path).verify_truncated_left(&bytes).is_ok()
    }

    /// Appends the signature for the URL's path. Works on absolute URLs and
    /// on bare paths.
    pub(crate) fn sign_url(&self, url: &str) -> String {
        let path_start = url
            .find("://")
            .and_then(|scheme_end| {
                url[scheme_end + 3..]
                    .find('/')
                    .map(|offset| scheme_end + 3 + offset)
            })
            .unwrap_or(0);
        let without_fragment = url.split('#').next().unwrap_or(url);
        let path = without_fragment[path_start..]
            .split('?')
            .next()
            .unwrap_or_default();
        let separator = if without_fragment.contains('?') {
            '&'
        } else {
            '?'
        };
        format!(
            "{without_fragment}{separator}{SIGNATURE_PARAM}={}",
            self.signature(path)
        )
    }
}

fn decode_hex(value: &str) -> Option<Vec<u8>> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(value.get(index..index + 2)?, 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signs_absolute_urls_and_paths_the_same_way() {
        let signer = UrlSigner::new(b"secret");
        let absolute = signer.sign_url("http://192.168.1.10:7878/stream/track/abc");
        let relative = signer.sign_url("/stream/track/abc");
        let signature = signer.signature("/stream/track/abc");
        assert_eq!(
            absolute,
            format!("http://192.168.1.10:7878/stream/track/abc?sig={signature}")
        );
        assert_eq!(relative, format!("/stream/track/abc?sig={signature}"));
        assert_eq!(
            signer.sign_url("/artwork/album/x?v=2"),
            format!(
                "/artwork/album/x?v=2&sig={}",
                signer.signature("/artwork/album/x")
            )
        );
        assert_eq!(signature.len(), SIGNATURE_HEX_LENGTH);
    }

    #[test]
    fn verifies_only_matching_paths_and_keys() {
        let signer = UrlSigner::new(b"secret");
        let signature = signer.signature("/stream/track/abc");
        assert!(signer.verify("/stream/track/abc", &signature));
        assert!(!signer.verify("/stream/track/abd", &signature));
        assert!(!UrlSigner::new(b"other").verify("/stream/track/abc", &signature));
        assert!(!signer.verify("/stream/track/abc", &signature[..30]));
        assert!(!signer.verify("/stream/track/abc", "zz"));
        assert!(!signer.verify("/stream/track/abc", ""));
    }

    #[test]
    fn signatures_are_deterministic() {
        let signer = UrlSigner::new(b"secret");
        assert_eq!(
            signer.sign_url("http://h/stream/track/a"),
            UrlSigner::new(b"secret").sign_url("http://h/stream/track/a")
        );
    }
}

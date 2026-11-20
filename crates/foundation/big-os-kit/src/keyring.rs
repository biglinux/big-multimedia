//! Secret Service boundary using the system libsecret `secret-tool` utility.
//!
//! All functions block and belong in a worker. The helper receives secrets only
//! on stdin, never argv/attributes. It may prompt to unlock the default collection.
//! A missing helper, locked service, declined prompt, timeout or missing referenced
//! item is an error; no case authorizes writing plaintext instead.
use crate::subprocess::BigSubprocessSpec;
use std::io;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// Limit below secret-tool's 8192-byte stdin buffer (including future metadata).
pub const MAX_SECRET_BYTES: usize = 7900;
/// Injectable boundary so tests never touch a real user's keyring.
pub trait Vault {
    /// Store one immutable item under a fresh [`crate::random::random_hex_id`].
    fn store(&self, scope: &str, id: &str, value: &[u8]) -> io::Result<()>;
    /// Retrieve exactly the referenced item. Absence is not a plaintext fallback.
    fn lookup(&self, scope: &str, id: &str) -> io::Result<Vec<u8>>;
    /// Remove only the referenced item, not all items in a scope.
    fn clear(&self, scope: &str, id: &str) -> io::Result<()>;
}
/// System libsecret bridge. Exits after a bounded supervised operation.
pub struct SecretService<'a> {
    /// Cooperative cancellation supplied by the operation owner.
    pub cancelled: &'a AtomicBool,
}
/// Item scopes this program owns, with the label the keyring shows for their
/// items; any other scope is refused.
const SCOPES: &[(&str, &str)] = &[("iptv", "Big Multimedia IPTV provider")];
/// The keyring label of an item, or an error for a scope or id not ours. Ids
/// come from [`crate::random::random_hex_id`].
fn item_label(scope: &str, id: &str) -> io::Result<&'static str> {
    let label = SCOPES
        .iter()
        .find(|(known, _)| *known == scope)
        .map(|(_, label)| *label);
    match label {
        Some(label) if id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()) => Ok(label),
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid owned secret reference",
        )),
    }
}
impl SecretService<'_> {
    fn run(&self, verb: &str, scope: &str, id: &str, input: Option<&[u8]>) -> io::Result<Vec<u8>> {
        let label = item_label(scope, id)?;
        if input.is_some_and(|v| {
            v.is_empty()
                || v.len() > MAX_SECRET_BYTES
                || std::str::from_utf8(v).is_err()
                || v.contains(&0)
        }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "credential payload exceeds Secret Service limits",
            ));
        }
        let mut arguments = vec![verb.to_owned()];
        if verb == "store" {
            arguments.extend([format!("--label={label}"), "--collection=default".into()]);
        }
        arguments.extend([
            "application".into(),
            "big-multimedia".into(),
            "scope".into(),
            scope.into(),
            "item".into(),
            id.into(),
        ]);
        let builder = BigSubprocessSpec::builder()
            .program("secret-tool")
            .args(arguments)
            .timeout(Duration::from_secs(30));
        let result = if let Some(value) = input {
            builder
                .stdin(value.to_vec())
                .build()
                .run_cancellable(self.cancelled)
        } else {
            builder.build().run_cancellable(self.cancelled)
        };
        // Never expose stdout/stderr or the formatted subprocess
        // error: provider diagnostics are not guaranteed to redact secrets.
        let result = result.map_err(|_| {
            io::Error::other("Secret Service operation unavailable, cancelled or not confirmed")
        })?;
        if !result.status.success() {
            return Err(io::Error::other(
                "Secret Service item unavailable; unlock the keyring and retry",
            ));
        }
        if result.stdout.len() > MAX_SECRET_BYTES {
            return Err(io::Error::other("Secret Service response is too large"));
        }
        Ok(result.stdout)
    }
}
impl Vault for SecretService<'_> {
    fn store(&self, scope: &str, id: &str, value: &[u8]) -> io::Result<()> {
        self.run("store", scope, id, Some(value)).map(|_| ())
    }
    fn lookup(&self, scope: &str, id: &str) -> io::Result<Vec<u8>> {
        self.run("lookup", scope, id, None)
    }
    fn clear(&self, scope: &str, id: &str) -> io::Result<()> {
        self.run("clear", scope, id, None).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreign_scopes_and_ids_are_refused() {
        let id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        assert_eq!(
            item_label("iptv", id).unwrap(),
            "Big Multimedia IPTV provider"
        );
        assert!(item_label("lastfm", id).is_err());
        assert!(item_label("iptv", &id[1..]).is_err());
        assert!(item_label("iptv", &id.replace('a', "g")).is_err());
    }

    /// `secret-tool` reaches whatever Secret Service the session bus offers:
    /// run it only on a private bus with a throwaway keyring (CONTRIBUTING).
    #[test]
    #[ignore = "needs a private D-Bus session with a throwaway Secret Service"]
    fn secret_service_round_trip() {
        assert_eq!(
            std::env::var("BIG_MEDIA_TEST_KEYRING_ISOLATED").as_deref(),
            Ok("1"),
            "refusing to touch a keyring not made for this test"
        );
        let cancelled = AtomicBool::new(false);
        let vault = SecretService {
            cancelled: &cancelled,
        };
        let id = crate::random::random_hex_id().unwrap();
        let secret = "https://h/get.php?username=ana&password=s3cret";
        vault.store("iptv", &id, secret.as_bytes()).unwrap();
        assert_eq!(vault.lookup("iptv", &id).unwrap(), secret.as_bytes());
        vault.clear("iptv", &id).unwrap();
        assert!(vault.lookup("iptv", &id).is_err());
    }
}

//! Recoverable Secret Service migration and credential replacement.
//!
//! Vault and filesystem are not one atomic resource. An immutable candidate item
//! and a non-secret journal bridge the two. Never remove legacy bytes or the old
//! vault item before the new item is read back and its reference is published.
//! No API here is suitable for the GTK main thread.
use std::{fs, io};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use big_os_kit::keyring::{SecretService, Vault, MAX_SECRET_BYTES};
use big_os_kit::{digest, storage};
use serde_json::{Value, json};

const MAX_DOCUMENT: usize = 64*1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider { LastFm, ListenBrainz }
impl Provider {
    pub fn scope(self) -> &'static str { match self { Self::LastFm => "lastfm", Self::ListenBrainz => "listenbrainz" } }
    pub fn path(self) -> io::Result<PathBuf> {
        Ok(big_os_kit::xdg_dirs::config_dir().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound,"private configuration directory unavailable"))?
            .join("big-audio-player").join(format!("{}.json",self.scope())))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageKind { NotConfigured, LegacyFile, SecretService, RecoveryRequired }
/// Carries the exact reviewed revision; Debug deliberately contains no credentials.
#[derive(Clone)]
pub struct AccountReview { provider: Provider, path: PathBuf, bytes: Option<Vec<u8>>, pub kind: StorageKind }
impl std::fmt::Debug for AccountReview {
    fn fmt(&self, f:&mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.debug_struct("AccountReview").field("kind",&self.kind).finish() }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveOutcome { Protected, ProtectedCleanupPending }
fn invalid(message: &'static str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData,message) }
fn json(bytes:&[u8]) -> io::Result<Value> { serde_json::from_slice(bytes).map_err(|_| invalid("invalid account document")) }
fn token(value:&Value) -> io::Result<String> {
    value.as_str().filter(|s|s.len()==32 && s.bytes().all(|b|b.is_ascii_hexdigit()))
        .map(str::to_owned).ok_or_else(||invalid("invalid account reference"))
}
fn reference(bytes:&[u8], provider: Provider) -> io::Result<Option<String>> {
    let doc=json(bytes)?;
    if doc.get("version").is_none() { canonical(provider,bytes)?; return Ok(None); }
    if doc.get("version").and_then(Value::as_u64)!=Some(2) || doc.get("provider").and_then(Value::as_str)!=Some(provider.scope()) {
        return Err(invalid("unsupported credential reference version or provider"));
    }
    if doc.as_object().is_none_or(|o|o.len()!=3) { return Err(invalid("unexpected credential reference fields")); }
    token(&doc["secret_id"]).map(Some)
}
fn journal_path(path:&Path)->PathBuf { let mut name=path.as_os_str().to_os_string();name.push(".vault-transaction");PathBuf::from(name) }
fn before_hash(bytes:Option<&[u8]>)->String { bytes.map_or_else(||"absent".to_string(),digest::bytes) }
fn canonical(provider:Provider, bytes:&[u8])->io::Result<Vec<u8>> {
    let value=json(bytes)?; let object=value.as_object().ok_or_else(||invalid("account must be an object"))?;
    let fields:&[&str]=match provider { Provider::LastFm=>&["api_key","api_secret","session_key","username"],Provider::ListenBrainz=>&["user_token"] };
    if object.keys().any(|k|k!="enabled" && !fields.contains(&k.as_str())) { return Err(invalid("unknown account field")); }
    let mut out=serde_json::Map::new();
    for &field in fields {
        let text=value.get(field).and_then(Value::as_str).filter(|v|v.len()<=4096 && !v.chars().any(char::is_control))
            .ok_or_else(||invalid("invalid credential field"))?;
        out.insert(field.into(),Value::String(text.into()));
    }
    let enabled=match value.get("enabled") { None=>false,Some(Value::Bool(v))=>*v,_=>return Err(invalid("consent flag must be boolean")) };
    out.insert("enabled".into(),Value::Bool(enabled));
    let bytes=serde_json::to_vec(&out).map_err(io::Error::other)?;
    if bytes.len()>MAX_SECRET_BYTES { return Err(invalid("combined credentials exceed keyring helper limit")); }
    Ok(bytes)
}
fn random_id()->io::Result<String> {
    use std::io::Read; let mut raw=[0u8;16]; fs::File::open("/dev/urandom")?.read_exact(&mut raw)?;
    Ok(raw.iter().map(|b|format!("{b:02x}")).collect())
}
/// Inspect without unlocking a vault or causing a migration.
pub fn review(provider:Provider)->io::Result<AccountReview> { review_at(provider,provider.path()?) }
fn review_at(provider:Provider,path:PathBuf)->io::Result<AccountReview> {
    let bytes=storage::read_regular_limited(&path,MAX_DOCUMENT)?;
    let pending=storage::read_regular_limited(&journal_path(&path),4096)?;
    let mut kind=match &bytes { None=>StorageKind::NotConfigured, Some(bytes)=> if reference(bytes,provider)?.is_some() { StorageKind::SecretService } else {StorageKind::LegacyFile} };
    if let Some(journal)=pending { validate_journal(&json(&journal)?,provider)?; kind=StorageKind::RecoveryRequired; }
    Ok(AccountReview{provider,path,bytes,kind})
}
fn validate_journal(j:&Value, p:Provider)->io::Result<()> {
    if j["version"]!=1 || j["provider"].as_str()!=Some(p.scope()) { return Err(invalid("invalid account recovery journal")); }
    token(&j["candidate"])?;
    if !j["previous"].is_null() { token(&j["previous"])?; }
    for key in ["before","payload_hash"] {
        let s=j[key].as_str().ok_or_else(||invalid("invalid account recovery digest"))?;
        if !(key=="before" && s=="absent") && !(s.len()==64 && s.bytes().all(|b|b.is_ascii_hexdigit())) { return Err(invalid("invalid account recovery digest")); }
    }
    Ok(())
}
/// Resolve credentials on a worker. Protected reads never fall back to plaintext.
pub fn load(provider:Provider)->io::Result<Option<Vec<u8>>> {
    let reviewed=review(provider)?;
    let cancel=AtomicBool::new(false); resolve(&reviewed,&SecretService{cancelled:&cancel})
}
fn resolve(review:&AccountReview,vault:&dyn Vault)->io::Result<Option<Vec<u8>>> {
    let Some(bytes)=&review.bytes else {return Ok(None)};
    let payload=match reference(bytes,review.provider)? {
        None=>bytes.clone(), Some(id)=>vault.lookup(review.provider.scope(),&id)?,
    };
    canonical(review.provider,&payload).map(Some)
}
/// Load the exact reviewed revision for an editor, on a service worker. Pending
/// recovery cannot be mistaken for a fresh editable configuration.
pub fn read_reviewed(review: &AccountReview, cancelled: &AtomicBool) -> io::Result<Option<Vec<u8>>> {
    if review.kind == StorageKind::RecoveryRequired { return Err(io::Error::other("Recover the pending account operation before editing")); }
    if storage::read_regular_limited(&review.path, MAX_DOCUMENT)? != review.bytes {
        return Err(io::Error::other("Account changed since review"));
    }
    resolve(review, &SecretService { cancelled })
}

/// Editor seed for a retryable journal. A candidate may not exist if a helper
/// failed before storing it. In that case an explicit editor can re-enter the
/// SAME intended payload (hash-checked by save_reviewed), without abandoning or
/// overwriting the transaction. Only the non-secret consent flag is prefilled.
pub fn editor_seed(review:&AccountReview,cancelled:&AtomicBool)->io::Result<Option<Vec<u8>>> {
    if review.kind!=StorageKind::RecoveryRequired {return read_reviewed(review,cancelled);}
    if storage::read_regular_limited(&review.path,MAX_DOCUMENT)?!=review.bytes {
        return Err(io::Error::other("Account changed since review"));
    }
    let bytes=storage::read_regular_limited(&journal_path(&review.path),4096)?.ok_or_else(||invalid("no pending account transaction"))?;
    let journal=json(&bytes)?;validate_journal(&journal,review.provider)?;
    let enabled=journal["enabled"].as_bool().ok_or_else(||invalid("recover this older transaction before editing"))?;
    Ok(Some(serde_json::to_vec(&json!({"enabled":enabled})).map_err(io::Error::other)?))
}

/// Explicit legacy migration. Existing listening-history consent is preserved,
/// never enabled by storing a token. Failed/uncertain work retains its journal.
pub fn protect(review:&AccountReview,cancelled:&AtomicBool)->io::Result<SaveOutcome> {
    protect_with(review,&SecretService{cancelled})
}
fn protect_with(review:&AccountReview,vault:&dyn Vault)->io::Result<SaveOutcome> {
    let payload=resolve(review,vault)?.ok_or_else(||invalid("account is not configured"))?;
    save_reviewed_with(review,&payload,vault)
}
/// Worker-only credential edit: rejects stale reviewed bytes before external work.
pub fn save_reviewed(review:&AccountReview,payload:&[u8],cancelled:&AtomicBool)->io::Result<SaveOutcome> {
    save_reviewed_with(review,payload,&SecretService{cancelled})
}
fn save_reviewed_with(review:&AccountReview,payload:&[u8],vault:&dyn Vault)->io::Result<SaveOutcome> {
    let payload=canonical(review.provider,payload)?;
    if let Some(parent)=review.path.parent(){ fs::create_dir_all(parent)?; }
    let _lock=storage::WriteLock::try_acquire(&review.path)?;
    let current=storage::read_regular_limited(&review.path,MAX_DOCUMENT)?;
    if current!=review.bytes { return Err(io::Error::other("account changed since review; reload before saving")); }
    let path=journal_path(&review.path);
    let journal=if let Some(bytes)=storage::read_regular_limited(&path,4096)? {
        let journal=json(&bytes)?;validate_journal(&journal,review.provider)?;
        // A previously published transaction needs only durable confirmation and cleanup.
        if current.as_deref().map(|b|reference(b,review.provider)).transpose()?.flatten().as_deref()==journal["candidate"].as_str() {
            if journal["payload_hash"].as_str() != Some(digest::bytes(&payload).as_str()) {
                return Err(io::Error::other("Recover the previous account transaction before applying this edit"));
            }
            return finish(review,&journal,vault);
        }
        if journal["before"].as_str()!=Some(before_hash(current.as_deref()).as_str()) || journal["payload_hash"].as_str()!=Some(digest::bytes(&payload).as_str()) {
            return Err(io::Error::other("another account change needs recovery before this edit"));
        }
        journal
    } else {
        let previous=current.as_deref().map(|b|reference(b,review.provider)).transpose()?.flatten();
        let journal=json!({"version":1,"provider":review.provider.scope(),"candidate":random_id()?,"previous":previous,
            "before":before_hash(current.as_deref()),"payload_hash":digest::bytes(&payload),"enabled":json(&payload)?["enabled"]});
        storage::atomic_write_private_receipt(&path,&serde_json::to_vec(&journal).map_err(io::Error::other)?)?.into_io_result()?;
        journal
    };
    let id=token(&journal["candidate"])?;
    // Retrying the same transaction uses the same immutable candidate payload.
    // A helper timeout is uncertain: never delete this item speculatively.
    vault.store(review.provider.scope(),&id,&payload)?;
    let readback=vault.lookup(review.provider.scope(),&id)?;
    if readback!=payload { return Err(io::Error::other("keyring did not confirm the stored credentials")); }
    // Recheck cooperative expectation even under the lock; non-cooperative writers
    // are not CAS-safe, so never advertise universal filesystem isolation.
    if storage::read_regular_limited(&review.path,MAX_DOCUMENT)?!=current { return Err(io::Error::other("account changed during vault operation; recovery retained")); }
    let reference=json!({"version":2,"provider":review.provider.scope(),"secret_id":id});
    storage::atomic_write_private_receipt(&review.path,&serde_json::to_vec(&reference).map_err(io::Error::other)?)?.into_io_result()?;
    finish(review,&journal,vault)
}
fn finish(review:&AccountReview,journal:&Value,vault:&dyn Vault)->io::Result<SaveOutcome> {
    let id=token(&journal["candidate"])?;
    let current=storage::read_regular_limited(&review.path,MAX_DOCUMENT)?.ok_or_else(||invalid("published account reference disappeared"))?;
    if reference(&current,review.provider)?.as_deref()!=Some(id.as_str()) { return Err(io::Error::other("account recovery conflicts with a newer reference")); }
    let payload=canonical(review.provider,&vault.lookup(review.provider.scope(),&id)?)?;
    if journal["payload_hash"].as_str()!=Some(digest::bytes(&payload).as_str()) { return Err(invalid("keyring recovery payload differs from the intended value")); }
    storage::sync_parent(&review.path)?;
    if let Some(old)=journal["previous"].as_str() {
        if old!=id && vault.clear(review.provider.scope(),old).is_err() {
            // Protected reference is valid. Keep the journal so an ambiguous old
            // item deletion is never misreported as a completely clean transaction.
            return Ok(SaveOutcome::ProtectedCleanupPending);
        }
    }
    fs::remove_file(journal_path(&review.path))?;
    storage::sync_parent(&review.path)?;
    Ok(SaveOutcome::Protected)
}
/// Resume only the exact journaled operation. Recovery never overwrites newer
/// configuration or deletes credentials on a speculative cancellation.
pub fn recover(review:&AccountReview,cancelled:&AtomicBool)->io::Result<SaveOutcome> {
    let vault=SecretService{cancelled}; recover_with(review,&vault)
}
fn recover_with(review:&AccountReview,vault:&dyn Vault)->io::Result<SaveOutcome> {
    let bytes=storage::read_regular_limited(&journal_path(&review.path),4096)?.ok_or_else(||invalid("no account operation to recover"))?;
    let journal=json(&bytes)?; validate_journal(&journal,review.provider)?;
    let current=storage::read_regular_limited(&review.path,MAX_DOCUMENT)?;
    if current!=review.bytes { return Err(io::Error::other("account changed since review")); }
    if current.as_deref().map(|b|reference(b,review.provider)).transpose()?.flatten().as_deref()==journal["candidate"].as_str() {
        let _lock=storage::WriteLock::try_acquire(&review.path)?;return finish(review,&journal,vault);
    }
    // Legacy migration can replay from its still intact source. For a protected
    // edit, recover the intended candidate from the vault, not the old credentials.
    let payload=match current.as_deref() {
        Some(bytes) if reference(bytes,review.provider)?.is_none()=>canonical(review.provider,bytes)?,
        _=>vault.lookup(review.provider.scope(),&token(&journal["candidate"])?)?,
    };
    save_reviewed_with(review,&payload,vault)
}

#[cfg(test)] mod tests {
    use super::*;
    use std::sync::Mutex;
    use std::collections::HashMap;
    #[derive(Default)] struct Fake { items:Mutex<HashMap<String,Vec<u8>>>, fail_after_store:bool }
    impl Vault for Fake {
        fn store(&self,_:&str,id:&str,v:&[u8])->io::Result<()> {
            self.items.lock().unwrap().insert(id.into(),v.into());
            if self.fail_after_store { Err(io::Error::other("uncertain store")) } else { Ok(()) }
        }
        fn lookup(&self,_:&str,id:&str)->io::Result<Vec<u8>> { self.items.lock().unwrap().get(id).cloned().ok_or_else(||io::Error::other("locked or absent")) }
        fn clear(&self,_:&str,id:&str)->io::Result<()> { self.items.lock().unwrap().remove(id);Ok(()) }
    }
    const LEGACY:&[u8]=br#"{"user_token":"sensitive-value","enabled":false}"#;
    #[test] fn migration_removes_plaintext_only_after_confirmation() {
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("account.json");fs::write(&path,LEGACY).unwrap();
        let reviewed=review_at(Provider::ListenBrainz,path.clone()).unwrap();let vault=Fake::default();
        assert_eq!(protect_with(&reviewed,&vault).unwrap(),SaveOutcome::Protected);
        let disk=fs::read(&path).unwrap();assert!(!String::from_utf8_lossy(&disk).contains("sensitive-value"));
        assert_eq!(json(&resolve(&review_at(Provider::ListenBrainz,path).unwrap(),&vault).unwrap().unwrap()).unwrap()["enabled"],false);
    }
    #[test] fn uncertain_store_preserves_source_and_can_recover() {
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("account.json");fs::write(&path,LEGACY).unwrap();
        let reviewed=review_at(Provider::ListenBrainz,path.clone()).unwrap();let vault=Fake{fail_after_store:true,..Fake::default()};
        assert!(protect_with(&reviewed,&vault).is_err());assert_eq!(fs::read(&path).unwrap(),LEGACY);
        assert!(journal_path(&path).exists());
        let vault=Fake{fail_after_store:false,items:vault.items};
        assert_eq!(recover_with(&review_at(Provider::ListenBrainz,path).unwrap(),&vault).unwrap(),SaveOutcome::Protected);
    }
    #[test] fn stale_review_never_changes_keyring() {
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("account.json");fs::write(&path,LEGACY).unwrap();
        let reviewed=review_at(Provider::ListenBrainz,path.clone()).unwrap();fs::write(path,b"newer").unwrap();
        let vault=Fake::default();assert!(protect_with(&reviewed,&vault).is_err());assert!(vault.items.lock().unwrap().is_empty());
    }
    #[test] fn debug_never_contains_reviewed_secrets() {
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("a");fs::write(&path,LEGACY).unwrap();
        assert!(!format!("{:?}",review_at(Provider::ListenBrainz,path).unwrap()).contains("sensitive-value"));
    }
    #[test] fn different_payload_cannot_replace_an_unfinished_transaction() {
        let dir=tempfile::tempdir().unwrap();let path=dir.path().join("new-account");
        let reviewed=review_at(Provider::ListenBrainz,path.clone()).unwrap();
        let failed=Fake{fail_after_store:true,..Fake::default()};
        assert!(save_reviewed_with(&reviewed,LEGACY,&failed).is_err());
        let reviewed=review_at(Provider::ListenBrainz,path.clone()).unwrap();
        let changed=br#"{"user_token":"another-secret","enabled":false}"#;
        assert!(save_reviewed_with(&reviewed,changed,&failed).is_err());
        let seed=editor_seed(&reviewed,&AtomicBool::new(false)).unwrap().unwrap();
        assert_eq!(json(&seed).unwrap(),json!({"enabled":false}));
        let working=Fake{items:failed.items,fail_after_store:false};
        assert_eq!(save_reviewed_with(&reviewed,LEGACY,&working).unwrap(),SaveOutcome::Protected);
        assert!(!String::from_utf8(fs::read(path).unwrap()).unwrap().contains("sensitive-value"));
    }

}

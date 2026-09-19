//! The assistant's API key, kept where the platform keeps secrets.
//!
//! A key in `preferences.json` is a key in a file anyone can read, copy and
//! sync; the platform's own store — Windows' Credential Manager, macOS's
//! Keychain — is encrypted, per user, and what every other application
//! uses. So on those platforms the key lives there, and the preferences
//! file carries none. Linux is left as it was: its persistent store is a
//! D-Bus secret service that a headless box may not run, and a key that
//! vanished at the next login because the daemon was not there would be
//! worse than a key in a file with the user's permissions.
//!
//! The field in the preferences stays in memory — everything that needs the
//! key reads it from there — and the file is what changes: once the
//! keychain holds the key, the file stops being written with it. A key found
//! in an older file is moved across on the next save.

use std::sync::atomic::{AtomicBool, Ordering};

/// Whether the keychain is the key's home on this platform.
pub const IN_KEYCHAIN: bool = cfg!(any(target_os = "windows", target_os = "macos"));

const SERVICE: &str = "Tessera";
const ACCOUNT: &str = "assistant-api-key";

/// Set once the keychain has taken the key, so the preferences file stops
/// carrying it. False until then, so a keychain that refuses — locked,
/// absent, a test — leaves the key in the file rather than nowhere.
static HELD: AtomicBool = AtomicBool::new(false);

/// Whether the preferences file should leave the key out.
pub fn held(_key: &String) -> bool {
    HELD.load(Ordering::SeqCst)
}

/// The key the keychain holds, if it holds one.
pub fn read() -> Option<String> {
    if !IN_KEYCHAIN {
        return None;
    }
    let entry = keyring::Entry::new(SERVICE, ACCOUNT).ok()?;
    match entry.get_password() {
        Ok(key) => {
            HELD.store(true, Ordering::SeqCst);
            Some(key)
        }
        Err(_) => None,
    }
}

/// Put `key` in the keychain, or take it out when empty. Returns whether
/// the keychain now stands in for the file.
pub fn store(key: &str) -> bool {
    if !IN_KEYCHAIN {
        return false;
    }
    let Ok(entry) = keyring::Entry::new(SERVICE, ACCOUNT) else {
        return false;
    };
    let ok = if key.trim().is_empty() {
        matches!(
            entry.delete_credential(),
            Ok(()) | Err(keyring::Error::NoEntry)
        )
    } else {
        entry.set_password(key.trim()).is_ok()
    };
    HELD.store(ok, Ordering::SeqCst);
    ok
}

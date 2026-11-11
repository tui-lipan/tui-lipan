//! The machine id that tells a terminal whether a dropped file is on the program's machine.
//!
//! The id is `1:` followed by the hex HMAC-SHA256 of the platform's machine id, keyed with
//! `tty-dnd-protocol-machine-id`. Hashing keeps the platform id private and the value printable.
//! An id this build cannot read, or one with another version, is a different machine: the
//! protocol then transfers file contents instead of trusting local paths, which is slower but
//! always correct.

use std::sync::{Arc, OnceLock};

use sha2::{Digest, Sha256};

const KEY: &[u8] = b"tty-dnd-protocol-machine-id";

/// A machine id in the protocol's format.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct MachineId(Arc<str>);

impl MachineId {
    /// The id of the machine this process runs on, when the platform has one.
    pub fn local() -> Option<Self> {
        static LOCAL: OnceLock<Option<MachineId>> = OnceLock::new();
        LOCAL
            .get_or_init(|| platform_id().map(|raw| Self::from_platform_id(&raw)))
            .clone()
    }

    /// The protocol id for a raw platform machine id.
    pub fn from_platform_id(raw: &[u8]) -> Self {
        let digest = hmac_sha256(KEY, raw.trim_ascii_end());
        let mut text = String::with_capacity(2 + digest.len() * 2);
        text.push_str("1:");
        for byte in digest {
            text.push(char::from_digit(u32::from(byte >> 4), 16).unwrap_or('0'));
            text.push(char::from_digit(u32::from(byte & 0xf), 16).unwrap_or('0'));
        }
        Self(Arc::from(text))
    }

    /// Wrap an id received from a peer.
    pub fn from_wire(id: impl Into<Arc<str>>) -> Self {
        Self(id.into())
    }

    /// The id as it travels.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether `other` names the same machine. Only a version-1 id that matches exactly does.
    pub fn same_machine(&self, other: &MachineId) -> bool {
        self.0.starts_with("1:") && self.0.len() >= 20 && self.0 == other.0
    }
}

impl std::fmt::Display for MachineId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    let mut block = [0u8; BLOCK];
    if key.len() > BLOCK {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let pad = |byte: u8| block.map(|b| b ^ byte);
    let inner = Sha256::new()
        .chain_update(pad(0x36))
        .chain_update(message)
        .finalize();
    Sha256::new()
        .chain_update(pad(0x5c))
        .chain_update(inner)
        .finalize()
        .into()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_id() -> Option<Vec<u8>> {
    ["/etc/machine-id", "/var/lib/dbus/machine-id"]
        .into_iter()
        .find_map(|path| std::fs::read(path).ok())
        .map(|raw| raw.trim_ascii().to_vec())
        .filter(|raw| !raw.is_empty())
}

#[cfg(target_os = "macos")]
fn platform_id() -> Option<Vec<u8>> {
    // `IOPlatformUUID` from the platform expert, as `ioreg` prints it.
    let output = std::process::Command::new("/usr/sbin/ioreg")
        .args(["-rd1", "-c", "IOPlatformExpertDevice"])
        .output()
        .ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    let line = text
        .lines()
        .find(|line| line.contains("\"IOPlatformUUID\""))?;
    let value = line.rsplit('=').next()?.trim().trim_matches('"');
    (!value.is_empty()).then(|| value.as_bytes().to_vec())
}

#[cfg(windows)]
fn platform_id() -> Option<Vec<u8>> {
    // `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid`, read through `reg` so the framework
    // needs no registry bindings. The 64-bit view holds it even for a 32-bit process.
    let output = std::process::Command::new("reg")
        .args([
            "query",
            r"HKLM\SOFTWARE\Microsoft\Cryptography",
            "/v",
            "MachineGuid",
            "/reg:64",
        ])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().find(|line| line.contains("MachineGuid"))?;
    let value = line.split_whitespace().last()?;
    (!value.is_empty()).then(|| value.as_bytes().to_vec())
}

#[cfg(not(any(unix, windows)))]
fn platform_id() -> Option<Vec<u8>> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_the_reference_hash() {
        // `machine_id('tty-dnd-protocol-machine-id')` in kitty for this raw id.
        let id = MachineId::from_platform_id(b"0123456789abcdef0123456789abcdef\n");
        assert_eq!(
            id.as_str(),
            "1:a81b6c3c9d37b0caa4a9c6b7de41059238bc101829db2975223b93d39d663b08"
        );
    }

    #[test]
    fn only_a_known_version_matches() {
        let id = MachineId::from_platform_id(b"abc");
        assert!(id.same_machine(&id.clone()));
        let other_version = MachineId::from_wire(id.as_str().replacen("1:", "2:", 1));
        assert!(!other_version.same_machine(&other_version.clone()));
        assert!(!id.same_machine(&MachineId::from_platform_id(b"abd")));
    }
}

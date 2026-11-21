use anyhow::{bail, Result};
#[cfg(windows)]
pub fn machine_fingerprint() -> Result<String> {
    use sha2::{Digest, Sha256};
    use winreg::{enums::HKEY_LOCAL_MACHINE, RegKey};
    let key =
        RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey("SOFTWARE\\Microsoft\\Cryptography")?;
    let raw: String = key.get_value("MachineGuid")?;
    let id = raw.trim().to_ascii_lowercase();
    if id.is_empty() {
        bail!("empty MachineGuid")
    }
    Ok(format!("{:x}", Sha256::digest(id.as_bytes())))
}
#[cfg(not(windows))]
pub fn machine_fingerprint() -> Result<String> {
    bail!("Windows only")
}

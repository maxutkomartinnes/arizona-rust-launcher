use anyhow::Result;
#[cfg(windows)]
pub fn set(enabled: bool) -> Result<()> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let root = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = root.create_subkey("Software\\Microsoft\\Windows\\CurrentVersion\\Run")?;
    if enabled {
        let exe = std::env::current_exe()?;
        key.set_value("ArizonaRustLauncher", &format!("\"{}\"", exe.display()))?;
    } else {
        let _ = key.delete_value("ArizonaRustLauncher");
    }
    Ok(())
}
#[cfg(not(windows))]
pub fn set(_enabled: bool) -> Result<()> {
    anyhow::bail!("Windows only")
}

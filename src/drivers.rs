use anyhow::{bail, Result};
use std::{fs, path::Path, process::Command};

pub fn install<F: FnMut(&str)>(root: &Path, mut progress: F) -> Result<()> {
    let dir = root.join("drivers");
    if !dir.is_dir() {
        bail!("drivers directory not found")
    }
    let directx = dir.join("directx_Jun2010_redist.exe");
    if directx.is_file() {
        progress("DirectX June 2010");
        let temp = dir.join("DXTEMP");
        fs::create_dir_all(&temp)?;
        let code = Command::new(&directx)
            .current_dir(&dir)
            .arg("/Q")
            .arg(format!("/T:{}", temp.display()))
            .status()?;
        if !code.success() {
            bail!("DirectX extraction failed")
        };
        let setup = temp.join("DXSETUP.exe");
        if !setup.is_file() {
            bail!("DXSETUP.exe not found after extraction")
        };
        let code = Command::new(setup)
            .current_dir(&temp)
            .arg("/silent")
            .status()?;
        if !code.success() {
            bail!("DirectX setup failed")
        };
    }
    for name in [
        "vcredist_x86_2010.exe",
        "vcredist_x86_2013.exe",
        "vcredist_x86.exe",
        "vc_redist17.x86.exe",
        "vc_redist17.x64.exe",
    ] {
        let exe = dir.join(name);
        if exe.is_file() {
            progress(name);
            let code = Command::new(&exe)
                .current_dir(&dir)
                .args(["/quiet", "/norestart", "/install"])
                .status()?;
            if !code.success() {
                bail!("driver installer failed: {name}")
            }
        }
    }
    Ok(())
}

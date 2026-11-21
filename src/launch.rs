use crate::{
    identity,
    model::{GameId, Server},
    settings::GameSettings,
};
use anyhow::{bail, Context, Result};
use std::{
    path::Path,
    process::{Child, Command},
};

fn validate_text(s: &str, label: &str) -> Result<()> {
    if s.is_empty() || s.len() > 128 || s.chars().any(|c| c.is_control()) {
        bail!("invalid {label}")
    }
    Ok(())
}
pub fn launch(
    game: GameId,
    root: &Path,
    settings: &GameSettings,
    server: &Server,
    cdn_indexes: &str,
    test_password: Option<&str>,
) -> Result<Child> {
    validate_text(&settings.user_name, "nickname")?;
    if settings.user_name.len() < 3
        || settings.user_name.len() > 20
        || !settings
            .user_name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_')
    {
        bail!("nickname must be 3–20 ASCII letters, digits or underscores")
    };
    validate_text(&server.ip, "server host")?;
    if !cdn_indexes.chars().all(|c| c.is_ascii_digit() || c == ',') {
        bail!("invalid CDN index list")
    }
    match game {
        GameId::ArizonaV | GameId::ArizonaVStaging => launch_ragemp(root, server),
        GameId::Trilogy => launch_trilogy(root, settings, server, cdn_indexes),
        _ => launch_gta(root, game, settings, server, cdn_indexes, test_password),
    }
}
fn launch_gta(
    root: &Path,
    game: GameId,
    s: &GameSettings,
    server: &Server,
    cdn: &str,
    password: Option<&str>,
) -> Result<Child> {
    let exe = ["enb.exe", "gta-sa.exe", "gtasa.exe", "gta_sa.exe"]
        .into_iter()
        .map(|p| root.join(p))
        .find(|p| p.is_file())
        .context("GTA SA executable not found")?;
    let mut c = Command::new(exe);
    c.current_dir(root).args([
        "-c",
        "-h",
        &server.ip,
        "-p",
        &server.port.to_string(),
        "-n",
        &s.user_name,
        "-mem",
        &s.memory.to_string(),
        "-cdn",
        cdn,
        "-window",
    ]);
    let opts = &s.options;
    if !opts.get("windowed").copied().unwrap_or(false) {
        c.arg("-modern_scale");
    }
    for (key, flag) in [
        ("autoLogin", "-x"),
        ("wideScreen", "-widescreen"),
        ("preload", "-ldo"),
        ("newGraphics", "-trees_new"),
        ("shitPc", "-t"),
        ("cefDirtyRects", "-cef_dirty_rects"),
        ("authCef", "-auth_cef_enable"),
        ("grass", "-enable_grass"),
        ("oldResolution", "-16bpp"),
        ("hdrResolution", "-allow_hdr"),
    ] {
        if opts.get(key).copied().unwrap_or(false) {
            c.arg(flag);
        }
    }
    c.arg(match game {
        GameId::Arizona | GameId::ArizonaStaging => "-arizona",
        GameId::Rodina | GameId::RodinaStaging => "-rodina",
        _ => "-village",
    });
    if let Ok(id) = identity::machine_fingerprint() {
        c.args(["-userId", &id]);
    }
    if opts.get("testBranch").copied().unwrap_or(false) {
        if let Some(p) = password {
            validate_text(p, "test branch password")?;
            c.args(["-z", p]);
        }
    }
    Ok(c.spawn().context("start GTA SA")?)
}
#[cfg(windows)]
fn launch_ragemp(root: &Path, server: &Server) -> Result<Child> {
    use winreg::{enums::HKEY_CURRENT_USER, RegKey};
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey("SOFTWARE\\RAGE-MP")?;
    key.set_value("launch2.ip", &server.ip)?;
    key.set_value("launch2.port", &server.port.to_string())?;
    key.set_value("launch2.params", &"launcher")?;
    key.set_value("rage_path", &root.to_string_lossy().to_string())?;
    let exe = root.join("updater.exe");
    if !exe.is_file() {
        bail!("RAGE MP updater.exe not found")
    }
    Ok(Command::new(exe).current_dir(root).arg("-gtav").spawn()?)
}
#[cfg(not(windows))]
fn launch_ragemp(_root: &Path, _server: &Server) -> Result<Child> {
    bail!("RAGE MP launch requires Windows")
}
fn launch_trilogy(root: &Path, s: &GameSettings, server: &Server, cdn: &str) -> Result<Child> {
    let pak = root.join("Game/Gameface/Content/Paks/gta.pak");
    if !pak.is_file() {
        let exe = root.join("FilesConsolidator.exe");
        if !exe.is_file() {
            bail!("Trilogy consolidator not found")
        }
        if fs2::available_space(root)? < 23_850_730_160 {
            bail!("Trilogy needs at least 23.85 GB free for consolidation")
        }
        let result = Command::new(exe)
            .current_dir(root)
            .args(["./GameArchive/", "./Game/Gameface/Content/Paks/gta.pak"])
            .status()?;
        if !result.success() || !pak.is_file() {
            bail!("Trilogy consolidation failed")
        }
    }
    let mod_dir = root.join("Game/Gameface/Content/Paks/Mods");
    let active = mod_dir.join("Snow.pak");
    let disabled = mod_dir.join("Snow.pak-disabled");
    if s.options.get("seasons").copied().unwrap_or(false) {
        if disabled.is_file() && !active.exists() {
            std::fs::rename(disabled, active)?
        }
    } else if active.is_file() && !disabled.exists() {
        std::fs::rename(active, disabled)?
    }
    let exe = root.join("Client/GameLauncher.exe");
    if !exe.is_file() {
        bail!("Trilogy GameLauncher.exe not found")
    }
    Ok(Command::new(exe)
        .current_dir(root.join("Client"))
        .args([
            &s.user_name,
            &server.number.saturating_sub(1).to_string(),
            "",
            cdn,
        ])
        .spawn()?)
}

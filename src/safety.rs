use anyhow::{bail, Context, Result};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

pub fn relative(input: &str) -> Result<PathBuf> {
    if input.is_empty() || input.contains(':') || input.starts_with(['/', '\\']) {
        bail!("unsafe manifest path: {input}")
    }
    let mut out = PathBuf::new();
    for part in input.split(['/', '\\']) {
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.trim_end_matches([' ', '.']) != part
        {
            bail!("unsafe manifest component: {input}")
        }
        if part
            .chars()
            .any(|c| c.is_control() || "<>\"|?*".contains(c))
        {
            bail!("unsafe manifest component: {input}")
        }
        let head = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        if [
            "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
            "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
        ]
        .contains(&head.as_str())
        {
            bail!("Windows reserved name: {input}")
        }
        out.push(part);
    }
    if out.components().any(|c| !matches!(c, Component::Normal(_))) {
        bail!("unsafe path: {input}")
    }
    Ok(out)
}

pub fn root(path: &Path) -> Result<PathBuf> {
    let planned = plan_root(path)?;
    fs::create_dir_all(&planned).with_context(|| {
        format!(
            "Не удалось создать папку игры {}. Выберите папку с правом записи, например в профиле пользователя",
            planned.display()
        )
    })?;
    let canonical = planned.canonicalize()?;
    tempfile::Builder::new()
        .prefix(".arizona-write-check-")
        .tempfile_in(&canonical)
        .with_context(|| {
            format!(
                "Нет доступа для записи в {}. Выберите другую папку игры (например, C:\\Users\\<имя>\\Games), вне Program Files",
                canonical.display()
            )
        })?;
    Ok(canonical)
}

pub fn plan_root(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        bail!("укажите полный путь к папке игры")
    }
    let mut missing = Vec::new();
    let mut base = path;
    while !base.exists() {
        let name = base.file_name().context("неверный путь к папке игры")?;
        missing.push(name.to_os_string());
        base = base.parent().context("неверный путь к папке игры")?;
    }
    let mut canonical = base.canonicalize()?;
    for part in missing.into_iter().rev() {
        canonical.push(part);
    }
    if canonical.parent().is_none() || canonical.parent().and_then(Path::parent).is_none() {
        bail!("выберите отдельную подпапку для игры")
    }
    if dirs::home_dir().is_some_and(|h| h == canonical) {
        bail!("домашняя папка не подходит для установки игры")
    }
    Ok(canonical)
}

pub fn join(root: &Path, rel: &Path) -> Result<PathBuf> {
    if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        bail!("unsafe relative path")
    }
    let mut p = root.to_path_buf();
    for part in rel.components() {
        p.push(part.as_os_str());
        if let Ok(m) = fs::symlink_metadata(&p) {
            if m.file_type().is_symlink() {
                bail!("symbolic link inside game path: {}", p.display())
            }
        }
    }
    Ok(p)
}

pub fn remove(root: &Path, rel: &Path) -> Result<()> {
    let path = join(root, rel)?;
    if !path.starts_with(root) {
        bail!("outside game path")
    }
    if let Ok(m) = fs::symlink_metadata(&path) {
        if m.is_dir() {
            check_tree_for_links(&path)?;
            fs::remove_dir_all(&path)?
        } else {
            fs::remove_file(&path)?
        }
    }
    Ok(())
}
fn check_tree_for_links(path: &Path) -> Result<()> {
    for e in fs::read_dir(path)? {
        let e = e?;
        let meta = fs::symlink_metadata(e.path())?;
        if meta.file_type().is_symlink() {
            bail!("symbolic link in removal tree: {}", e.path().display())
        }
        if meta.is_dir() {
            check_tree_for_links(&e.path())?
        }
    }
    Ok(())
}
pub fn remove_file(root: &Path, rel: &Path) -> Result<()> {
    let path = join(root, rel)?;
    if let Ok(m) = fs::symlink_metadata(&path) {
        if !m.is_file() {
            bail!("expected file: {}", path.display())
        }
        fs::remove_file(path)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_traversal() {
        for p in [
            "../x", "a/../b", "C:/x", "/etc/x", "a\\..\\b", "CON.txt", "a//b",
        ] {
            assert!(relative(p).is_err(), "{p}")
        }
    }
    #[test]
    fn blocks_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let root = root(tmp.path()).unwrap();
        assert!(join(&root, &relative("safe/file").unwrap()).is_ok());
    }
    #[test]
    fn planning_does_not_create_root() {
        let tmp = tempfile::tempdir().unwrap();
        let child = tmp.path().join("new-game");
        assert!(plan_root(&child).is_ok());
        assert!(!child.exists());
    }
    #[test]
    fn root_checks_write_access_without_leaving_probe() {
        let tmp = tempfile::tempdir().unwrap();
        let game = tmp.path().join("game");
        assert_eq!(root(&game).unwrap(), game.canonicalize().unwrap());
        assert!(fs::read_dir(&game).unwrap().next().is_none());
    }
}

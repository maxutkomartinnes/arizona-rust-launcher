use crate::{
    install,
    model::{GameId, Manifest, ModCatalog, Node},
    net::Net,
    plan::{self, Options},
    safety,
    settings::{InstalledMod, Settings},
};
use anyhow::{bail, Context, Result};
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};

fn listed(manifest: &Manifest) -> Result<Vec<PathBuf>> {
    fn walk(nodes: &[Node], parent: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
        for n in nodes {
            let rel = parent.join(safety::relative(n.install_name())?);
            match n {
                Node::Dir { data, .. } => walk(data, &rel, out)?,
                Node::File(_) | Node::Check(_) | Node::Res(_) => out.push(rel),
                Node::Delete(_) => {}
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(&manifest.data, Path::new(""), &mut out)?;
    Ok(out)
}
fn backup_root(game: GameId, id: u64) -> Result<PathBuf> {
    Ok(Settings::path()?
        .parent()
        .unwrap()
        .join("backups")
        .join(game.id())
        .join(id.to_string()))
}
pub fn install_mod<F: FnMut(String)>(
    net: &Net,
    game: GameId,
    id: u64,
    catalog: &ModCatalog,
    root: &Path,
    settings: &mut Settings,
    cancel: &AtomicBool,
    mut report: F,
) -> Result<()> {
    let root = safety::root(root)?;
    let mut done = HashSet::new();
    let mut visiting = HashSet::new();
    fn install_recursive<F: FnMut(String)>(
        net: &Net,
        game: GameId,
        id: u64,
        catalog: &ModCatalog,
        root: &Path,
        settings: &mut Settings,
        cancel: &AtomicBool,
        report: &mut F,
        done: &mut HashSet<u64>,
        visiting: &mut HashSet<u64>,
    ) -> Result<()> {
        if done.contains(&id) {
            return Ok(());
        }
        if !visiting.insert(id) {
            bail!("mod dependency cycle")
        }
        let entry = catalog
            .mods
            .iter()
            .chain(&catalog.top_of_month)
            .find(|m| m.id == id)
            .context("mod not found in catalog")?;
        if entry.visible == Some(false) {
            bail!("mod is hidden")
        }
        for dep in &entry.dependency_mods_ids {
            install_recursive(
                net, game, *dep, catalog, root, settings, cancel, report, done, visiting,
            )?
        }
        visiting.remove(&id);
        if settings
            .installed_mods
            .get(game.id())
            .is_some_and(|v| v.iter().any(|m| m.id == id && m.version == entry.version))
        {
            done.insert(id);
            return Ok(());
        }
        if settings
            .installed_mods
            .get(game.id())
            .is_some_and(|v| v.iter().any(|m| m.id == id))
        {
            uninstall_mod(game, id, root, settings)?;
        }
        let m = net.mod_manifest(game, id)?;
        let files = listed(&m)?;
        let owned: HashSet<_> = settings
            .installed_mods
            .get(game.id())
            .into_iter()
            .flatten()
            .filter(|m| m.id != id)
            .flat_map(|m| m.files.iter())
            .collect();
        for file in &files {
            if owned.contains(&file.to_string_lossy().to_string()) {
                bail!("mod conflicts with installed mod at {}", file.display())
            }
        }
        let plan = plan::build(&m, root, Options::default())?;
        let backup = backup_root(game, id)?;
        fs::create_dir_all(&backup)?;
        let mut backups = Vec::new();
        for f in &files {
            let source = safety::join(root, f)?;
            if source.is_file() {
                let to = safety::join(&backup, f)?;
                fs::create_dir_all(to.parent().unwrap())?;
                fs::copy(&source, &to)?;
                backups.push(f.to_string_lossy().to_string());
            }
        }
        let cdns = game.mod_cdns();
        if let Err(e) = install::apply(
            net,
            root,
            &plan,
            cdns,
            &id.to_string(),
            cancel,
            false,
            |done, total, file| report(format!("Mod {id}: {file} {done}/{total}")),
        ) {
            for f in &files {
                let _ = safety::remove_file(root, f);
            }
            for rel in &backups {
                let rel = safety::relative(rel)?;
                let from = safety::join(&backup, &rel)?;
                let to = safety::join(root, &rel)?;
                if from.is_file() {
                    let _ = fs::copy(from, to);
                }
            }
            return Err(e);
        }
        let rec = InstalledMod {
            id,
            version: entry.version.clone(),
            files: files
                .iter()
                .map(|p| p.to_string_lossy().to_string())
                .collect(),
            backups,
        };
        let installed = settings.installed_mods.entry(game.id().into()).or_default();
        installed.retain(|m| m.id != id);
        installed.push(rec);
        settings.save()?;
        done.insert(id);
        Ok(())
    }
    install_recursive(
        net,
        game,
        id,
        catalog,
        &root,
        settings,
        cancel,
        &mut report,
        &mut done,
        &mut visiting,
    )
}
pub fn uninstall_mod(game: GameId, id: u64, root: &Path, settings: &mut Settings) -> Result<()> {
    let root = safety::root(root)?;
    let mods = settings.installed_mods.entry(game.id().into()).or_default();
    let rec = mods
        .iter()
        .find(|m| m.id == id)
        .cloned()
        .context("mod is not installed")?;
    let backup = backup_root(game, id)?;
    for rel in &rec.files {
        let rel = safety::relative(rel)?;
        safety::remove_file(&root, &rel)?;
    }
    for rel in &rec.backups {
        let rel = safety::relative(rel)?;
        let from = safety::join(&backup, &rel)?;
        let to = safety::join(&root, &rel)?;
        fs::create_dir_all(to.parent().unwrap())?;
        fs::rename(from, to)?;
    }
    mods.retain(|m| m.id != id);
    settings.save()?;
    Ok(())
}

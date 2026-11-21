use crate::{
    model::{Manifest, Node},
    safety,
};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

#[derive(Clone, Copy, Default)]
pub struct Options {
    pub deep: bool,
    pub repair: bool,
    pub future_graphics: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Download {
    pub source: PathBuf,
    pub target: PathBuf,
    pub size: u64,
    pub hash: String,
    pub mtime: Option<i64>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Plan {
    pub mkdir: Vec<PathBuf>,
    pub download: Vec<Download>,
    pub delete: Vec<PathBuf>,
    pub touch: Vec<(PathBuf, i64)>,
    pub total_bytes: u64,
}

fn hash(path: &Path) -> Result<String> {
    let mut f = File::open(path)?;
    let mut ctx = md5::Context::new();
    let mut buf = [0u8; 1024 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        ctx.consume(&buf[..n]);
    }
    Ok(format!("{:x}", ctx.compute()))
}
fn mtime(path: &Path) -> Result<i64> {
    Ok(fs::metadata(path)?
        .modified()?
        .duration_since(UNIX_EPOCH)?
        .as_secs() as i64)
}
fn collect_extra(
    root: &Path,
    rel: &Path,
    wanted: &HashSet<PathBuf>,
    out: &mut Vec<PathBuf>,
) -> Result<()> {
    let dir = safety::join(root, rel)?;
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(&dir)? {
        let entry = entry?;
        let child = rel.join(entry.file_name());
        let meta = fs::symlink_metadata(entry.path())?;
        if meta.file_type().is_symlink() {
            bail!(
                "symbolic link in game directory: {}",
                entry.path().display()
            )
        }
        if !wanted.contains(&child) {
            out.push(child);
        } else if meta.is_dir() {
            collect_extra(root, &child, wanted, out)?;
        }
    }
    Ok(())
}
fn add_node(
    root: &Path,
    node: &Node,
    source_parent: &Path,
    target_parent: &Path,
    opts: Options,
    plan: &mut Plan,
    wanted: &mut HashSet<PathBuf>,
    pure_dirs: &mut Vec<PathBuf>,
) -> Result<()> {
    if let Some(v) = node.variant() {
        if v != if opts.future_graphics {
            "future"
        } else {
            "default"
        } {
            return Ok(());
        }
    }
    let src = source_parent.join(safety::relative(node.name())?);
    let rel = target_parent.join(safety::relative(node.install_name())?);
    let path = safety::join(root, &rel)?;
    wanted.insert(rel.clone());
    match node {
        Node::Dir { data, pure, .. } => {
            if !path.is_dir() {
                plan.mkdir.push(rel.clone())
            }
            if *pure {
                pure_dirs.push(rel.clone())
            }
            for child in data {
                add_node(root, child, &src, &rel, opts, plan, wanted, pure_dirs)?
            }
        }
        Node::Delete(f) => {
            if path.exists()
                && f.date_change
                    .is_some_and(|t| mtime(&path).unwrap_or(i64::MAX) < t)
            {
                plan.delete.push(rel)
            }
        }
        Node::File(f) | Node::Check(f) | Node::Res(f) => {
            if path.is_dir() {
                bail!("manifest file collides with directory: {}", path.display())
            }
            if matches!(node, Node::Res(_)) && path.exists() && !opts.repair {
                return Ok(());
            }
            let expected = f
                .hash
                .as_ref()
                .context("manifest file lacks MD5")?
                .to_ascii_lowercase();
            if expected.len() != 32 || !expected.bytes().all(|c| c.is_ascii_hexdigit()) {
                bail!("invalid MD5 in manifest")
            }
            let exists = path.is_file();
            let size_ok = exists && fs::metadata(&path)?.len() == f.size;
            let actual_mtime = if exists { Some(mtime(&path)?) } else { None };
            let hash_required = opts.deep
                || opts.repair
                || matches!(node, Node::Check(_))
                || (size_ok
                    && f.date_change
                        .is_some_and(|t| actual_mtime.is_some_and(|m| m < t)));
            let hash_ok = if size_ok && hash_required {
                hash(&path)? == expected
            } else {
                size_ok
            };
            if !hash_ok {
                plan.total_bytes += f.size;
                plan.download.push(Download {
                    source: src,
                    target: rel,
                    size: f.size,
                    hash: expected,
                    mtime: f.date_change,
                });
            } else if let (Some(want), Some(have)) = (f.date_change, actual_mtime) {
                if have != want {
                    plan.touch.push((rel, want))
                }
            }
        }
    }
    Ok(())
}
pub fn build(manifest: &Manifest, root: &Path, opts: Options) -> Result<Plan> {
    let root = safety::plan_root(root)?;
    let mut plan = Plan::default();
    let mut wanted = HashSet::new();
    let mut pure_dirs = Vec::new();
    for node in &manifest.data {
        add_node(
            &root,
            node,
            Path::new(""),
            Path::new(""),
            opts,
            &mut plan,
            &mut wanted,
            &mut pure_dirs,
        )?
    }
    if opts.repair {
        collect_extra(&root, Path::new(""), &wanted, &mut plan.delete)?
    } else {
        for dir in pure_dirs {
            collect_extra(&root, &dir, &wanted, &mut plan.delete)?
        }
    }
    plan.delete.sort();
    plan.delete.dedup();
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixture_and_variants() {
        let m: Manifest =
            serde_json::from_str(include_str!("../fixtures/arizona_game.json")).unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let p = build(&m, tmp.path(), Options::default()).unwrap();
        assert!(p.download.len() > 1000);
        assert!(p.total_bytes > 1_000_000_000);
        let f = build(
            &m,
            tmp.path(),
            Options {
                future_graphics: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_ne!(
            p.download.iter().map(|x| &x.source).collect::<Vec<_>>(),
            f.download.iter().map(|x| &x.source).collect::<Vec<_>>()
        );
    }
    #[test]
    fn install_as_and_delete_preview() {
        let m:Manifest=serde_json::from_str(r#"{"data":[{"type":"file","name":"source.txt","install_as":"dest.txt","size":3,"hash":"900150983cd24fb0d6963f7d28e17f72"}]}"#).unwrap();
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("extra"), "bad").unwrap();
        let p = build(
            &m,
            tmp.path(),
            Options {
                repair: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(p.download[0].source, PathBuf::from("source.txt"));
        assert_eq!(p.download[0].target, PathBuf::from("dest.txt"));
        assert_eq!(p.delete, vec![PathBuf::from("extra")]);
    }
    #[test]
    fn deep_repair_detects_corrupt_same_size_file() {
        let m:Manifest=serde_json::from_str(r#"{"data":[{"type":"file","name":"x.bin","size":3,"date_change":1700000000,"hash":"900150983cd24fb0d6963f7d28e17f72"}]}"#).unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("x.bin");
        std::fs::write(&file, b"bad").unwrap();
        let p = build(
            &m,
            tmp.path(),
            Options {
                deep: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(p.download.len(), 1);
        std::fs::write(&file, b"abc").unwrap();
        let p = build(
            &m,
            tmp.path(),
            Options {
                deep: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(p.download.is_empty());
        assert_eq!(p.touch.len(), 1);
    }
}

use crate::{
    net::{file_url, Net},
    plan::{Download, Plan},
    safety,
};
use anyhow::{bail, Context, Result};
use std::{
    fs::{self, File, FileTimes},
    io::{Read, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, SystemTime},
};

fn set_mtime(path: &Path, seconds: i64) -> Result<()> {
    if seconds < 0 {
        bail!("negative manifest timestamp")
    };
    let at = SystemTime::UNIX_EPOCH + Duration::from_secs(seconds as u64);
    File::options()
        .write(true)
        .open(path)?
        .set_times(FileTimes::new().set_modified(at))?;
    Ok(())
}

pub fn apply<F: FnMut(u64, u64, &str)>(
    net: &Net,
    root: &Path,
    plan: &Plan,
    cdns: (&str, &str),
    source_prefix: &str,
    cancel: &AtomicBool,
    allow_delete: bool,
    mut progress: F,
) -> Result<()> {
    let root = safety::root(root)?;
    if !plan.delete.is_empty() && !allow_delete {
        bail!("deletion requires explicit confirmation")
    }
    let free = fs2::available_space(&root).with_context(|| {
        format!(
            "Не удалось проверить свободное место на диске для {}",
            root.display()
        )
    })?;
    if free < plan.total_bytes.saturating_add(256 * 1024 * 1024) {
        bail!("insufficient free disk space")
    }
    for d in &plan.mkdir {
        let path = safety::join(&root, d)?;
        fs::create_dir_all(&path)
            .with_context(|| format!("Не удалось создать папку {}", path.display()))?;
    }
    let mut done = 0;
    for item in &plan.download {
        if cancel.load(Ordering::Relaxed) {
            bail!("cancelled")
        }
        install_one(net, &root, item, cdns, source_prefix, cancel, |n| {
            progress(done + n, plan.total_bytes, &item.target.to_string_lossy())
        })?;
        done += item.size;
        progress(done, plan.total_bytes, &item.target.to_string_lossy());
    }
    for (rel, time) in &plan.touch {
        let p = safety::join(&root, rel)?;
        set_mtime(&p, *time)?;
    }
    if allow_delete {
        for rel in &plan.delete {
            if cancel.load(Ordering::Relaxed) {
                bail!("cancelled")
            };
            safety::remove(&root, rel)?;
        }
    }
    Ok(())
}
fn install_one<F: FnMut(u64)>(
    net: &Net,
    root: &Path,
    item: &Download,
    cdns: (&str, &str),
    source_prefix: &str,
    cancel: &AtomicBool,
    mut progress: F,
) -> Result<()> {
    let dest = safety::join(root, &item.target)?;
    let parent = dest.parent().context("target has no parent")?;
    fs::create_dir_all(parent)
        .with_context(|| format!("Не удалось создать папку {}", parent.display()))?;
    let staged = dest.with_extension(format!("{}.arizona-part", std::process::id()));
    let backup = dest.with_extension(format!("{}.arizona-old", std::process::id()));
    let mut errors = Vec::new();
    for base in [cdns.0, cdns.1] {
        if cancel.load(Ordering::Relaxed) {
            bail!("cancelled")
        }
        let url = file_url(base, source_prefix, &item.source)?;
        let result = (|| -> Result<()> {
            let mut response = net.get_file(&url)?;
            let mut out = File::create(&staged)
                .with_context(|| format!("Нет доступа для записи файла {}", staged.display()))?;
            let mut ctx = md5::Context::new();
            let mut bytes = 0u64;
            let mut buf = [0u8; 1024 * 1024];
            loop {
                if cancel.load(Ordering::Relaxed) {
                    bail!("cancelled")
                };
                let n = response.read(&mut buf)?;
                if n == 0 {
                    break;
                };
                out.write_all(&buf[..n])
                    .with_context(|| format!("Не удалось записать файл {}", staged.display()))?;
                ctx.consume(&buf[..n]);
                bytes += n as u64;
                progress(bytes);
                if bytes > item.size {
                    bail!("download exceeds declared size")
                }
            }
            out.flush()
                .with_context(|| format!("Не удалось завершить запись {}", staged.display()))?;
            out.sync_all()
                .with_context(|| format!("Не удалось сохранить файл {}", staged.display()))?;
            drop(out);
            if bytes != item.size {
                bail!("download size mismatch: {bytes} vs {}", item.size)
            }
            if format!("{:x}", ctx.compute()) != item.hash {
                bail!("download MD5 mismatch")
            }
            if backup.exists() {
                fs::remove_file(&backup).with_context(|| {
                    format!("Не удалось удалить резервную копию {}", backup.display())
                })?
            }
            if dest.exists() {
                fs::rename(&dest, &backup)
                    .with_context(|| format!("Не удалось заменить файл {}", dest.display()))?
            }
            if let Err(e) = fs::rename(&staged, &dest) {
                if backup.exists() {
                    let _ = fs::rename(&backup, &dest);
                }
                return Err(e)
                    .with_context(|| format!("Не удалось поместить файл в {}", dest.display()));
            }
            if let Some(t) = item.mtime {
                set_mtime(&dest, t)?
            }
            if backup.exists() {
                fs::remove_file(&backup).with_context(|| {
                    format!("Не удалось удалить резервную копию {}", backup.display())
                })?
            }
            Ok(())
        })();
        if result.is_ok() {
            return Ok(());
        }
        let _ = fs::remove_file(&staged);
        let error = result.unwrap_err();
        if error
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::PermissionDenied)
        {
            return Err(error).context("Доступ к файлам игры запрещён. Закройте игру или выберите папку с правом записи вне Program Files");
        }
        errors.push(format!("{error:#}"));
    }
    bail!("download failed from both CDNs: {}", errors.join(" | "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    #[test]
    fn cancel_before_download() {
        let tmp = tempfile::tempdir().unwrap();
        let p = Plan {
            download: vec![Download {
                source: PathBuf::from("x"),
                target: PathBuf::from("x"),
                size: 3,
                hash: "900150983cd24fb0d6963f7d28e17f72".into(),
                mtime: None,
            }],
            total_bytes: 3,
            ..Default::default()
        };
        let net = Net::new().unwrap();
        let flag = AtomicBool::new(true);
        assert!(apply(
            &net,
            tmp.path(),
            &p,
            ("https://example.org", "https://example.org"),
            "game",
            &flag,
            false,
            |_, _, _| {}
        )
        .is_err());
        assert!(!tmp.path().join("x").exists());
    }
}

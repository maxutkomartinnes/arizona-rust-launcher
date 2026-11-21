use crate::{model::GameId, net::Net, settings::Settings};
use anyhow::{bail, Context, Result};
use image::{ImageReader, Limits};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};
use url::Url;

const BASES: [&str; 2] = ["https://pc.rod-ins.com", "https://rod-pc.react-group.tech"];
const MAX_ASSET: u64 = 30 * 1024 * 1024;
fn image_limits() -> Limits {
    let mut limits = Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(128 * 1024 * 1024);
    limits
}

#[derive(Debug, Deserialize)]
struct ThemeDocument {
    projects: HashMap<String, ThemeProject>,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThemeProject {
    bg: Option<String>,
    character: Option<String>,
    logo: Option<String>,
    loader_bg: Option<String>,
    loader_logo: Option<String>,
}
impl ThemeProject {
    fn assets(&self) -> [(&'static str, Option<&str>); 5] {
        [
            ("bg", self.bg.as_deref()),
            ("character", self.character.as_deref()),
            ("logo", self.logo.as_deref()),
            ("loader-bg", self.loader_bg.as_deref()),
            ("loader-logo", self.loader_logo.as_deref()),
        ]
    }
}
#[derive(Debug, Default, Serialize, Deserialize)]
struct CacheIndex {
    entries: HashMap<String, String>,
}

fn theme_key(game: GameId) -> &'static str {
    if game.id().starts_with("rodina") {
        "rodina"
    } else {
        "arizona"
    }
}
fn asset_urls(raw: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for base in BASES {
        let u = Url::parse(base)?.join(raw)?;
        if u.scheme() != "https"
            || !BASES.iter().any(|b| {
                Url::parse(b)
                    .ok()
                    .is_some_and(|v| v.host_str() == u.host_str())
            })
        {
            bail!("theme asset host not allowed")
        }
        out.push(u.to_string());
    }
    Ok(out)
}
fn cache_dir() -> Result<PathBuf> {
    Ok(Settings::path()?
        .parent()
        .context("settings parent")?
        .join("theme-assets"))
}
fn save_index(path: &Path, index: &CacheIndex) -> Result<()> {
    let staged = path.with_extension("json.part");
    fs::write(&staged, serde_json::to_vec_pretty(index)?)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(staged, path)?;
    Ok(())
}

/// Downloads the current theme into an isolated cache. Existing assets are reused
/// only when their source URL still matches the live manifest.
pub fn sync(net: &Net, game: GameId) -> Result<HashMap<String, PathBuf>> {
    let manifest_urls: Vec<String> = BASES
        .iter()
        .map(|b| format!("{b}/resource/pc-launcher/theme/theme.json"))
        .collect();
    let doc: ThemeDocument = net.get_json(&manifest_urls)?;
    let project = doc
        .projects
        .get(theme_key(game))
        .context("theme project missing")?;
    let dir = cache_dir()?;
    fs::create_dir_all(&dir)?;
    let index_path = dir.join("manifest.json");
    let mut index: CacheIndex = fs::read(&index_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default();
    let mut files = HashMap::new();
    for (name, raw) in project.assets() {
        let Some(raw) = raw.filter(|v| !v.is_empty()) else {
            continue;
        };
        let key = format!("{}-{name}", theme_key(game));
        let urls = asset_urls(raw)?;
        let dest = dir.join(format!("{key}.img"));
        if dest.exists() && index.entries.get(&key) == Some(&raw.to_owned()) {
            files.insert(name.to_owned(), dest);
            continue;
        }
        let mut last_error = String::new();
        let mut fetched = None;
        for url in urls {
            let attempt = (|| -> Result<Vec<u8>> {
                let response = net.get_file(&url)?;
                let content_type = response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if !content_type.starts_with("image/") {
                    bail!("unexpected theme content type: {content_type}")
                }
                if response.content_length().is_some_and(|len| len > MAX_ASSET) {
                    bail!("theme asset exceeds 30 MiB")
                }
                let mut bytes = Vec::new();
                response.take(MAX_ASSET + 1).read_to_end(&mut bytes)?;
                if bytes.len() as u64 > MAX_ASSET {
                    bail!("theme asset exceeds 30 MiB")
                }
                let mut reader = ImageReader::new(Cursor::new(&bytes)).with_guessed_format()?;
                reader.limits(image_limits());
                reader.decode().context("invalid theme image")?;
                Ok(bytes)
            })();
            match attempt {
                Ok(bytes) => {
                    fetched = Some(bytes);
                    break;
                }
                Err(e) => last_error = e.to_string(),
            }
        }
        if let Some(bytes) = fetched {
            let staged = dest.with_extension("img.part");
            fs::write(&staged, bytes)?;
            if dest.exists() {
                fs::remove_file(&dest)?;
            }
            fs::rename(staged, &dest)?;
            index.entries.insert(key, raw.to_owned());
            files.insert(name.to_owned(), dest);
        } else if dest.exists() {
            files.insert(name.to_owned(), dest);
        } else {
            bail!("theme {name}: {last_error}")
        }
    }
    save_index(&index_path, &index)?;
    Ok(files)
}

pub fn rgba(path: &Path) -> Result<(Vec<u8>, [usize; 2])> {
    let mut reader = ImageReader::open(path)?.with_guessed_format()?;
    reader.limits(image_limits());
    let image = reader.decode()?.to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    Ok((image.into_raw(), size))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_external_theme_host() {
        assert!(asset_urls("https://evil.example/image.png").is_err());
        assert_eq!(asset_urls("/resource/theme/a.png").unwrap().len(), 2);
    }
}

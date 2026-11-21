use crate::model::{GameId, Manifest, ModCatalog, Notification, Project, Servers};
use anyhow::{bail, Context, Result};
use reqwest::blocking::Client;
use std::{path::Path, time::Duration};
use url::Url;

pub struct Net {
    api: Client,
    download: Client,
}
impl Net {
    pub fn new() -> Result<Self> {
        Ok(Self {
            api: Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .timeout(Duration::from_secs(15))
                .user_agent("ArizonaRustLauncher/0.1")
                .build()?,
            download: Client::builder()
                .connect_timeout(Duration::from_secs(15))
                .timeout(Duration::from_secs(3600))
                .user_agent("ArizonaRustLauncher/0.1")
                .build()?,
        })
    }
    pub fn get_json<T: serde::de::DeserializeOwned>(&self, urls: &[String]) -> Result<T> {
        let mut errors = Vec::new();
        for url in urls {
            let result = (|| -> Result<T> {
                let r = self.api.get(url).send()?.error_for_status()?;
                if r.url().scheme() != "https" {
                    bail!("insecure API redirect")
                }
                Ok(r.json()?)
            })();
            match result {
                Ok(v) => return Ok(v),
                Err(e) => errors.push(format!("{url}: {e}")),
            }
        }
        bail!("all endpoints failed: {}", errors.join(" | "))
    }
    pub fn projects(&self) -> Result<Vec<Project>> {
        self.get_json(&[
            "https://api.arizona-five.com/launcher/projects".into(),
            "https://arz-api-reserve.s-amp.com/launcher/projects".into(),
        ])
    }
    pub fn servers(&self) -> Result<Servers> {
        self.get_json(&[
            "https://api.arizona-five.com/launcher/servers".into(),
            "https://arz-api-reserve.s-amp.com/launcher/servers".into(),
        ])
    }
    pub fn manifest(&self, g: GameId) -> Result<Manifest> {
        let (a, b) = g.cdns();
        self.get_json(&[format!("{a}/game.json"), format!("{b}/game.json")])
    }
    pub fn mod_catalog(&self, g: GameId) -> Result<ModCatalog> {
        let id = g.id();
        self.get_json(&[
            format!("https://api-samp.arizona-five.com/launcher/mods/{id}"),
            format!("https://arz-api-samp-reserve.s-amp.com/launcher/mods/{id}"),
        ])
    }
    pub fn mod_manifest(&self, g: GameId, id: u64) -> Result<Manifest> {
        let g = g.id();
        self.get_json(&[
            format!("https://api-samp.arizona-five.com/launcher/mods/{g}/{id}"),
            format!("https://arz-api-samp-reserve.s-amp.com/launcher/mods/{g}/{id}"),
        ])
    }
    pub fn notifications(&self, fingerprint: Option<&str>) -> Result<Vec<Notification>> {
        let path = if let Some(id) = fingerprint {
            format!("notifications/own/{id}?platform=pc")
        } else {
            "notifications?platform=pc".into()
        };
        self.get_json(&[
            format!("https://api.arizona-five.com/{path}"),
            format!("https://arz-api-reserve.s-amp.com/{path}"),
        ])
    }
    pub fn cdn_indexes(&self) -> String {
        #[derive(serde::Deserialize)]
        struct Ping {
            hash: String,
        }
        let groups: [&[&str]; 3] = [
            &[
                "https://cdn.azresources.cloud",
                "https://resources.azgames-cdn.ru",
            ],
            &["https://cdn.azsounds.net", "https://azgamesound-cdn.ru"],
            &[
                "https://server-api.arizona.games",
                "https://server-api.azgames-cdn.ru",
            ],
        ];
        let client = match Client::builder()
            .timeout(Duration::from_secs(5))
            .user_agent("Arizona PC/0.0.144")
            .build()
        {
            Ok(c) => c,
            Err(_) => return "0,0,0".into(),
        };
        let mut indexes = Vec::new();
        for urls in groups {
            let mut selected = 0;
            for (i, base) in urls.iter().enumerate() {
                let result = client
                    .get(format!("{base}/ping.json"))
                    .send()
                    .and_then(|r| r.error_for_status())
                    .and_then(|r| r.json::<Ping>());
                if result.is_ok_and(|p| p.hash == "5aa4731d5d84e09e2f7e7141e560104f") {
                    selected = i;
                    break;
                }
            }
            indexes.push(selected.to_string());
        }
        indexes.join(",")
    }
    pub fn get_file(&self, url: &str) -> Result<reqwest::blocking::Response> {
        if !url.starts_with("https://") {
            bail!("non-HTTPS download")
        }
        let r = self
            .download
            .get(url)
            .send()?
            .error_for_status()
            .context("file request failed")?;
        if r.url().scheme() != "https" {
            bail!("insecure download redirect")
        }
        Ok(r)
    }
}
pub fn file_url(base: &str, kind: &str, path: &Path) -> Result<String> {
    let mut url = Url::parse(base)?;
    if url.scheme() != "https" {
        bail!("non-HTTPS CDN")
    }
    {
        let mut seg = url
            .path_segments_mut()
            .map_err(|_| anyhow::anyhow!("invalid CDN URL"))?;
        seg.pop_if_empty();
        seg.push(kind);
        for c in path.components() {
            let s = c
                .as_os_str()
                .to_str()
                .context("non-Unicode manifest path")?;
            seg.push(s);
        }
    }
    Ok(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshots() {
        let _: Vec<Project> =
            serde_json::from_str(include_str!("../fixtures/projects.json")).unwrap();
        let _: Servers = serde_json::from_str(include_str!("../fixtures/servers.json")).unwrap();
        let _: ModCatalog = serde_json::from_str(include_str!("../fixtures/mods.json")).unwrap();
        let _: Manifest =
            serde_json::from_str(include_str!("../fixtures/mod_manifest.json")).unwrap();
    }
    #[test]
    fn encodes_url() {
        assert!(file_url(
            "https://example.org/release",
            "game",
            Path::new("x/a b.txt")
        )
        .unwrap()
        .ends_with("/release/game/x/a%20b.txt"));
    }
}

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Manifest {
    pub data: Vec<Node>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Node {
    Dir {
        name: String,
        #[serde(default)]
        data: Vec<Node>,
        #[serde(default)]
        pure: bool,
        #[serde(default)]
        variant_tag: Option<String>,
        #[serde(default)]
        install_as: Option<String>,
    },
    File(FileNode),
    Check(FileNode),
    Res(FileNode),
    Delete(FileNode),
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct FileNode {
    pub name: String,
    #[serde(default)]
    pub install_as: Option<String>,
    #[serde(default)]
    pub variant_tag: Option<String>,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub date_change: Option<i64>,
    #[serde(default)]
    pub hash: Option<String>,
}
impl Node {
    pub fn name(&self) -> &str {
        match self {
            Self::Dir { name, .. } => name,
            Self::File(f) | Self::Check(f) | Self::Res(f) | Self::Delete(f) => &f.name,
        }
    }
    pub fn install_name(&self) -> &str {
        match self {
            Self::Dir {
                name, install_as, ..
            } => install_as.as_deref().unwrap_or(name),
            Self::File(f) | Self::Check(f) | Self::Res(f) | Self::Delete(f) => {
                f.install_as.as_deref().unwrap_or(&f.name)
            }
        }
    }
    pub fn variant(&self) -> Option<&str> {
        match self {
            Self::Dir { variant_tag, .. } => variant_tag.as_deref(),
            Self::File(f) | Self::Check(f) | Self::Res(f) | Self::Delete(f) => {
                f.variant_tag.as_deref()
            }
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub game_id: String,
    #[serde(default)]
    pub news: Vec<News>,
    #[serde(default)]
    pub trilogy_available: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct News {
    pub title: String,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub date: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub image_url: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Server {
    pub number: u32,
    pub name: String,
    pub ip: String,
    pub port: u16,
    #[serde(default)]
    pub online: u32,
    #[serde(default)]
    pub queue: u32,
    #[serde(default)]
    pub maxplayers: u32,
    #[serde(default)]
    pub additional_ips: Vec<String>,
}
pub type Servers = HashMap<String, Vec<Server>>;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Notification {
    pub id: u64,
    #[serde(default)]
    pub project: String,
    pub title: String,
    #[serde(default)]
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModCatalog {
    #[serde(default)]
    pub top_of_month: Vec<ModEntry>,
    #[serde(default)]
    pub mods: Vec<ModEntry>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModEntry {
    pub id: u64,
    pub title: String,
    #[serde(default)]
    pub author: String,
    #[serde(default, deserialize_with = "version_as_string")]
    pub version: String,
    #[serde(default)]
    pub short_description: String,
    #[serde(default)]
    pub dependency_mods_ids: Vec<u64>,
    #[serde(default)]
    pub visible: Option<bool>,
}
fn version_as_string<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let v = serde_json::Value::deserialize(d)?;
    Ok(match v {
        serde_json::Value::String(s) => s,
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameId {
    Arizona,
    ArizonaStaging,
    Rodina,
    RodinaStaging,
    ArizonaV,
    ArizonaVStaging,
    Village,
    VillageStaging,
    Trilogy,
}
impl GameId {
    pub const ALL: [Self; 9] = [
        Self::Arizona,
        Self::ArizonaStaging,
        Self::Rodina,
        Self::RodinaStaging,
        Self::ArizonaV,
        Self::ArizonaVStaging,
        Self::Village,
        Self::VillageStaging,
        Self::Trilogy,
    ];
    pub fn id(self) -> &'static str {
        match self {
            Self::Arizona => "arizona",
            Self::ArizonaStaging => "arizona_staging",
            Self::Rodina => "rodina",
            Self::RodinaStaging => "rodina_staging",
            Self::ArizonaV => "arizonav",
            Self::ArizonaVStaging => "arizonav_staging",
            Self::Village => "village",
            Self::VillageStaging => "village_staging",
            Self::Trilogy => "trilogy",
        }
    }
    pub fn from_id(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|g| g.id() == s)
    }
    pub fn cdns(self) -> (&'static str, &'static str) {
        match self {
            Self::Arizona => (
                "https://pc.az-ins.com/release",
                "https://arz-pc.react-group.tech/release",
            ),
            Self::ArizonaStaging => (
                "https://pc.az-ins.com/staging",
                "https://arz-pc.react-group.tech/staging",
            ),
            Self::Rodina => (
                "https://pc.rod-ins.com/release",
                "https://rod-pc.react-group.tech/release",
            ),
            Self::ArizonaV => (
                "https://pc.rod-ins.com/arizonav",
                "https://rod-pc.react-group.tech/arizonav",
            ),
            Self::Village => (
                "https://pc.az-ins.com/village",
                "https://arz-pc.react-group.tech/village",
            ),
            Self::RodinaStaging => (
                "https://arizona-recovery.react.domains/desktop/game/rodina_staging",
                "https://arizona-recovery.react-group.tech/desktop/game/rodina_staging",
            ),
            Self::ArizonaVStaging => (
                "https://arizona-recovery.react.domains/desktop/game/arizonav_staging",
                "https://arizona-recovery.react-group.tech/desktop/game/arizonav_staging",
            ),
            Self::VillageStaging => (
                "https://arizona-recovery.react.domains/desktop/game/village_staging",
                "https://arizona-recovery.react-group.tech/desktop/game/village_staging",
            ),
            Self::Trilogy => (
                "https://mob.azinternal.com/trilogy",
                "https://rod-mob.react-group.tech/trilogy",
            ),
        }
    }
    pub fn mod_cdns(self) -> (&'static str, &'static str) {
        match self {
            Self::Arizona | Self::ArizonaStaging => (
                "https://pc.az-ins.com/resource/mods",
                "https://arz-pc.react-group.tech/resource/mods",
            ),
            Self::Rodina => (
                "https://pc.rod-ins.com/resource/mods",
                "https://rod-pc.react-group.tech/resource/mods",
            ),
            Self::ArizonaV => (
                "https://pc.rod-ins.com/arizonav/mods",
                "https://rod-pc.react-group.tech/arizonav/mods",
            ),
            Self::Village => (
                "https://pc.az-ins.com/village/mods",
                "https://arz-pc.react-group.tech/village/mods",
            ),
            Self::RodinaStaging => (
                "https://arizona-recovery.react.domains/desktop/mods/rodina_staging",
                "https://arizona-recovery.react.domains/desktop/mods/rodina_staging",
            ),
            Self::ArizonaVStaging => (
                "https://arizona-recovery.react.domains/desktop/mods/arizonav_staging",
                "https://arizona-recovery.react.domains/desktop/mods/arizonav_staging",
            ),
            Self::VillageStaging => (
                "https://arizona-recovery.react.domains/desktop/mods/village_staging",
                "https://arizona-recovery.react.domains/desktop/mods/village_staging",
            ),
            Self::Trilogy => (
                "https://pc.az-ins.com/resource/launcher/mods/trilogy",
                "https://pc.az-ins.com/resource/launcher/mods/trilogy",
            ),
        }
    }
}

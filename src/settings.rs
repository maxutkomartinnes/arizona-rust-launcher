use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fs, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GameSettings {
    #[serde(default)]
    pub game_path: String,
    #[serde(default)]
    pub user_name: String,
    #[serde(default = "default_memory")]
    pub memory: u32,
    #[serde(default)]
    pub last_selected_server_id: Option<u32>,
    #[serde(default)]
    pub options: HashMap<String, bool>,
}
fn default_memory() -> u32 {
    2048
}
impl Default for GameSettings {
    fn default() -> Self {
        Self {
            game_path: String::new(),
            user_name: String::new(),
            memory: default_memory(),
            last_selected_server_id: None,
            options: HashMap::new(),
        }
    }
}
impl GameSettings {
    pub fn for_game(id: &str) -> Self {
        let mut g = Self::default();
        if [
            "arizona",
            "arizona_staging",
            "rodina",
            "rodina_staging",
            "village",
            "village_staging",
        ]
        .contains(&id)
        {
            g.options.insert("wideScreen".into(), true);
            g.options.insert("autoLogin".into(), true);
        }
        if ["arizona", "arizona_staging"].contains(&id) {
            for key in ["grass", "newGraphics", "authCef"] {
                g.options.insert(key.into(), true);
            }
        }
        g
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledMod {
    pub id: u64,
    pub version: String,
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub backups: Vec<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    #[serde(default)]
    pub last_selected_game_id: String,
    #[serde(default)]
    pub games: HashMap<String, GameSettings>,
    #[serde(default)]
    pub installed_mods: HashMap<String, Vec<InstalledMod>>,
    #[serde(default)]
    pub last_notification_id: u64,
    #[serde(default)]
    pub auto_launch: bool,
}
impl Settings {
    pub fn path() -> Result<PathBuf> {
        if let Some(root) = std::env::var_os("ARIZONA_RUST_CONFIG_DIR") {
            let root = PathBuf::from(root);
            if !root.is_absolute() {
                anyhow::bail!("ARIZONA_RUST_CONFIG_DIR must be an absolute path")
            }
            return Ok(root.join("settings.json"));
        }
        Ok(dirs::config_dir()
            .context("APPDATA unavailable")?
            .join("arizona-rust-launcher")
            .join("settings.json"))
    }
    pub fn load() -> Result<Self> {
        let p = Self::path()?;
        if p.exists() {
            return Ok(serde_json::from_slice(&fs::read(p)?)?);
        }
        Ok(Self::migrate_legacy().unwrap_or_default())
    }
    pub fn save(&self) -> Result<()> {
        let p = Self::path()?;
        fs::create_dir_all(p.parent().unwrap())?;
        let tmp = p.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(&tmp, &p).or_else(|_| {
            fs::remove_file(&p)?;
            fs::rename(&tmp, &p)
        })?;
        Ok(())
    }
    fn migrate_legacy() -> Result<Self> {
        let p = dirs::config_dir()
            .context("APPDATA unavailable")?
            .join("arizona-launcher")
            .join("settings.json");
        let v: serde_json::Value = serde_json::from_slice(&fs::read(p)?)?;
        let mut s = Self::default();
        s.last_selected_game_id = v["lastSelectedGameId"].as_str().unwrap_or("arizona").into();
        for id in crate::model::GameId::ALL.map(|g| g.id()) {
            let game = &v[id];
            if !game.is_object() {
                continue;
            }
            let mut g = GameSettings::for_game(id);
            g.game_path = game["gamePath"].as_str().unwrap_or("").into();
            g.user_name = game["userName"].as_str().unwrap_or("").into();
            g.last_selected_server_id = game["lastSelectedServerId"].as_u64().map(|x| x as u32);
            g.memory = game["memory"]["value"].as_u64().unwrap_or(2048) as u32;
            if let Some(opts) = game["options"].as_array() {
                for o in opts {
                    if let (Some(id), Some(value)) = (o["id"].as_str(), o["value"].as_bool()) {
                        g.options.insert(id.to_string(), value);
                    }
                }
            }
            s.games.insert(id.into(), g);
        }
        Ok(s)
    }
    pub fn game_mut(&mut self, id: &str) -> &mut GameSettings {
        self.games
            .entry(id.into())
            .or_insert_with(|| GameSettings::for_game(id))
    }
    pub fn game(&self, id: &str) -> GameSettings {
        self.games
            .get(id)
            .cloned()
            .unwrap_or_else(|| GameSettings::for_game(id))
    }
}

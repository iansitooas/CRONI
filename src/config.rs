use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub home_url: String,
    pub search_url: String,
    pub discard_after_minutes: u64,
    pub ultra_light_mode: bool,
    pub reduce_motion: bool,
    pub pause_media_when_unfocused: bool,
    pub adblock_disabled_hosts: Vec<String>,
    pub bookmarks: Vec<Bookmark>,
    pub shortcuts_initialized: bool,
    pub restore_urls: Vec<String>,
    pub downloads: Vec<StoredDownload>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Bookmark {
    pub title: String,
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredDownload {
    pub name: String,
    pub url: String,
    pub path: String,
    pub received: u64,
    pub total: Option<u64>,
    pub status: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            home_url: "https://www.google.com".into(),
            search_url: "https://www.google.com/search?q={query}".into(),
            discard_after_minutes: 5,
            ultra_light_mode: true,
            reduce_motion: true,
            pause_media_when_unfocused: true,
            adblock_disabled_hosts: Vec::new(),
            bookmarks: Vec::new(),
            shortcuts_initialized: false,
            restore_urls: Vec::new(),
            downloads: Vec::new(),
        }
    }
}

impl AppConfig {
    pub fn load() -> Self {
        let mut config: Self = fs::read_to_string(config_path())
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();

        // Migrate the original project defaults without overwriting a custom choice.
        if config.home_url == "https://duckduckgo.com" {
            config.home_url = "https://www.google.com".into();
        }
        if config.search_url == "https://duckduckgo.com/?q={query}" {
            config.search_url = "https://www.google.com/search?q={query}".into();
        }
        for url in &mut config.restore_urls {
            if url == "https://duckduckgo.com" || url == "https://duckduckgo.com/" {
                *url = "https://www.google.com".into();
            }
        }
        if !config.shortcuts_initialized {
            config.shortcuts_initialized = true;
            if !config
                .bookmarks
                .iter()
                .any(|item| item.url.contains("youtube.com"))
            {
                config.bookmarks.push(Bookmark {
                    title: "YouTube".into(),
                    url: "https://www.youtube.com".into(),
                });
            }
        }
        config
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let payload = serde_json::to_vec_pretty(self)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        fs::write(path, payload)
    }
}

pub fn app_data_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let croni = base.join("CRONI");
    let legacy = base.join("Navegadir");
    // Existing users keep their cookies, logins and settings after the rename.
    if !croni.exists() && legacy.exists() {
        legacy
    } else {
        croni
    }
}

fn config_path() -> PathBuf {
    app_data_dir().join("config.json")
}

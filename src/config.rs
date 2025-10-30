
use anyhow::Result;
use home::home_dir;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Config {
    pub editor: String, // default nvim or nano
    pub file_manager: String, // default nnn or lf
    pub root_dir_name: String, // e.g., "helpername"
    pub aur_mirror: String, // "aur" (default) or "github"
    pub mirror_base: Option<String>, // optional custom base when using github mirror
    pub noconfirm: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            editor: "nvim".to_string(),
            file_manager: "nnn".to_string(),
            root_dir_name: "turbo".to_string(),
            aur_mirror: "aur".to_string(),
            mirror_base: None,
            noconfirm: false,
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        // Start with defaults
        let mut cfg = Self::default();

        // Load from legacy config file ~/.config/aurwrap/config.toml (if present)
        if let Ok(ed) = std::env::var("AURWRAP_EDITOR") {
            if !ed.trim().is_empty() { cfg.editor = ed; }
        }
        if let Ok(fm) = std::env::var("AURWRAP_FM") {
            if !fm.trim().is_empty() { cfg.file_manager = fm; }
        }
        if let Ok(rd) = std::env::var("AURWRAP_ROOT_DIR_NAME") {
            if !rd.trim().is_empty() { cfg.root_dir_name = rd; }
        }
        if let Ok(m) = std::env::var("AURWRAP_MIRROR") {
            if !m.trim().is_empty() { cfg.aur_mirror = m.to_lowercase(); }
        }
        if let Ok(b) = std::env::var("AURWRAP_MIRROR_BASE") {
            if !b.trim().is_empty() { cfg.mirror_base = Some(b); }
        }
        // Config file: ~/.config/aurwrap/config.toml
        if let Some(home) = home_dir() {
            let path = home.join(".config/aurwrap/config.toml");
            if path.exists() {
                if let Ok(contents) = fs::read_to_string(&path) {
                    let value: toml::Value = contents.parse::<toml::Value>()?;
                    if let Some(t) = value.get("editor").and_then(|v| v.as_str()) {
                        cfg.editor = t.to_string();
                    }
                    if let Some(t) = value.get("file_manager").and_then(|v| v.as_str()) {
                        cfg.file_manager = t.to_string();
                    }
                    if let Some(t) = value.get("root_dir_name").and_then(|v| v.as_str()) {
                        cfg.root_dir_name = t.to_string();
                    }
                    if let Some(t) = value.get("mirror").and_then(|v| v.as_str()) {
                        cfg.aur_mirror = t.to_string();
                    }
                    if let Some(t) = value.get("mirror_base").and_then(|v| v.as_str()) {
                        cfg.mirror_base = Some(t.to_string());
                    }
                    if let Some(t) = value.get("noconfirm").and_then(|v| v.as_str()) {
                        cfg.noconfirm = t.to_lowercase() == "true";
                    }
                }
            }
        }

        // Also support simple conf at ~/turbo/conf (key=value lines)
        if let Some(home) = home_dir() {
            let conf_path = home.join(cfg.root_dir_name.as_str()).join("conf");
            if conf_path.exists() {
                if let Ok(contents) = fs::read_to_string(&conf_path) {
                    for line in contents.lines() {
                        let line = line.trim();
                        if line.is_empty() || line.starts_with('#') { continue; }
                        if let Some((k,v)) = line.split_once('=') {
                            let k = k.trim();
                            let v = v.trim();
                            match k {
                                "editor" => cfg.editor = v.to_string(),
                                "file_manager" => cfg.file_manager = v.to_string(),
                                "mirror" => cfg.aur_mirror = v.to_lowercase(),
                                "mirror_base" => cfg.mirror_base = Some(v.to_string()),
                                _ => {}
                            }
                        }
                    }
                }
            }
        }

        // Finally, apply env overrides again to supersede conf (as requested)
        if let Ok(ed) = std::env::var("AURWRAP_EDITOR") {
            if !ed.trim().is_empty() { cfg.editor = ed; }
        }
        if let Ok(fm) = std::env::var("AURWRAP_FM") {
            if !fm.trim().is_empty() { cfg.file_manager = fm; }
        }
        if let Ok(rd) = std::env::var("AURWRAP_ROOT_DIR_NAME") {
            if !rd.trim().is_empty() { cfg.root_dir_name = rd; }
        }
        if let Ok(m) = std::env::var("AURWRAP_MIRROR") {
            if !m.trim().is_empty() { cfg.aur_mirror = m.to_lowercase(); }
        }
        if let Ok(b) = std::env::var("AURWRAP_MIRROR_BASE") {
            if !b.trim().is_empty() { cfg.mirror_base = Some(b); }
        }
        Ok(cfg)
    }

    pub fn root_dir(&self) -> PathBuf {
        let home = home_dir().unwrap_or_else(|| PathBuf::from("/"));
        home.join(&self.root_dir_name)
    }

    pub fn cache_dir(&self) -> PathBuf {
        self.root_dir().join("cache")
    }

    pub fn temp_dir(&self) -> PathBuf {
        self.cache_dir().join("temp")
    }
}


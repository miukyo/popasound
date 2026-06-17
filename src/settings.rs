use crate::library;
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Settings {
    #[serde(default = "default_volume")]
    pub global_volume: f64,
    #[serde(default)]
    pub device_name: Option<String>,
}

fn default_volume() -> f64 {
    1.0
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            global_volume: 1.0,
            device_name: None,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let path = library::data_dir().join("settings.json");
        fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let path = library::data_dir().join("settings.json");
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = fs::write(path, json);
        }
    }

    pub fn apply_deserialize_overrides(&mut self) {
        if self.global_volume < 0.0 {
            self.global_volume = 0.0;
        }
        if self.global_volume > 1.0 {
            self.global_volume = 1.0;
        }
    }
}

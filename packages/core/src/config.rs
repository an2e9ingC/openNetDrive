//! Configuration management for openNetDrive

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::error::{Result, Error};

/// Connection type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum ConnectionType {
    #[serde(rename = "webdav")]
    WebDAV {
        url: String,
        username: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        password: Option<String>,
    },
    #[serde(rename = "smb")]
    SMB {
        host: String,
        port: u16,
        share: String,
        path: String,
        username: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        password: Option<String>,
    },
}

/// Connection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionConfig {
    pub id: String,
    pub name: String,
    pub connection_type: ConnectionType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mount_point: Option<String>,
    #[serde(default)]
    pub auto_mount: bool,
    #[serde(default)]
    pub enabled: bool,
}

/// Main configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    pub connections: Vec<ConnectionConfig>,
    #[serde(default)]
    pub dark_mode: bool,
    #[serde(default)]
    pub start_on_boot: bool,
    #[serde(default)]
    pub log_level: String,
}

impl Config {
    /// Get the configuration file path
    pub fn config_path() -> Result<PathBuf> {
        let config_dir = dirs::config_dir()
            .ok_or_else(|| Error::Config("Could not determine config directory".to_string()))?;

        let config_dir = config_dir.join("openNetDrive");
        std::fs::create_dir_all(&config_dir)?;

        Ok(config_dir.join("config.toml"))
    }

    /// Load configuration from file
    pub fn load() -> Result<Self> {
        let path = Self::config_path()?;

        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&path)?;
        let config: Config = toml::from_str(&content)
            .map_err(|e| Error::Config(format!("Failed to parse config: {}", e)))?;

        Ok(config)
    }

    /// Save configuration to file
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path()?;
        let content = toml::to_string_pretty(self)
            .map_err(|e| Error::Config(format!("Failed to serialize config: {}", e)))?;

        std::fs::write(path, content)?;
        Ok(())
    }

    /// Add a connection
    pub fn add_connection(&mut self, config: ConnectionConfig) {
        self.connections.push(config);
    }

    /// Remove a connection by ID
    pub fn remove_connection(&mut self, id: &str) -> Option<ConnectionConfig> {
        if let Some(pos) = self.connections.iter().position(|c| c.id == id) {
            Some(self.connections.remove(pos))
        } else {
            None
        }
    }

    /// Get a connection by ID
    pub fn get_connection(&self, id: &str) -> Option<&ConnectionConfig> {
        self.connections.iter().find(|c| c.id == id)
    }
}

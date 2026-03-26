//! Runtime state management for mounted drives

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use anyhow::{Result, Context};

/// Mount state for a mounted connection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MountState {
    pub mount_point: String,
    pub connection_id: String,
    pub connection_name: String,
    pub pid: u32,
}

/// Get the state file path
pub fn state_dir() -> PathBuf {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("openNetDrive");

    fs::create_dir_all(&config_dir).ok();
    config_dir.join("mounts")
}

/// Get state file path for a specific mount point
fn state_file_for_mount(mount_point: &str) -> PathBuf {
    // Sanitize mount point for filename
    let safe_name = mount_point.replace(":", "").replace("\\", "");
    state_dir().join(format!("{}.json", safe_name))
}

/// Save mount state
pub fn save_mount_state(state: &MountState) -> Result<()> {
    let state_file = state_file_for_mount(&state.mount_point);
    let content = toml::to_string_pretty(state)
        .context("Failed to serialize mount state")?;

    fs::write(&state_file, content)
        .context(format!("Failed to write state file: {:?}", state_file))?;

    Ok(())
}

/// Remove mount state
pub fn remove_mount_state(mount_point: &str) -> Result<()> {
    let state_file = state_file_for_mount(mount_point);

    if state_file.exists() {
        fs::remove_file(&state_file)
            .context(format!("Failed to remove state file: {:?}", state_file))?;
    }

    Ok(())
}

/// Get mount state for a specific mount point
pub fn get_mount_state(mount_point: &str) -> Result<Option<MountState>> {
    let state_file = state_file_for_mount(mount_point);

    if !state_file.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&state_file)
        .context(format!("Failed to read state file: {:?}", state_file))?;

    let state: MountState = toml::from_str(&content)
        .context("Failed to parse mount state")?;

    Ok(Some(state))
}

/// Get mount state by connection ID
pub fn get_mount_state_by_id(connection_id: &str) -> Result<Option<MountState>> {
    let state_dir = state_dir();

    if !state_dir.exists() {
        return Ok(None);
    }

    for entry in fs::read_dir(&state_dir)
        .context("Failed to read state directory")?
    {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(state) = toml::from_str::<MountState>(&content) {
                    if state.connection_id == connection_id {
                        return Ok(Some(state));
                    }
                }
            }
        }
    }

    Ok(None)
}

/// List all mount states
#[allow(dead_code)]
pub fn list_mount_states() -> Result<Vec<MountState>> {
    let state_dir = state_dir();
    let mut states = Vec::new();

    if !state_dir.exists() {
        return Ok(states);
    }

    for entry in fs::read_dir(&state_dir)
        .context("Failed to read state directory")?
    {
        let entry = entry?;
        let path = entry.path();

        if path.extension().and_then(|s| s.to_str()) == Some("json") {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(state) = toml::from_str::<MountState>(&content) {
                    states.push(state);
                }
            }
        }
    }

    Ok(states)
}

/// Check if a mount point is already in use
pub fn is_mount_point_in_use(mount_point: &str) -> Result<bool> {
    get_mount_state(mount_point).map(|s| s.is_some())
}

// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use opennetdrive_core::{Config, ConnectionConfig, ConnectionType, WebDAVClient, create_smb_client};
use opennetdrive_mount_win::WinFspDriver;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use log::info;

// Global mount state
struct MountState {
    drivers: RwLock<std::collections::HashMap<String, WinFspDriver>>,
}

impl MountState {
    fn new() -> Self {
        Self {
            drivers: RwLock::new(std::collections::HashMap::new()),
        }
    }
}

lazy_static::lazy_static! {
    static ref MOUNT_STATE: Arc<MountState> = Arc::new(MountState::new());
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConnectionInfo {
    id: String,
    name: String,
    connection_type: String,
    mount_point: Option<String>,
    auto_mount: bool,
    enabled: bool,
}

#[tauri::command]
fn get_connections() -> Result<Vec<ConnectionInfo>, String> {
    let config = Config::load().map_err(|e| e.to_string())?;

    let connections = config.connections.into_iter().map(|c| {
        let connection_type = match c.connection_type {
            ConnectionType::WebDAV { .. } => "webdav".to_string(),
            ConnectionType::SMB { .. } => "smb".to_string(),
        };

        ConnectionInfo {
            id: c.id,
            name: c.name,
            connection_type,
            mount_point: c.mount_point,
            auto_mount: c.auto_mount,
            enabled: c.enabled,
        }
    }).collect();

    Ok(connections)
}

#[tauri::command]
async fn add_connection(
    name: String,
    connection_type: String,
    host: String,
    username: String,
    password: Option<String>,
    auto_mount: Option<bool>,
) -> Result<(), String> {
    let mut config = Config::load().map_err(|e| e.to_string())?;

    let id = uuid::Uuid::new_v4().to_string()[..8].to_string();

    let conn_type = match connection_type.as_str() {
        "webdav" => ConnectionType::WebDAV {
            url: host,
            username,
            password,
        },
        "smb" => ConnectionType::SMB {
            host,
            port: 445,
            share: "share".to_string(),
            path: "/".to_string(),
            username,
            password,
        },
        _ => return Err("Invalid connection type".to_string()),
    };

    let conn = ConnectionConfig {
        id,
        name,
        connection_type: conn_type,
        mount_point: None,
        auto_mount: auto_mount.unwrap_or(false),
        enabled: false,
    };

    config.add_connection(conn);
    config.save().map_err(|e| e.to_string())?;

    Ok(())
}

#[tauri::command]
fn remove_connection(id: String) -> Result<(), String> {
    let mut config = Config::load().map_err(|e| e.to_string())?;

    if config.remove_connection(&id).is_some() {
        config.save().map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err("Connection not found".to_string())
    }
}

#[tauri::command]
async fn mount_connection(id: String) -> Result<(), String> {
    info!("Mounting connection: {}", id);

    let config = Config::load().map_err(|e| e.to_string())?;

    let conn = config.connections.iter()
        .find(|c| c.id == id)
        .ok_or_else(|| "Connection not found".to_string())?
        .clone();

    // Check if already mounted
    {
        let drivers = MOUNT_STATE.drivers.read().await;
        if drivers.contains_key(&id) {
            return Err("Connection already mounted".to_string());
        }
    }

    // Create protocol instance based on connection type
    let protocol: Box<dyn opennetdrive_core::Protocol> = match &conn.connection_type {
        ConnectionType::WebDAV { url, username, password } => {
            let client = WebDAVClient::new(url, username, password.as_deref())
                .map_err(|e| format!("Failed to create WebDAV client: {}", e))?;
            Box::new(client)
        }
        ConnectionType::SMB { host, port, share, path, username, password } => {
            let client = create_smb_client(
                host,
                *port,
                share,
                path,
                username,
                password.as_deref(),
            ).map_err(|e| format!("Failed to create SMB client: {}", e))?;
            Box::new(client)
        }
    };

    // Determine mount point (drive letter)
    let mount_point = conn.mount_point.unwrap_or_else(|| {
        // Find available drive letter
        ('Z'..='A').rev()
            .map(|c| format!("{}:", c))
            .find(|drive| !std::path::Path::new(&format!("{}\\", drive)).exists())
            .unwrap_or_else(|| "X:".to_string())
    });

    // Create and start the driver
    let mut driver = WinFspDriver::new(mount_point.clone(), protocol);

    driver.start().await
        .map_err(|e| format!("Failed to start mount: {}", e))?;

    // Store the driver
    {
        let mut drivers = MOUNT_STATE.drivers.write().await;
        drivers.insert(id.clone(), driver);
    }

    // Update config to mark as enabled
    let mut config = Config::load().map_err(|e| e.to_string())?;
    if let Some(c) = config.connections.iter_mut().find(|c| c.id == id) {
        c.enabled = true;
        c.mount_point = Some(mount_point.clone());
    }
    config.save().map_err(|e| e.to_string())?;

    info!("Successfully mounted {} at {}", id, mount_point);
    Ok(())
}

#[tauri::command]
async fn unmount_connection(id: String) -> Result<(), String> {
    info!("Unmounting connection: {}", id);

    // Stop and remove the driver
    {
        let mut drivers = MOUNT_STATE.drivers.write().await;
        if let Some(mut driver) = drivers.remove(&id) {
            driver.stop().await
                .map_err(|e| format!("Failed to stop mount: {}", e))?;
        } else {
            return Err("Connection not mounted".to_string());
        }
    }

    // Update config to mark as disabled
    let mut config = Config::load().map_err(|e| e.to_string())?;
    if let Some(c) = config.connections.iter_mut().find(|c| c.id == id) {
        c.enabled = false;
        c.mount_point = None;
    }
    config.save().map_err(|e| e.to_string())?;

    info!("Successfully unmounted {}", id);
    Ok(())
}

#[tauri::command]
fn update_connection(
    id: String,
    name: String,
    connection_type: String,
    auto_mount: Option<bool>,
) -> Result<(), String> {
    let mut config = Config::load().map_err(|e| e.to_string())?;

    if let Some(conn) = config.connections.iter_mut().find(|c| c.id == id) {
        conn.name = name;
        conn.auto_mount = auto_mount.unwrap_or(false);

        // Update connection type if needed
        match connection_type.as_str() {
            "webdav" => {
                if let ConnectionType::WebDAV { .. } = &conn.connection_type {
                    // Already WebDAV, keep existing config
                } else {
                    // Switch to WebDAV
                    conn.connection_type = ConnectionType::WebDAV {
                        url: String::new(),
                        username: String::new(),
                        password: None,
                    };
                }
            }
            "smb" => {
                if let ConnectionType::SMB { .. } = &conn.connection_type {
                    // Already SMB, keep existing config
                } else {
                    // Switch to SMB
                    conn.connection_type = ConnectionType::SMB {
                        host: String::new(),
                        port: 445,
                        share: "share".to_string(),
                        path: "/".to_string(),
                        username: String::new(),
                        password: None,
                    };
                }
            }
            _ => return Err("Invalid connection type".to_string()),
        }

        config.save().map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err("Connection not found".to_string())
    }
}

fn main() {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("opennetdrive=info".parse().unwrap())
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_connections,
            add_connection,
            remove_connection,
            mount_connection,
            unmount_connection,
            update_connection
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

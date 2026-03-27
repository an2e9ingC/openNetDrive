// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use opennetdrive_core::{Config, ConnectionConfig, ConnectionType, WebDAVClient, create_smb_client, CredentialManager};
use opennetdrive_mount_win::WinFspDriver;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use log::{info, error, warn};

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

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ConnectionInfo {
    pub id: String,
    pub name: String,
    pub connection_type: String,
    pub mount_point: Option<String>,
    pub auto_mount: bool,
    pub enabled: bool,
    pub host: Option<String>,
    pub username: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MountResult {
    pub success: bool,
    pub mount_point: Option<String>,
    pub message: String,
}

#[tauri::command]
fn get_connections() -> Result<Vec<ConnectionInfo>, String> {
    let config = Config::load().map_err(|e| e.to_string())?;

    let connections = config.connections.iter().map(|c| {
        let (connection_type, host, username) = match &c.connection_type {
            ConnectionType::WebDAV { url, username, .. } => ("webdav".to_string(), Some(url.clone()), Some(username.clone())),
            ConnectionType::SMB { host, username, .. } => ("smb".to_string(), Some(host.clone()), Some(username.clone())),
        };

        ConnectionInfo {
            id: c.id.clone(),
            name: c.name.clone(),
            connection_type,
            mount_point: c.mount_point.clone(),
            auto_mount: c.auto_mount,
            enabled: c.enabled,
            host,
            username,
        }
    }).collect();

    Ok(connections)
}

#[tauri::command]
fn get_connection_details(id: String) -> Result<ConnectionInfo, String> {
    let config = Config::load().map_err(|e| e.to_string())?;

    let conn = config.connections.iter()
        .find(|c| c.id == id)
        .ok_or_else(|| "Connection not found".to_string())?;

    let (connection_type, host, username) = match &conn.connection_type {
        ConnectionType::WebDAV { url, username, .. } => ("webdav".to_string(), Some(url.clone()), Some(username.clone())),
        ConnectionType::SMB { host, username, .. } => ("smb".to_string(), Some(host.clone()), Some(username.clone())),
    };

    Ok(ConnectionInfo {
        id: conn.id.clone(),
        name: conn.name.clone(),
        connection_type,
        mount_point: conn.mount_point.clone(),
        auto_mount: conn.auto_mount,
        enabled: conn.enabled,
        host,
        username,
    })
}

#[tauri::command]
async fn add_connection(
    name: String,
    connection_type: String,
    host: String,
    username: String,
    password: Option<String>,
    auto_mount: Option<bool>,
) -> Result<String, String> {
    let mut config = Config::load().map_err(|e| e.to_string())?;

    let id = uuid::Uuid::new_v4().to_string()[..8].to_string();

    // Store password in credential manager
    if let Some(ref pwd) = password {
        if !pwd.is_empty() {
            let cred_manager = CredentialManager::new().map_err(|e| e.to_string())?;
            cred_manager.store_for_connection(&id, &username, pwd).map_err(|e| e.to_string())?;
        }
    }

    let conn_type = match connection_type.as_str() {
        "webdav" => ConnectionType::WebDAV {
            url: host,
            username,
            password: None, // Password stored in credential manager
        },
        "smb" => ConnectionType::SMB {
            host,
            port: 445,
            share: "share".to_string(),
            path: "/".to_string(),
            username,
            password: None, // Password stored in credential manager
        },
        _ => return Err("Invalid connection type".to_string()),
    };

    let conn = ConnectionConfig {
        id: id.clone(),
        name,
        connection_type: conn_type,
        mount_point: None,
        auto_mount: auto_mount.unwrap_or(false),
        enabled: false,
    };

    config.add_connection(conn);
    config.save().map_err(|e| e.to_string())?;

    info!("Added connection: {}", id);
    Ok(id)
}

#[tauri::command]
fn remove_connection(id: String) -> Result<(), String> {
    let mut config = Config::load().map_err(|e| e.to_string())?;

    // Get connection to remove credentials
    if let Some(conn) = config.get_connection(&id) {
        let username = match &conn.connection_type {
            ConnectionType::WebDAV { username, .. } => username,
            ConnectionType::SMB { username, .. } => username,
        };

        // Remove credentials
        if let Ok(cred_manager) = CredentialManager::new() {
            let _ = cred_manager.delete_for_connection(&id, username);
        }
    }

    if config.remove_connection(&id).is_some() {
        config.save().map_err(|e| e.to_string())?;
        info!("Removed connection: {}", id);
        Ok(())
    } else {
        Err("Connection not found".to_string())
    }
}

#[tauri::command]
async fn mount_connection(id: String) -> Result<MountResult, String> {
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
            return Ok(MountResult {
                success: false,
                mount_point: conn.mount_point,
                message: "Connection already mounted".to_string(),
            });
        }
    }

    // Get password from credential manager
    let password = {
        let username = match &conn.connection_type {
            ConnectionType::WebDAV { username, .. } => username,
            ConnectionType::SMB { username, .. } => username,
        };

        if let Ok(cred_manager) = CredentialManager::new() {
            cred_manager.get_for_connection(&id, username).ok()
        } else {
            None
        }
    };

    // Create protocol instance based on connection type
    let protocol: Box<dyn opennetdrive_core::Protocol> = match &conn.connection_type {
        ConnectionType::WebDAV { url, username, .. } => {
            let client = WebDAVClient::new(url, username, password.as_deref())
                .map_err(|e| format!("Failed to create WebDAV client: {}", e))?;
            Box::new(client)
        }
        ConnectionType::SMB { host, port, share, path, username, .. } => {
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

    match driver.start().await {
        Ok(_) => {
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

            Ok(MountResult {
                success: true,
                mount_point: Some(mount_point),
                message: "Mounted successfully".to_string(),
            })
        }
        Err(e) => {
            error!("Failed to mount {}: {}", id, e);
            Ok(MountResult {
                success: false,
                mount_point: None,
                message: format!("Failed to mount: {}", e),
            })
        }
    }
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
    password: Option<String>,
) -> Result<(), String> {
    let mut config = Config::load().map_err(|e| e.to_string())?;

    if let Some(conn) = config.connections.iter_mut().find(|c| c.id == id) {
        // Update password if provided
        if let Some(ref pwd) = password {
            if !pwd.is_empty() {
                let username = match &conn.connection_type {
                    ConnectionType::WebDAV { username, .. } => username,
                    ConnectionType::SMB { username, .. } => username,
                };

                if let Ok(cred_manager) = CredentialManager::new() {
                    let _ = cred_manager.store_for_connection(&id, username, pwd);
                }
            }
        }

        conn.name = name;
        conn.auto_mount = auto_mount.unwrap_or(conn.auto_mount);

        // Update connection type if needed
        match connection_type.as_str() {
            "webdav" => {
                if let ConnectionType::SMB { .. } = &conn.connection_type {
                    conn.connection_type = ConnectionType::WebDAV {
                        url: String::new(),
                        username: String::new(),
                        password: None,
                    };
                }
            }
            "smb" => {
                if let ConnectionType::WebDAV { .. } = &conn.connection_type {
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
        info!("Updated connection: {}", id);
        Ok(())
    } else {
        Err("Connection not found".to_string())
    }
}

#[tauri::command]
async fn auto_mount_connections() -> Result<Vec<MountResult>, String> {
    info!("Auto mounting connections...");

    let config = Config::load().map_err(|e| e.to_string())?;
    let mut results = Vec::new();

    for conn in config.connections.iter().filter(|c| c.auto_mount && !c.enabled) {
        let result = match mount_connection(conn.id.clone()).await {
            Ok(r) => r,
            Err(e) => MountResult {
                success: false,
                mount_point: None,
                message: e,
            },
        };
        results.push(result);
    }

    Ok(results)
}

/// Initialize auto-mount for connections marked as auto_mount
async fn init_auto_mount() {
    if let Ok(config) = Config::load() {
        for conn in config.connections.iter().filter(|c| c.auto_mount && !c.enabled) {
            info!("Auto-mounting connection: {} ({})", conn.name, conn.id);

            match mount_connection(conn.id.clone()).await {
                Ok(result) => {
                    if result.success {
                        info!("Auto-mounted {} at {}", conn.name, result.mount_point.unwrap_or_default());
                    } else {
                        warn!("Failed to auto-mount {}: {}", conn.name, result.message);
                    }
                }
                Err(e) => {
                    warn!("Failed to auto-mount {}: {}", conn.name, e);
                }
            }
        }
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

    info!("Starting openNetDrive...");

    // Auto-mount connections on startup
    let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
    runtime.block_on(async {
        init_auto_mount().await;
    });

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            get_connections,
            get_connection_details,
            add_connection,
            remove_connection,
            mount_connection,
            unmount_connection,
            update_connection,
            auto_mount_connections
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
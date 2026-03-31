// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use opennetdrive_core::{Config, ConnectionConfig, ConnectionType, WebDAVClient, create_smb_client, mount_smb_share, CredentialManager};
use opennetdrive_mount_win::WinFspDriver;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use log::{info, error, warn};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, WindowEvent,
};

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
    pub share: Option<String>,       // SMB 共享名称
    pub remote_path: Option<String>, // SMB 远程路径
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MountResult {
    pub success: bool,
    pub mount_point: Option<String>,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppSettings {
    pub theme_mode: String,  // "dark", "light", "system"
    pub start_minimized: bool,
    pub auto_start: bool,
    pub log_level: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            theme_mode: "system".to_string(),
            start_minimized: false,
            auto_start: false,
            log_level: "info".to_string(),
        }
    }
}

#[tauri::command]
fn get_connections() -> Result<Vec<ConnectionInfo>, String> {
    let config = Config::load().map_err(|e| e.to_string())?;

    let connections = config.connections.iter().map(|c| {
        let (connection_type, host, username, share, remote_path) = match &c.connection_type {
            ConnectionType::WebDAV { url, username, .. } => ("webdav".to_string(), Some(url.clone()), Some(username.clone()), None, None),
            ConnectionType::SMB { host, share, path, username, .. } => ("smb".to_string(), Some(host.clone()), Some(username.clone()), Some(share.clone()), Some(path.clone())),
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
            share,
            remote_path,
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

    let (connection_type, host, username, share, remote_path) = match &conn.connection_type {
        ConnectionType::WebDAV { url, username, .. } => ("webdav".to_string(), Some(url.clone()), Some(username.clone()), None, None),
        ConnectionType::SMB { host, share, path, username, .. } => ("smb".to_string(), Some(host.clone()), Some(username.clone()), Some(share.clone()), Some(path.clone())),
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
        share,
        remote_path,
    })
}

#[tauri::command]
async fn add_connection(
    name: String,
    connection_type: String,
    host: String,
    share: Option<String>,
    remote_path: Option<String>,
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
            password: None,
        },
        "smb" => ConnectionType::SMB {
            host,
            port: 445,
            share: share.unwrap_or_else(|| "share".to_string()),
            path: remote_path.unwrap_or_else(|| "/".to_string()),
            username,
            password: None,
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
async fn mount_connection(id: String, app_handle: tauri::AppHandle) -> Result<MountResult, String> {
    info!("Mounting connection: {}", id);
    let _ = app_handle.emit("log-event", serde_json::json!({"level": "info", "message": format!("开始挂载连接: {}", id)}));

    let config = Config::load().map_err(|e| {
        let _ = app_handle.emit("log-event", serde_json::json!({"level": "error", "message": format!("加载配置失败: {}", e)}));
        e.to_string()
    })?;

    let conn = config.connections.iter()
        .find(|c| c.id == id)
        .ok_or_else(|| "Connection not found".to_string())?
        .clone();

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

    // 根据协议类型处理挂载
    let mount_point = if let Some(ref mp) = conn.mount_point {
        let path = format!("{}\\", mp);
        if !std::path::Path::new(&path).exists() {
            mp.clone()
        } else {
            find_available_drive(&conn.name).unwrap_or_else(|| "X:".to_string())
        }
    } else {
        find_available_drive(&conn.name).unwrap_or_else(|| "X:".to_string())
    };

    // SMB 协议使用 net use 直接挂载
    if let ConnectionType::SMB { host, port, share, path, username, .. } = &conn.connection_type {
        // 检查凭据是否完整
        let has_username = !username.is_empty();
        let has_password = password.is_some() && !password.as_ref().unwrap().is_empty();

        if !has_username || !has_password {
            return Ok(MountResult {
                success: false,
                mount_point: None,
                message: "请先编辑连接，填写用户名和密码后再挂载".to_string(),
            });
        }

        info!("Mounting SMB share: \\{}{}\\{}", host, share, path);

        // 使用 net use 直接挂载
        match mount_smb_share(
            host,
            *port,
            share,
            path,
            username,
            password.as_deref(),
            &mount_point,
        ).await {
            Ok(true) => {
                // 验证挂载点存在
                let mount_path = format!("{}\\", mount_point);
                if !std::path::Path::new(&mount_path).exists() {
                    return Ok(MountResult {
                        success: false,
                        mount_point: Some(mount_point),
                        message: "挂载命令执行成功，但磁盘未生效，请检查网络连接".to_string(),
                    });
                }

                // 保存挂载状态
                let mut config = Config::load().map_err(|e| e.to_string())?;
                if let Some(c) = config.connections.iter_mut().find(|c| c.id == id) {
                    c.enabled = true;
                    c.mount_point = Some(mount_point.clone());
                }
                config.save().map_err(|e| e.to_string())?;

                info!("Successfully mounted SMB {} to {}", conn.name, mount_point);
                let _ = app_handle.emit("log-event", serde_json::json!({"level": "info", "message": format!("SMB 挂载成功: {} -> {}", conn.name, mount_point)}));

                Ok(MountResult {
                    success: true,
                    mount_point: Some(mount_point.clone()),
                    message: format!("已成功挂载到 {}", mount_point),
                })
            }
            Ok(false) => {
                let _ = app_handle.emit("log-event", serde_json::json!({"level": "error", "message": "SMB 服务器连接失败"}));
                Ok(MountResult {
                    success: false,
                    mount_point: None,
                    message: "无法连接到 SMB 服务器，请检查网络连接、服务器地址和凭据".to_string(),
                })
            }
            Err(e) => {
                let error_msg = format!("{}", e);
                error!("Failed to mount SMB: {}", error_msg);
                let _ = app_handle.emit("log-event", serde_json::json!({"level": "error", "message": format!("SMB 挂载失败: {}", error_msg)}));
                Ok(MountResult {
                    success: false,
                    mount_point: None,
                    message: error_msg,
                })
            }
        }
    } else {
        // WebDAV 协议使用 WinFsp
        let protocol: Box<dyn opennetdrive_core::Protocol> = match &conn.connection_type {
            ConnectionType::WebDAV { url, username, .. } => {
                let client = WebDAVClient::new(url, username, password.as_deref())
                    .map_err(|e| format!("Failed to create WebDAV client: {}", e))?;
                Box::new(client)
            }
            _ => return Err("Unsupported connection type".to_string()),
        };

        let mut driver = WinFspDriver::new(mount_point.clone(), protocol);

        match driver.start().await {
            Ok(_) => {
                let mount_path = format!("{}\\", mount_point);
                if !std::path::Path::new(&mount_path).exists() {
                    return Ok(MountResult {
                        success: false,
                        mount_point: Some(mount_point),
                        message: "挂载失败：磁盘未成功创建，请检查网络连接或配置".to_string(),
                    });
                }

                {
                    let mut drivers = MOUNT_STATE.drivers.write().await;
                    drivers.insert(id.clone(), driver);
                }

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
}

#[tauri::command]
async fn unmount_connection(id: String, app_handle: tauri::AppHandle) -> Result<(), String> {
    info!("Unmounting connection: {}", id);
    let _ = app_handle.emit("log-event", serde_json::json!({"level": "info", "message": format!("开始断开连接: {}", id)}));

    // 先查找连接信息
    let config_result = Config::load();

    // 无论能否加载配置，都尝试从系统获取已挂载的网络驱动器
    let mut unmounted = false;

    // 从系统获取当前已挂载的网络驱动器
    let output = std::process::Command::new("net")
        .args(["use"])
        .output()
        .map_err(|e| format!("Failed to get net use list: {}", e))?;

    let output_str = String::from_utf8_lossy(&output.stdout);
    info!("Current net use output:\n{}", output_str);

    // 尝试用 ID 作为驱动器号断开
    // 假设 id 可能是驱动器号如 "Z" 或完整名称
    let drive_letters = ['Z', 'Y', 'X', 'W', 'V', 'U', 'T', 'S', 'R', 'Q', 'P', 'O', 'N', 'M', 'L', 'K', 'J', 'I', 'H', 'G', 'F', 'E', 'D', 'C', 'B', 'A'];

    // 如果 ID 看起来像驱动器号，尝试断开它
    if id.len() == 1 && id.chars().next().map(|c| c.is_alphabetic()).unwrap_or(false) {
        let drive_with_colon = format!("{}:", id.to_uppercase());
        info!("Trying to unmount drive: {}", drive_with_colon);

        let output = std::process::Command::new("net")
            .args(["use", &drive_with_colon, "/delete", "/y"])
            .output()
            .map_err(|e| format!("Failed to execute net use: {}", e))?;

        if output.status.success() {
            info!("Successfully unmounted drive {}", drive_with_colon);
            unmounted = true;
        } else {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            info!("net use result: {} {}", stdout, stderr);
        }
    } else if let Ok(config) = config_result {
        // 从配置中查找连接
        if let Some(conn) = config.connections.iter().find(|c| c.id == id) {
            info!("Found connection in config: {} mount_point: {:?}", conn.name, conn.mount_point);

            if let Some(ref mount_point) = conn.mount_point {
                let drive = mount_point.trim_end_matches('\\').trim_end_matches(':');
                let drive_with_colon = format!("{}:", drive.to_uppercase());

                info!("Trying to unmount: {}", drive_with_colon);

                let output = std::process::Command::new("net")
                    .args(["use", &drive_with_colon, "/delete", "/y"])
                    .output()
                    .map_err(|e| format!("Failed to execute net use: {}", e))?;

                if output.status.success() {
                    info!("Successfully unmounted drive {}", drive_with_colon);
                    unmounted = true;
                }
            }
        }

        // 更新配置 - 只更新 enabled 状态，保留 mount_point
        let mut config = config;
        if let Some(c) = config.connections.iter_mut().find(|c| c.id == id) {
            c.enabled = false;
            // 不再清空 mount_point，保留用户配置的盘符
            let _ = config.save();
            info!("Updated config for {}", id);
        }
    }

    // 尝试从 drivers 中查找（用于 WebDAV）
    {
        let mut drivers = MOUNT_STATE.drivers.write().await;
        if let Some(mut driver) = drivers.remove(&id) {
            driver.stop().await
                .map_err(|e| format!("Failed to stop mount: {}", e))?;
            unmounted = true;
        }
    }

    if unmounted {
        info!("Successfully unmounted {}", id);
        let _ = app_handle.emit("log-event", serde_json::json!({"level": "info", "message": format!("已成功断开: {}", id)}));
        Ok(())
    } else {
        // 即使没有成功断开，也不报错，因为可能已经断开了
        info!("Could not find mounted connection for {}, assuming already unmounted", id);
        Ok(())
    }
}

/// Get list of currently mounted network drives from system
#[derive(Debug, Serialize)]
pub struct MountedDrive {
    pub drive: String,
    pub remote: String,
    pub status: String,
}

#[tauri::command]
fn get_mounted_drives() -> Result<Vec<MountedDrive>, String> {
    info!("Getting mounted drives...");

    let output = std::process::Command::new("net")
        .args(["use"])
        .output()
        .map_err(|e| format!("Failed to get net use list: {}", e))?;

    let output_str = String::from_utf8_lossy(&output.stdout);
    info!("net use output:\n{}", output_str);

    let mut drives = Vec::new();

    // 解析输出，格式类似：
    // Z:        \\server\share    Microsoft Windows Network
    for line in output_str.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // 检查是否是驱动器行（以字母冒号开头）
        if line.len() >= 2 && line.chars().nth(1) == Some(':') {
            let drive = line[0..2].to_string();
            let rest = line[3..].trim();

            // 提取远程路径
            let remote = if rest.starts_with("\\\\") || rest.starts_with("//") {
                rest.split_whitespace().next().unwrap_or("").to_string()
            } else {
                String::new()
            };

            let status = if !remote.is_empty() {
                "Connected".to_string()
            } else {
                "Disconnected".to_string()
            };

            if !drive.is_empty() {
                drives.push(MountedDrive {
                    drive,
                    remote,
                    status,
                });
            }
        }
    }

    info!("Found {} mounted drives", drives.len());
    Ok(drives)
}

/// Scan existing SMB connections from system and auto-import to config
#[tauri::command]
fn sync_existing_connections() -> Result<Vec<ConnectionInfo>, String> {
    info!("Syncing existing SMB connections from system...");

    // Get current mounted drives from system
    let output = std::process::Command::new("net")
        .args(["use"])
        .output()
        .map_err(|e| format!("Failed to get net use list: {}", e))?;

    let output_str = String::from_utf8_lossy(&output.stdout);

    let mut config = Config::load().map_err(|e| e.to_string())?;
    let mut added_count = 0;

    // Parse output and find SMB connections
    for line in output_str.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Check if it's a drive line (starts with letter colon)
        if line.len() >= 2 && line.chars().nth(1) == Some(':') {
            let drive = line[0..2].to_string();
            let rest = line[3..].trim();

            // Extract remote path (UNC path)
            if rest.starts_with("\\\\") {
                let parts: Vec<&str> = rest.split_whitespace().collect();
                if parts.is_empty() {
                    continue;
                }

                let unc_path = parts[0];
                // Parse UNC path: \\server\share[\path]
                let unc_parts: Vec<&str> = unc_path.split('\\').filter(|s| !s.is_empty()).collect();

                if unc_parts.len() >= 2 {
                    let server = unc_parts[0];
                    let share = unc_parts[1];

                    // Check if this connection already exists in config
                    let exists = config.connections.iter().any(|c| {
                        if let ConnectionType::SMB { host, share: s, .. } = &c.connection_type {
                            host == server && s == share
                        } else {
                            false
                        }
                    });

                    if !exists {
                        // Create new connection
                        let id = uuid::Uuid::new_v4().to_string()[..8].to_string();
                        let name = format!("{} ({})", share, server);

                        let conn = ConnectionConfig {
                            id: id.clone(),
                            name,
                            connection_type: ConnectionType::SMB {
                                host: server.to_string(),
                                port: 445,
                                share: share.to_string(),
                                path: String::new(),
                                username: String::new(),
                                password: None,
                            },
                            mount_point: Some(drive.clone()),
                            auto_mount: false,
                            enabled: true,
                        };

                        config.connections.push(conn);
                        added_count += 1;
                        info!("Auto-added SMB connection: {} -> \\\\{}\\{}", drive, server, share);
                    }
                }
            }
        }
    }

    // Save config if we added new connections
    if added_count > 0 {
        config.save().map_err(|e| e.to_string())?;
        info!("Added {} new SMB connections to config", added_count);
    }

    // Return updated connection list
    let connections = config.connections.iter().map(|c| {
        let (connection_type, host, username, share, remote_path) = match &c.connection_type {
            ConnectionType::WebDAV { url, username, .. } => ("webdav".to_string(), Some(url.clone()), Some(username.clone()), None, None),
            ConnectionType::SMB { host, share, path, username, .. } => ("smb".to_string(), Some(host.clone()), Some(username.clone()), Some(share.clone()), Some(path.clone())),
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
            share,
            remote_path,
        }
    }).collect();

    Ok(connections)
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
async fn auto_mount_connections(app_handle: tauri::AppHandle) -> Result<Vec<MountResult>, String> {
    info!("Auto mounting connections...");
    let _ = app_handle.emit("log-event", serde_json::json!({"level": "info", "message": "开始自动挂载连接..."}));

    let config = Config::load().map_err(|e| e.to_string())?;
    let mut results = Vec::new();

    for conn in config.connections.iter().filter(|c| c.auto_mount && !c.enabled) {
        let result = match mount_connection(conn.id.clone(), app_handle.clone()).await {
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

#[tauri::command]
fn get_mounted_connections() -> Result<Vec<ConnectionInfo>, String> {
    let config = Config::load().map_err(|e| e.to_string())?;

    let mounted: Vec<ConnectionInfo> = config.connections.iter()
        .filter(|c| c.enabled)
        .map(|c| {
            let (connection_type, host, username, share, remote_path) = match &c.connection_type {
                ConnectionType::WebDAV { url, username, .. } => ("webdav".to_string(), Some(url.clone()), Some(username.clone()), None, None),
                ConnectionType::SMB { host, share, path, username, .. } => ("smb".to_string(), Some(host.clone()), Some(username.clone()), Some(share.clone()), Some(path.clone())),
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
                share,
                remote_path,
            }
        })
        .collect();

    Ok(mounted)
}

#[tauri::command]
fn get_settings() -> Result<AppSettings, String> {
    let config = Config::load().map_err(|e| e.to_string())?;

    Ok(AppSettings {
        theme_mode: config.theme_mode,
        start_minimized: false,
        auto_start: config.start_on_boot,
        log_level: config.log_level,
    })
}

#[tauri::command]
fn save_settings(settings: AppSettings) -> Result<(), String> {
    let mut config = Config::load().map_err(|e| e.to_string())?;

    config.theme_mode = settings.theme_mode;
    config.start_on_boot = settings.auto_start;
    config.log_level = settings.log_level;

    config.save().map_err(|e| e.to_string())?;

    info!("Settings saved");
    Ok(())
}

fn find_available_drive(name: &str) -> Option<String> {
    // 获取已被系统占用的盘符
    let mut used: std::collections::HashSet<String> = std::collections::HashSet::new();
    for letter in b'A'..=b'Z' {
        let drive = format!("{}:", letter as char);
        let path = format!("{}\\", drive);
        if std::path::Path::new(&path).exists() {
            used.insert(drive);
        }
    }

    // 优先尝试使用名称的首字母作为盘符
    if !name.is_empty() {
        let first_char = name.chars().next().unwrap_or('A').to_ascii_uppercase();
        if first_char.is_ascii_alphabetic() {
            let drive = format!("{}:", first_char);
            if !used.contains(&drive) {
                return Some(drive);
            }
        }
    }

    // 从 Z 到 A 找第一个可用的
    for letter in (b'A'..=b'Z').rev() {
        let drive = format!("{}:", letter as char);
        if !used.contains(&drive) {
            return Some(drive);
        }
    }
    None
}

#[tauri::command]
fn get_available_drives() -> Result<Vec<String>, String> {
    // 获取可用盘符 - 返回未被系统占用的盘符
    // 先获取已使用的盘符
    let mut used_drives: std::collections::HashSet<String> = std::collections::HashSet::new();
    for letter in b'A'..=b'Z' {
        let drive = format!("{}:", letter as char);
        let path = format!("{}\\", drive);
        // 如果盘符存在，说明被占用
        if std::path::Path::new(&path).exists() {
            used_drives.insert(drive);
        }
    }

    // 从 Z 到 A 遍历，找出未被占用的盘符
    let mut found: Vec<String> = Vec::new();
    for letter in (b'A'..=b'Z').rev() {
        let drive = format!("{}:", letter as char);
        if !used_drives.contains(&drive) {
            found.push(drive);
        }
    }

    // 按字母顺序排序返回（A 到 Z）
    found.sort();
    Ok(found)
}

#[tauri::command]
fn open_folder(path: String) -> Result<(), String> {
    info!("Opening folder: {}", path);

    // 检查路径是否存在
    if !std::path::Path::new(&path).exists() {
        return Err("路径不存在，请先确认磁盘已正确挂载".to_string());
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("Failed to open folder: {}", e))?;
    }

    Ok(())
}

#[tauri::command]
fn get_connection_host_info(id: String) -> Result<String, String> {
    let config = Config::load().map_err(|e| e.to_string())?;

    let conn = config.connections.iter()
        .find(|c| c.id == id)
        .ok_or_else(|| "Connection not found".to_string())?;

    let host_info = match &conn.connection_type {
        ConnectionType::WebDAV { url, .. } => url.clone(),
        ConnectionType::SMB { host, port, share, path, .. } => {
            // 统一使用 \ 作为路径分隔符，处理 path 前导的 /
            let clean_path = path.trim_start_matches('/');
            if clean_path.is_empty() || clean_path == "." {
                format!("\\\\{}:{}\\{}", host, port, share)
            } else {
                format!("\\\\{}:{}\\{}\\{}", host, port, share, clean_path.replace('/', "\\"))
            }
        }
    };

    Ok(host_info)
}

/// 更新连接的全部信息（包括远端信息和盘符）
#[tauri::command]
fn update_connection_full(
    id: String,
    name: String,
    connection_type: String,
    host: String,
    share: Option<String>,
    remote_path: Option<String>,
    username: String,
    password: Option<String>,
    mount_point: Option<String>,
    auto_mount: Option<bool>,
) -> Result<(), String> {
    let mut config = Config::load().map_err(|e| e.to_string())?;

    if let Some(conn) = config.connections.iter_mut().find(|c| c.id == id) {
        // 更新密码
        if let Some(ref pwd) = password {
            if !pwd.is_empty() {
                if let Ok(cred_manager) = CredentialManager::new() {
                    let _ = cred_manager.store_for_connection(&id, &username, pwd);
                }
            }
        }

        conn.name = name;
        conn.auto_mount = auto_mount.unwrap_or(conn.auto_mount);
        conn.mount_point = mount_point;

        // 更新连接类型和远端信息
        match connection_type.as_str() {
            "webdav" => {
                conn.connection_type = ConnectionType::WebDAV {
                    url: host,
                    username,
                    password: None,
                };
            }
            "smb" => {
                // 解析 host 字符串 (支持 host:port 格式)
                let parts: Vec<&str> = host.split(':').collect();
                let (smb_host, smb_port) = if parts.len() >= 2 {
                    (parts[0].to_string(), parts[1].parse().unwrap_or(445))
                } else {
                    (host, 445)
                };

                conn.connection_type = ConnectionType::SMB {
                    host: smb_host,
                    port: smb_port,
                    share: share.unwrap_or_else(|| "share".to_string()),
                    path: remote_path.unwrap_or_else(|| "/".to_string()),
                    username,
                    password: None,
                };
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

async fn init_auto_mount(app_handle: tauri::AppHandle) {
    if let Ok(config) = Config::load() {
        for conn in config.connections.iter().filter(|c| c.auto_mount && !c.enabled) {
            info!("Auto-mounting connection: {} ({})", conn.name, conn.id);
            let _ = app_handle.emit("log-event", serde_json::json!({"level": "info", "message": format!("自动挂载: {}", conn.name)}));

            match mount_connection(conn.id.clone(), app_handle.clone()).await {
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
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("opennetdrive=info".parse().unwrap())
        )
        .init();

    info!("Starting openNetDrive...");

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            info!("Setting up application...");

            // 自动挂载之前保存的连接
            let app_handle = app.handle().clone();
            let runtime = tokio::runtime::Runtime::new().expect("Failed to create runtime");
            runtime.spawn(async move {
                init_auto_mount(app_handle).await;
            });

            // Create system tray menu
            let quit_item = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let show_item = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
            let hide_item = MenuItem::with_id(app, "hide", "隐藏窗口", true, None::<&str>)?;

            let menu = Menu::with_items(app, &[&show_item, &hide_item, &quit_item])?;

            // Create tray icon
            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().unwrap())
                .menu(&menu)
                .menu_on_left_click(false)
                .on_menu_event(|app, event| {
                    match event.id.as_ref() {
                        "quit" => {
                            info!("Quit requested from tray");
                            app.exit(0);
                        }
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.set_focus();
                            }
                        }
                        "hide" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.hide();
                            }
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            info!("System tray initialized");
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                // Hide window instead of closing (minimize to tray)
                let _ = window.hide();
                api.prevent_close();
                info!("Window hidden to tray");
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_connections,
            get_connection_details,
            add_connection,
            remove_connection,
            mount_connection,
            unmount_connection,
            update_connection,
            update_connection_full,
            auto_mount_connections,
            get_mounted_connections,
            get_settings,
            save_settings,
            get_available_drives,
            open_folder,
            get_connection_host_info,
            get_mounted_drives,
            sync_existing_connections
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
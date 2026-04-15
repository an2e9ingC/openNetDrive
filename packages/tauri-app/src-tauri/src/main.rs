// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use opennetdrive_core::{Config, ConnectionConfig, ConnectionType, WebDAVClient, mount_smb_share, CredentialManager};
use opennetdrive_mount_win::WinFspDriver;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, OnceLock};
use tokio::sync::RwLock;
use log::{info, error, warn, debug};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, WindowEvent,
};
use tauri::AppHandle;
use tracing_subscriber::{fmt, prelude::*, EnvFilter, registry, layer::SubscriberExt};

// 全局变量存储 app_handle，用于日志发送到 GUI
static APP_HANDLE: OnceLock<Arc<AppHandle>> = OnceLock::new();

fn get_app_handle() -> Option<Arc<AppHandle>> {
    APP_HANDLE.get().cloned()
}

// 自定义 tracing layer，将日志发送到 GUI
struct GuiLogLayer;

impl<S: tracing::Subscriber> tracing_subscriber::layer::Layer<S> for GuiLogLayer {
    fn on_event(&self, event: &tracing::Event, _ctx: tracing_subscriber::layer::Context<'_, S>) {
        let mut message = String::new();

        // 获取日志级别
        let level = match *event.metadata().level() {
            tracing::Level::ERROR => "error",
            tracing::Level::WARN => "warn",
            tracing::Level::DEBUG => "debug",
            _ => "info",
        };

        // 收集日志字段
        use std::fmt::Write;
        for field in event.fields() {
            write!(&mut message, "{} ", field).ok();
        }

        // 发送到 GUI
        if let Some(app_handle) = get_app_handle() {
            let _ = app_handle.emit("log-event", serde_json::json!({
                "level": level,
                "message": message.trim()
            }));
        }
    }
}

#[cfg(windows)]
fn set_network_drive_label(_drive_letter: &str, unc_path: &str, label: &str) -> Result<(), String> {
    use std::process::Command;

    // 解析 UNC 路径，提取 server 和 share
    // unc_path 格式: \\server\share 或 \\server\share\subfolder
    let unc_path_clean = unc_path.trim_end_matches('\\').trim_end_matches('/');
    debug!("[Registry] Input UNC path: '{}', cleaned: '{}'", unc_path, unc_path_clean);

    let parts: Vec<&str> = unc_path_clean.trim_start_matches("\\\\").split('\\').collect();

    debug!("[Registry] Parsed parts: {:?}", parts);

    if parts.len() < 2 {
        return Err("Invalid UNC path format".to_string());
    }

    let server = parts[0];
    let share = parts[1];

    // 系统创建的键名格式: ##server#share (如 ##NAS4MrLady#home_public)
    let server_share_key = format!("##{}#{}", server, share);
    let reg_path = format!(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\MountPoints2\{}", server_share_key);

    debug!("[Registry] Server: {}, Share: {}, Full key: {}", server, share, server_share_key);
    debug!("[Registry] Setting label at: {}, label: {}", reg_path, label);

    // 检查注册表键是否存在
    let check_output = Command::new("reg")
        .args(["query", &reg_path])
        .output()
        .map_err(|e| format!("Failed to query reg: {}", e))?;

    if !check_output.status.success() {
        // 键不存在，等待一下再试（系统可能还没创建）
        debug!("[Registry] Key not found, waiting and retrying...");
        std::thread::sleep(std::time::Duration::from_millis(500));

        let check_output = Command::new("reg")
            .args(["query", &reg_path])
            .output()
            .map_err(|e| format!("Failed to query reg: {}", e))?;

        if !check_output.status.success() {
            warn!("[Registry] Registry key does not exist: {}", server_share_key);
            return Err(format!("Registry key does not exist: {}", server_share_key));
        }
    }

    debug!("[Registry] Registry key exists, now setting value");

    // 如果标签值已存在，先删除旧值（确保系统能识别更新）
    let _ = Command::new("reg")
        .args(["delete", &reg_path, "/v", "_LabelFromDesktopINI", "/f"])
        .output();

    // 设置 _LabelFromDesktopINI 值
    let mut last_error = String::new();
    for attempt in 1..=3 {
        debug!("[Registry] Setting label, attempt {}", attempt);

        let output = Command::new("reg")
            .args(["add", &reg_path, "/v", "_LabelFromDesktopINI", "/t", "REG_SZ", "/d", label, "/f"])
            .output()
            .map_err(|e| format!("Failed to execute reg: {}", e))?;

        if output.status.success() {
            // 第三步：验证设置是否成功
            debug!("[Registry] Setting success, verifying...");

            let verify_output = Command::new("reg")
                .args(["query", &reg_path, "/v", "_LabelFromDesktopINI"])
                .output()
                .map_err(|e| format!("Failed to verify reg: {}", e))?;

            if verify_output.status.success() {
                let stdout = String::from_utf8_lossy(&verify_output.stdout);
                debug!("[Registry] Verify output: {}", stdout.trim());
                if stdout.contains(label) {
                    debug!("[Registry] Verification passed: label set to {}", label);
                    return Ok(());
                } else {
                    warn!("[Registry] Verification failed: expected '{}' but output is: {}", label, stdout.trim());
                    last_error = format!("Verification failed: expected '{}'", label);
                }
            } else {
                let stderr = String::from_utf8_lossy(&verify_output.stderr);
                warn!("[Registry] Verification failed: could not read value, stderr: {}", stderr);
                last_error = "Verification failed: could not read value".to_string();
            }
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            last_error = stderr.to_string();
            warn!("[Registry] Setting failed: {}", last_error);
        }

        // 等待后重试
        if attempt < 3 {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }

    Err(format!("Failed to set label after 3 attempts: {}", last_error))
}

#[cfg(windows)]
fn clear_network_drive_label(_drive_letter: &str, unc_path: &str) -> Result<(), String> {
    use std::process::Command;

    // 解析 UNC 路径
    let unc_path_clean = unc_path.trim_end_matches('\\').trim_end_matches('/');
    let parts: Vec<&str> = unc_path_clean.trim_start_matches("\\\\").split('\\').collect();

    if parts.len() < 2 {
        return Err("Invalid UNC path format".to_string());
    }

    let server = parts[0];
    let share = parts[1];

    // 系统创建的键名格式: ##server#share
    let server_share_key = format!("##{}#{}", server, share);
    let reg_path = format!(r"HKCU\Software\Microsoft\Windows\CurrentVersion\Explorer\MountPoints2\{}", server_share_key);

    debug!("[Registry] Clearing label at: {}", reg_path);

    // 删除整个注册表键（包含 _LabelFromDesktopINI 值）
    let output = Command::new("reg")
        .args(["delete", &reg_path, "/f"])
        .output()
        .map_err(|e| format!("Failed to execute reg: {}", e))?;

    if output.status.success() {
        debug!("[Registry] Label key deleted successfully");
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    if stderr.contains("unable to find") || stderr.contains("系统找不到") || stderr.contains("The system was unable to find") {
        debug!("[Registry] Key already deleted or not found");
        return Ok(());
    }

    warn!("[Registry] Failed to delete label: {}", stderr);
    Ok(())
}

#[cfg(not(windows))]
fn set_network_drive_label(_drive_letter: &str, _unc_path: &str, _label: &str) -> Result<(), String> {
    Ok(())
}

#[cfg(not(windows))]
fn clear_network_drive_label(_drive_letter: &str, _unc_path: &str) -> Result<(), String> {
    Ok(())
}

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
    pub has_password: bool,          // 是否已保存密码
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
fn get_app_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[tauri::command]
fn get_connections() -> Result<Vec<ConnectionInfo>, String> {
    let config = Config::load().map_err(|e| e.to_string())?;

    debug!("[GetConnections] Returning {} connections", config.connections.len());
    let connections = config.connections.iter().map(|c| {
        debug!("[GetConnections] Connection {}: enabled={}, mount_point={:?}", c.name, c.enabled, c.mount_point);
        let (connection_type, host, username, share, remote_path, has_password) = match &c.connection_type {
            ConnectionType::WebDAV { url, username, password, .. } => ("webdav".to_string(), Some(url.clone()), Some(username.clone()), None, None, password.is_some()),
            ConnectionType::SMB { host, share, path, username, password, .. } => ("smb".to_string(), Some(host.clone()), Some(username.clone()), Some(share.clone()), Some(path.clone()), password.is_some()),
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
            has_password,
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

    let (connection_type, host, username, share, remote_path, has_password) = match &conn.connection_type {
        ConnectionType::WebDAV { url, username, password, .. } => ("webdav".to_string(), Some(url.clone()), Some(username.clone()), None, None, password.is_some()),
        ConnectionType::SMB { host, share, path, username, password, .. } => ("smb".to_string(), Some(host.clone()), Some(username.clone()), Some(share.clone()), Some(path.clone()), password.is_some()),
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
        has_password,
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
    mount_point: Option<String>,
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
        "smb" => {
            // 解析 host 字符串 (支持 host:port 格式)
            let parts: Vec<&str> = host.split(':').collect();
            let (smb_host, smb_port) = if parts.len() >= 2 {
                (parts[0].to_string(), parts[1].parse().unwrap_or(445))
            } else {
                (host, 445)
            };
            ConnectionType::SMB {
                host: smb_host,
                port: smb_port,
                share: share.unwrap_or_else(|| "share".to_string()),
                path: remote_path.unwrap_or_else(|| "/".to_string()),
                username,
                password: None,
            }
        }
        _ => return Err("Invalid connection type".to_string()),
    };

    let conn = ConnectionConfig {
        id: id.clone(),
        name,
        connection_type: conn_type,
        mount_point,
        auto_mount: auto_mount.unwrap_or(false),
        enabled: false,
    };

    config.add_connection(conn);
    config.save().map_err(|e| e.to_string())?;

    info!("Added connection: {}", id);
    Ok(id)
}

#[tauri::command]
fn remove_connection(id: String, app_handle: tauri::AppHandle) -> Result<(), String> {
    info!("[Remove] Starting to remove connection: {}", id);
    let _ = app_handle.emit("log-event", serde_json::json!({"level": "info", "message": format!("Starting to remove connection: {}", id)}));

    let mut config = Config::load().map_err(|e| e.to_string())?;

    // Get connection to remove credentials and cleanup registry
    if let Some(conn) = config.get_connection(&id) {
        info!("[Remove] Found connection: name={}, enabled={}, mount_point={:?}", conn.name, conn.enabled, conn.mount_point);
        let _ = app_handle.emit("log-event", serde_json::json!({"level": "info", "message": format!("Found connection: {}, enabled={}", conn.name, conn.enabled)}));

        // 清理注册表中的驱动器标签 (如果是 SMB 连接且已挂载)
        if let (ConnectionType::SMB { host, share, .. }, Some(ref mount_point)) = (&conn.connection_type, &conn.mount_point) {
            let drive = mount_point.trim_end_matches('\\').trim_end_matches(':');
            let drive_with_colon = format!("{}:", drive.to_uppercase());
            let host_for_reg = host.split(':').next().unwrap_or(host);
            let unc_path = format!(r"\\{}\{}", host_for_reg, share);
            info!("[Remove] Clearing registry for drive: {}, UNC: {}", drive_with_colon, unc_path);
            let _ = clear_network_drive_label(&drive_with_colon, &unc_path);
            info!("[Remove] Registry cleanup completed");
            let _ = app_handle.emit("log-event", serde_json::json!({"level": "info", "message": "Registry cleanup done"}));
        } else {
            info!("[Remove] No mount_point found, skipping registry cleanup");
            let _ = app_handle.emit("log-event", serde_json::json!({"level": "info", "message": "No mount point, skipping registry cleanup"}));
        }

        let username = match &conn.connection_type {
            ConnectionType::WebDAV { username, .. } => username,
            ConnectionType::SMB { username, .. } => username,
        };

        if let Ok(cred_manager) = CredentialManager::new() {
            let _ = cred_manager.delete_for_connection(&id, username);
            info!("[Remove] Credentials deleted");
            let _ = app_handle.emit("log-event", serde_json::json!({"level": "info", "message": "Credentials deleted"}));
        }
    }

    if config.remove_connection(&id).is_some() {
        config.save().map_err(|e| e.to_string())?;
        info!("[Remove] Connection {} removed successfully", id);
        let _ = app_handle.emit("log-event", serde_json::json!({"level": "success", "message": format!("Connection removed: {}", id)}));
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

                // 设置网络驱动器名称（通过注册表）- 修改系统创建的 ##server#share 键
                // 需要从host中去除端口号，因为注册表键名不包含端口
                let host_for_reg = host.split(':').next().unwrap_or(host);
                let unc_path = format!(r"\\{}\{}", host_for_reg, share);
                debug!("Setting network drive label for UNC: {}, label: {}", unc_path, conn.name);
                if let Err(e) = set_network_drive_label(&mount_point, &unc_path, &conn.name) {
                    warn!("Failed to set network drive label: {}", e);
                } else {
                    debug!("Network drive label set successfully");
                }

                // 保存挂载状态
                let mut config = Config::load().map_err(|e| e.to_string())?;
                if let Some(c) = config.connections.iter_mut().find(|c| c.id == id) {
                    c.enabled = true;
                    c.mount_point = Some(mount_point.clone());
                    info!("[Mount] Setting connection {} enabled=true, mount_point={}", id, mount_point);
                }
                config.save().map_err(|e| e.to_string())?;
                info!("[Mount] Config saved, connection {} now enabled", id);

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
    debug!("[Unmount] Starting unmount for id: {}", id);
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
    debug!("[Unmount] Current net use output:\n{}", output_str);

    // 尝试用 ID 作为驱动器号断开
    // 假设 id 可能是驱动器号如 "Z" 或完整名称

    // 如果 ID 看起来像驱动器号，尝试断开它
    if id.len() == 1 && id.chars().next().map(|c| c.is_alphabetic()).unwrap_or(false) {
        let drive_with_colon = format!("{}:", id.to_uppercase());
        debug!("[Unmount] ID looks like drive letter: {}", drive_with_colon);
        info!("Trying to unmount drive: {}", drive_with_colon);

        let output = std::process::Command::new("net")
            .args(["use", &drive_with_colon, "/delete", "/y"])
            .output()
            .map_err(|e| format!("Failed to execute net use: {}", e))?;

        if output.status.success() {
            debug!("[Unmount] Successfully unmounted drive via net use");
            info!("Successfully unmounted drive {}", drive_with_colon);
            unmounted = true;
        } else {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            debug!("[Unmount] net use failed: stdout={}, stderr={}", stdout, stderr);
            info!("net use result: {} {}", stdout, stderr);
        }
    } else if let Ok(config) = config_result {
        // 从配置中查找连接
        if let Some(conn) = config.connections.iter().find(|c| c.id == id) {
            debug!("[Unmount] Found connection in config: name={}, type={:?}, mount_point={:?}", conn.name, conn.connection_type, conn.mount_point);
            info!("Found connection in config: {} mount_point: {:?}", conn.name, conn.mount_point);

            // 保存驱动器号和UNC路径供后续使用（在更新配置之前）
            let drive_to_clear: Option<(String, String)> = if let (ConnectionType::SMB { host, share, .. }, Some(ref mount_point)) = (&conn.connection_type, &conn.mount_point) {
                let drive = mount_point.trim_end_matches('\\').trim_end_matches(':');
                let drive_str = format!("{}:", drive.to_uppercase());
                let host_for_reg = host.split(':').next().unwrap_or(host);
                let unc_path = format!(r"\\{}\{}", host_for_reg, share);
                debug!("[Unmount] Will clear registry for drive: {}, UNC: {}", drive_str, unc_path);
                Some((drive_str, unc_path))
            } else {
                debug!("[Unmount] Not SMB or no mount_point, skipping registry clear");
                None
            };

            if let Some(ref mount_point) = conn.mount_point {
                let drive = mount_point.trim_end_matches('\\').trim_end_matches(':');
                let drive_with_colon = format!("{}:", drive.to_uppercase());

                debug!("[Unmount] Attempting to unmount drive: {}", drive_with_colon);
                info!("Trying to unmount: {}", drive_with_colon);

                let output = std::process::Command::new("net")
                    .args(["use", &drive_with_colon, "/delete", "/y"])
                    .output()
                    .map_err(|e| format!("Failed to execute net use: {}", e))?;

                if output.status.success() {
                    debug!("[Unmount] net use success, now clearing registry");
                    info!("Successfully unmounted drive {}", drive_with_colon);
                    unmounted = true;

                    // 清理注册表中的驱动器标签（在更新配置之前）
                    if let Some((ref drive, ref unc_path)) = drive_to_clear {
                        debug!("[Unmount] Calling clear_network_drive_label for: {}, UNC: {}", drive, unc_path);
                        let _ = clear_network_drive_label(drive, unc_path);
                        debug!("[Unmount] clear_network_drive_label returned");
                    }
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

    // 构建当前实际挂载的驱动器映射: drive -> UNC path
    let mut current_mounts: std::collections::HashMap<String, String> = std::collections::HashMap::new();
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
                if !parts.is_empty() {
                    // 去掉冒号，只存储驱动器字母
                    let drive_letter = drive.trim_end_matches(':').to_uppercase();
                    current_mounts.insert(drive_letter, parts[0].to_string());
                }
            }
        }
    }

    // 检查现有连接的实际挂载状态
    let mut updated = false;
    debug!("[Sync] Checking {} existing connections, current_mounts: {:?}", config.connections.len(), current_mounts);
    for conn in config.connections.iter_mut() {
        let is_actually_mounted = if let Some(ref mount_point) = conn.mount_point {
            let drive = mount_point.trim_end_matches('\\').trim_end_matches(':').to_uppercase();
            let mounted = current_mounts.contains_key(&drive);
            debug!("[Sync] Connection {}: mount_point={}, drive={}, is_actually_mounted={}, current_enabled={}",
                conn.name, mount_point, drive, mounted, conn.enabled);
            mounted
        } else {
            debug!("[Sync] Connection {}: no mount_point, current_enabled={}", conn.name, conn.enabled);
            false
        };

        // 如果配置显示已挂载，但实际未挂载，则更新状态
        if conn.enabled && !is_actually_mounted {
            info!("Connection {} appears to be disconnected, updating enabled status", conn.name);
            conn.enabled = false;
            updated = true;
        }
        // 如果配置显示未挂载，但实际已挂载，则更新状态
        else if !conn.enabled && is_actually_mounted {
            info!("Connection {} is actually mounted, updating enabled status", conn.name);
            conn.enabled = true;
            updated = true;
        }
    }

    // Save config if we added new connections or updated existing ones
    if added_count > 0 || updated {
        config.save().map_err(|e| e.to_string())?;
        if added_count > 0 {
            info!("Added {} new SMB connections to config", added_count);
        }
        if updated {
            info!("Updated connection enabled status based on actual net use");
        }
    }

    // Return updated connection list
    let connections = config.connections.iter().map(|c| {
        let (connection_type, host, username, share, remote_path, has_password) = match &c.connection_type {
            ConnectionType::WebDAV { url, username, password, .. } => ("webdav".to_string(), Some(url.clone()), Some(username.clone()), None, None, password.is_some()),
            ConnectionType::SMB { host, share, path, username, password, .. } => ("smb".to_string(), Some(host.clone()), Some(username.clone()), Some(share.clone()), Some(path.clone()), password.is_some()),
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
            has_password,
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
            let (connection_type, host, username, share, remote_path, has_password) = match &c.connection_type {
                ConnectionType::WebDAV { url, username, password, .. } => ("webdav".to_string(), Some(url.clone()), Some(username.clone()), None, None, password.is_some()),
                ConnectionType::SMB { host, share, path, username, password, .. } => ("smb".to_string(), Some(host.clone()), Some(username.clone()), Some(share.clone()), Some(path.clone()), password.is_some()),
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
                has_password,
            }
        })
        .collect();

    Ok(mounted)
}

#[tauri::command]
fn emit_log(level: String, message: String, app_handle: AppHandle) -> Result<(), String> {
    // 将前端日志发送到 GUI
    let _ = app_handle.emit("log-event", serde_json::json!({
        "level": level,
        "message": message
    }));

    // 同时输出到 tracing 日志
    match level.as_str() {
        "error" => error!("[Frontend] {}", message),
        "warn" => warn!("[Frontend] {}", message),
        "info" => info!("[Frontend] {}", message),
        _ => debug!("[Frontend] {}", message),
    }

    Ok(())
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
fn get_log_file_path() -> Result<String, String> {
    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("openNetDrive")
        .join("logs");

    // 获取最新的日志文件
    let log_file = std::fs::read_dir(&log_dir)
        .ok()
        .and_then(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map_or(false, |ext| ext == "log"))
                .max_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()))
                .map(|e| e.path())
        });

    match log_file {
        Some(path) => Ok(path.to_string_lossy().to_string()),
        None => Err("日志文件不存在".to_string()),
    }
}

#[tauri::command]
fn open_log_file() -> Result<(), String> {
    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("openNetDrive")
        .join("logs");

    // 检查目录是否存在
    if !log_dir.exists() {
        return Err("日志目录不存在".to_string());
    }

    // 获取最新的日志文件
    let log_file = std::fs::read_dir(&log_dir)
        .ok()
        .and_then(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map_or(false, |ext| ext == "log"))
                .max_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()))
                .map(|e| e.path())
        });

    match log_file {
        Some(path) => {
            let path_str = path.to_string_lossy().to_string();
            info!("[open_log_file] 打开日志文件: {}", path_str);

            // 用 PowerShell Start-Process 打开日志文件（避免 os error 50）
            std::process::Command::new("powershell")
                .args(["-NoProfile", "-Command", &format!("Start-Process notepad.exe '{}'", path_str)])
                .spawn()
                .map_err(|e| format!("打开日志文件失败: {}", e))?;

            Ok(())
        }
        None => Err("未找到日志文件".to_string()),
    }
}

#[tauri::command]
fn clean_log_files() -> Result<u32, String> {
    let log_dir = dirs::data_local_dir()
        .ok_or("无法获取本地数据目录")?
        .join("openNetDrive")
        .join("logs");

    if !log_dir.exists() {
        return Ok(0);
    }

    let mut deleted_count = 0;
    let entries = std::fs::read_dir(&log_dir)
        .map_err(|e| format!("无法读取日志目录: {}", e))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "log" {
                    // 不删除今天的日志文件
                    let file_name = path.file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("");
                    let today = chrono::Local::now().format("%Y%m%d").to_string();
                    if !file_name.contains(&today) {
                        if std::fs::remove_file(&path).is_ok() {
                            deleted_count += 1;
                            info!("[clean_log_files] 已删除: {:?}", path);
                        }
                    }
                }
            }
        }
    }

    Ok(deleted_count)
}

#[tauri::command]
fn minimize_to_tray(window: tauri::Window) -> Result<(), String> {
    window.hide().map_err(|e| e.to_string())?;
    info!("[minimize_to_tray] 窗口已隐藏到托盘");
    Ok(())
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) -> Result<(), String> {
    info!("[quit_app] 退出应用");
    app.exit(0);
    Ok(())
}

#[tauri::command]
fn unmount_all_connections(app: tauri::AppHandle) -> Result<u32, String> {
    info!("[unmount_all_connections] 开始卸载所有连接");
    let _ = app.emit("log-event", serde_json::json!({"level": "info", "message": "正在断开所有连接..."}));

    // 获取当前已挂载的网络驱动器
    let output = std::process::Command::new("net")
        .args(["use"])
        .output()
        .map_err(|e| format!("获取网络驱动器列表失败: {}", e))?;

    let output_str = String::from_utf8_lossy(&output.stdout);
    let mut unmounted_count = 0;

    // 解析输出，找到所有网络驱动器
    for line in output_str.lines() {
        // 查找类似 "Z:        \\192.168.1.100\share   Microsoft Windows Network" 的行
        if line.contains("Microsoft Windows Network") || line.contains("WebDAV") {
            if let Some(drive) = line.chars().next() {
                if drive.is_alphabetic() {
                    let drive_with_colon = format!("{}:", drive);
                    info!("正在断开驱动器: {}", drive_with_colon);

                    let result = std::process::Command::new("net")
                        .args(["use", &drive_with_colon, "/delete", "/y"])
                        .output();

                    if let Ok(out) = result {
                        if out.status.success() {
                            unmounted_count += 1;
                            info!("已断开驱动器: {}", drive_with_colon);
                        }
                    }
                }
            }
        }
    }

    info!("[unmount_all_connections] 共断开 {} 个驱动器", unmounted_count);
    let _ = app.emit("log-event", serde_json::json!({"level": "info", "message": format!("已断开 {} 个驱动器", unmounted_count)}));
    Ok(unmounted_count)
}

#[tauri::command]
fn open_folder(path: String) -> Result<(), String> {
    info!("[open_folder] 收到请求: {}", path);

    #[cfg(target_os = "windows")]
    {
        // 方法1: 使用 PowerShell Start-Process（最可靠）
        let result = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &format!("Start-Process explorer '{}'", path)])
            .spawn();

        if let Ok(child) = result {
            info!("[open_folder] PowerShell 成功: pid={:?}", child.id());
            return Ok(());
        }

        // 方法2: 直接用 explorer
        let result2 = std::process::Command::new("explorer")
            .arg(&path)
            .spawn();

        if let Ok(child) = result2 {
            info!("[open_folder] explorer 成功: pid={:?}", child.id());
            return Ok(());
        }

        // 方法3: 使用 cmd /c start
        let result3 = std::process::Command::new("cmd")
            .args(["/c", "start", "", &path])
            .spawn();

        if let Ok(child) = result3 {
            info!("[open_folder] cmd /c start 成功: pid={:?}", child.id());
            return Ok(());
        }

        // 所有方法都失败
        return Err("打开资源管理器失败，请手动打开".to_string());
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

    // 首先提取需要更新注册表的信息（在修改 config 之前）
    let should_update_registry = config.connections
        .iter()
        .find(|c| c.id == id)
        .map(|c| c.enabled)
        .unwrap_or(false);

    let (old_host, old_share, old_mount_point) = if should_update_registry {
        if let Some(conn) = config.connections.iter().find(|c| c.id == id) {
            if let ConnectionType::SMB { ref host, ref share, .. } = conn.connection_type {
                (Some(host.clone()), Some(share.clone()), conn.mount_point.clone())
            } else {
                (None, None, None)
            }
        } else {
            (None, None, None)
        }
    } else {
        (None, None, None)
    };

    if let Some(conn) = config.connections.iter_mut().find(|c| c.id == id) {
        // 更新密码
        if let Some(ref pwd) = password {
            if pwd == "**REMOVE_PASSWORD**" {
                // 删除保存的密码
                if let Ok(cred_manager) = CredentialManager::new() {
                    let _ = cred_manager.delete_for_connection(&id, &username);
                }
            } else if !pwd.is_empty() {
                // 保存新密码
                if let Ok(cred_manager) = CredentialManager::new() {
                    let _ = cred_manager.store_for_connection(&id, &username, pwd);
                }
            }
        }

        // 保存新的名称
        let new_name = name;

        conn.name = new_name.clone();
        conn.auto_mount = auto_mount.unwrap_or(conn.auto_mount);
        conn.mount_point = mount_point;

        // 保存更新后的连接信息用于后续注册表操作（在修改 connection_type 之前）
        let new_mount_point = conn.mount_point.clone();

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

        // 保存新的 host/share（在更新 connection_type 之后）
        let (new_host, new_share) = if let ConnectionType::SMB { ref host, ref share, .. } = conn.connection_type {
            (Some(host.clone()), Some(share.clone()))
        } else {
            (None, None)
        };

        config.save().map_err(|e| e.to_string())?;

        // 如果连接已挂载且为SMB，更新注册表中的驱动器标签
        if should_update_registry {
            // 先删除旧的注册表项（如果 host/share 改变了）
            if let (Some(ref old_mp), Some(ref old_h), Some(ref old_s)) = (&old_mount_point, &old_host, &old_share) {
                let old_drive = old_mp.trim_end_matches('\\').trim_end_matches(':');
                let old_drive_str = format!("{}:", old_drive.to_uppercase());
                let old_unc_path = format!(r"\\{}\{}", old_h, old_s);

                if let (Some(ref nh), Some(ref ns)) = (&new_host, &new_share) {
                    if old_h != nh || old_s != ns {
                        info!("[Registry] Host/share changed from {}/{} to {}/{}, clearing old registry entry",
                            old_h, old_s, nh, ns);
                        let _ = clear_network_drive_label(&old_drive_str, &old_unc_path);
                    }
                }
            }

            // 设置新的注册表项
            if let (Some(ref mount_point), Some(ref host), Some(ref share)) = (&new_mount_point, &new_host, &new_share) {
                let drive = mount_point.trim_end_matches('\\').trim_end_matches(':');
                let drive_with_colon = format!("{}:", drive.to_uppercase());
                let host_for_reg = host.split(':').next().unwrap_or(host);
                let unc_path = format!(r"\\{}\{}", host_for_reg, share);
                info!("Setting network drive label for connected SMB: {} -> {}", new_name, unc_path);
                let _ = set_network_drive_label(&drive_with_colon, &unc_path, &new_name);
            }
        }

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

// 崩溃上报配置 - 用户可以在环境中设置 GITHUB_TOKEN 和 GITHUB_REPO
const GITHUB_OWNER: &str = "xuchuan";
const GITHUB_REPO: &str = "openNetDrive";

/// 上报崩溃信息到 GitHub
fn report_crash_to_github(panic_info: &str) {
    // 检查是否配置了 GitHub Token
    let github_token = std::env::var("GITHUB_TOKEN").ok();
    if github_token.is_none() {
        log::warn!("GITHUB_TOKEN not set, skipping crash report");
        return;
    }
    let token = github_token.unwrap();

    // 构建 issue 内容
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let os_info = std::env::consts::OS;
    let arch = std::env::consts::ARCH;

    let title = format!("[Auto-Crash] {} - {}", timestamp, "openNetDrive");

    let body = format!(r#"## 崩溃信息

**时间:** {}
**操作系统:** {} ({})
**版本:** 0.1.0

### 崩溃详情

```
{}
```

### 系统信息

- OS: {}
- Architecture: {}

---
*此 issue 由 openNetDrive 自动创建*
"#, timestamp, os_info, arch, panic_info, os_info, arch);

    // 发送创建 issue 的请求
    let client = reqwest::blocking::Client::new();
    let url = format!("https://api.github.com/repos/{}/{}/issues", GITHUB_OWNER, GITHUB_REPO);

    let response = client.post(&url)
        .header("Authorization", format!("Bearer {}", token))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "openNetDrive/0.1.0")
        .json(&serde_json::json!({
            "title": title,
            "body": body,
            "labels": ["crash", "auto-reported"]
        }))
        .send();

    match response {
        Ok(resp) if resp.status().is_success() => {
            log::info!("Crash report submitted to GitHub successfully");
        }
        Ok(resp) => {
            let status = resp.status();
            let text = resp.text().unwrap_or_default();
            log::warn!("Failed to submit crash report: {} - {}", status, text);
        }
        Err(e) => {
            log::warn!("Failed to submit crash report: {}", e);
        }
    }

    // 同时保存崩溃日志到本地文件
    if let Some(data_dir) = dirs::data_local_dir() {
        let log_dir = data_dir.join("openNetDrive").join("crash_logs");
        let _ = std::fs::create_dir_all(&log_dir);
        let log_file = log_dir.join(format!("crash_{}.log", chrono::Local::now().format("%Y%m%d_%H%M%S")));
        let content = format!("=== Crash Report ===\nTime: {}\nOS: {} ({})\n\n{}\n",
            timestamp, os_info, arch, panic_info);
        let _ = std::fs::write(&log_file, content);
        log::info!("Crash log saved to: {:?}", log_file);
    }
}

/// 设置全局 panic hook 来捕获未处理的崩溃
fn setup_panic_handler() {
    std::panic::set_hook(Box::new(|panic_info| {
        let message = if let Some(s) = panic_info.payload().downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = panic_info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Unknown panic".to_string()
        };

        let location = if let Some(loc) = panic_info.location() {
            format!("{}:{}:{}", loc.file(), loc.line(), loc.column())
        } else {
            "unknown location".to_string()
        };

        let panic_msg = format!("PANIC at {}: {}", location, message);

        log::error!("{}", panic_msg);

        // 上报到 GitHub
        report_crash_to_github(&panic_msg);
    }));
}

fn main() {
    // 设置 panic handler
    setup_panic_handler();

    // 获取日志文件路径
    let log_dir = dirs::data_local_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("openNetDrive")
        .join("logs");

    // 创建日志目录
    let _ = std::fs::create_dir_all(&log_dir);

    // 生成日志文件名（带日期和时间，每次启动独立文件）
    let log_file = log_dir.join(format!("opennetdrive_{}.log", chrono::Local::now().format("%Y%m%d_%H%M%S")));

    // 创建文件 writer
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
        .expect("Failed to open log file");

    let file_layer = fmt::layer()
        .with_ansi(false)
        .with_writer(std::sync::Mutex::new(file))
        .with_target(true)
        .with_level(true)
        .with_thread_ids(true);

    // 初始化日志系统，添加自定义 layer 发送到 GUI
    let gui_layer = GuiLogLayer;
    registry()
        .with(fmt::layer().with_target(true).with_level(true))
        .with(file_layer)
        .with(gui_layer)
        .with(EnvFilter::from_default_env()
            .add_directive("opennetdrive=debug".parse().unwrap()))
        .init();

    info!("Starting openNetDrive...");
    info!("Log file: {:?}", log_file);

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            info!("Setting up application...");

            // 保存 app_handle 到全局变量，供日志 layer 使用
            let _ = APP_HANDLE.set(Arc::new(app.handle().clone()));

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
                .show_menu_on_left_click(false)
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
            match event {
                WindowEvent::CloseRequested { api, .. } => {
                    // 点击关闭按钮时，隐藏窗口到托盘（而不是退出）
                    let _ = window.hide();
                    api.prevent_close();
                    info!("Close requested - window hidden to tray");
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_app_version,
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
            sync_existing_connections,
            emit_log,
            get_log_file_path,
            open_log_file,
            clean_log_files,
            minimize_to_tray,
            quit_app,
            unmount_all_connections
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
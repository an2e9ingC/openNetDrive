//! SMB protocol implementation
//!
//! Uses Windows native SMB support via command line for connection testing

use crate::error::{Error, Result};
use crate::protocol::{FileEntry, Protocol};
use async_trait::async_trait;
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::process::Command;
use std::sync::Arc;
use tokio::sync::Mutex;

/// SMB client configuration
#[derive(Debug, Clone)]
pub struct SMBConfig {
    pub host: String,
    pub port: u16,
    pub share: String,
    pub path: String,
    pub username: String,
    pub password: Option<String>,
}

/// SMB client state
#[derive(Debug, Clone)]
struct SMBState {
    connected: bool,
    server: String,
    mount_success: bool,
}

/// SMB Protocol Client
pub struct SMBClient {
    config: SMBConfig,
    state: Arc<Mutex<SMBState>>,
    // In-memory file cache
    file_cache: Arc<Mutex<HashMap<String, Vec<u8>>>>,
    #[cfg(windows)]
    drive_letter: Arc<Mutex<Option<String>>>,
}

impl SMBClient {
    /// Create a new SMB client
    pub fn new(config: SMBConfig) -> Self {
        Self {
            config,
            state: Arc::new(Mutex::new(SMBState {
                connected: false,
                server: String::new(),
                mount_success: false,
            })),
            file_cache: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(windows)]
            drive_letter: Arc::new(Mutex::new(None)),
        }
    }

    /// Build UNC path
    fn build_unc_path(&self) -> String {
        let path = if self.config.path.is_empty() || self.config.path == "/" {
            String::new()
        } else {
            let clean_path = self.config.path.trim_start_matches('/').replace('/', "\\");
            format!("\\{}", clean_path)
        };
        format!("\\\\{host}\\{share}{path}",
            host = self.config.host,
            share = self.config.share,
            path = path
        )
    }

    /// Test SMB connection by pinging the server and trying to access it
    #[cfg(windows)]
    async fn test_smb_connection(&self) -> Result<bool> {
        let unc_path = self.build_unc_path();
        info!("Testing SMB connection to: {}", unc_path);

        // First, try to ping the server to check if it's reachable
        let ping_output = Command::new("ping")
            .args(["-n", "1", "-w", "1000", &self.config.host])
            .output();

        match ping_output {
            Ok(output) => {
                if !output.status.success() {
                    info!("Server {} is not reachable (ping failed)", self.config.host);
                    return Ok(false);
                }
            }
            Err(e) => {
                warn!("Failed to ping server: {}", e);
                return Ok(false);
            }
        }

        // Try to access the share using net use
        // First, check if any existing connection exists
        let list_output = Command::new("net")
            .args(["use"])
            .output();

        if let Ok(output) = list_output {
            let output_str = String::from_utf8_lossy(&output.stdout);
            // Check if already connected to this share
            if output_str.contains(&self.config.host) && output_str.contains(&self.config.share) {
                info!("Already connected to the share");
                return Ok(true);
            }
        }

        // Try to map the drive temporarily to test connection
        // Use a temporary drive letter for testing
        let test_drive = "Z:";

        let mut cmd = Command::new("net");
        let mut args = vec![
            "use".to_string(),
            test_drive.to_string(),
            unc_path.clone(),
        ];

        // Add credentials if provided
        if !self.config.username.is_empty() {
            args.push("/user:".to_string());
            if self.config.username.contains('\\') || self.config.username.contains('@') {
                args.push(self.config.username.clone());
            } else {
                args.push(format!("{}\\{}", self.config.host, self.config.username));
            }
            if let Some(ref password) = self.config.password {
                args.push(password.clone());
            }
        }

        let output = cmd.args(&args).output();

        match output {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                if output.status.success() {
                    info!("SMB connection test successful");

                    // Clean up the test connection
                    let _ = Command::new("net")
                        .args(["use", test_drive, "/delete", "/y"])
                        .output();

                    Ok(true)
                } else {
                    // Check for common error messages
                    if stderr.contains("The network name cannot be found") ||
                       stderr.contains("The specified network name is no longer available") ||
                       stdout.contains("The network name cannot be found") {
                        error!("Share not found or access denied: {}", unc_path);
                        Ok(false)
                    } else if stderr.contains("Access is denied") ||
                              stdout.contains("Access is denied") {
                        error!("Access denied to share: {}", unc_path);
                        Ok(false)
                    } else {
                        warn!("SMB connection test failed: {} {}", stdout, stderr);
                        Ok(false)
                    }
                }
            }
            Err(e) => {
                error!("Failed to run net use command: {}", e);
                Ok(false)
            }
        }
    }

    #[cfg(not(windows))]
    async fn test_smb_connection(&self) -> Result<bool> {
        // On non-Windows, just simulate connection for now
        warn!("SMB connection test not implemented for this platform");
        Ok(false)
    }

    /// Try to mount the SMB share using net use
    #[cfg(windows)]
    async fn mount_share(&self, drive: &str) -> Result<bool> {
        let unc_path = self.build_unc_path();
        info!("Attempting to mount SMB share to {}: {}", drive, unc_path);

        // Check if drive is already in use
        let list_output = Command::new("net")
            .args(["use"])
            .output();

        if let Ok(output) = list_output {
            let output_str = String::from_utf8_lossy(&output.stdout);
            if output_str.contains(&format!("{}:", drive.trim_end_matches(':'))) {
                info!("Drive {} is already in use", drive);
                return Ok(false);
            }
        }

        // Try to map the drive
        let mut cmd = Command::new("net");
        let mut args = vec![
            "use".to_string(),
            drive.to_string(),
            unc_path.clone(),
        ];

        if !self.config.username.is_empty() {
            args.push("/user:".to_string());
            if self.config.username.contains('\\') || self.config.username.contains('@') {
                args.push(self.config.username.clone());
            } else {
                args.push(format!("{}\\{}", self.config.host, self.config.username));
            }
            if let Some(ref password) = self.config.password {
                args.push(password.clone());
            }
        }

        // Add persistent option
        args.push("/persistent:yes".to_string());

        let output = cmd.args(&args).output()
            .map_err(|e| Error::Connection(format!("Failed to execute net use: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            info!("Successfully mounted {} to {}", unc_path, drive);

            // Verify the drive exists
            if std::path::Path::new(&format!("{}\\", drive)).exists() {
                {
                    let mut dl = self.drive_letter.lock().await;
                    *dl = Some(drive.to_string());
                }
                return Ok(true);
            } else {
                warn!("Drive was created but path doesn't exist yet");
                // Give Windows a moment to finish the connection
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                if std::path::Path::new(&format!("{}\\", drive)).exists() {
                    let mut dl = self.drive_letter.lock().await;
                    *dl = Some(drive.to_string());
                    return Ok(true);
                }
            }
        }

        // Provide detailed error message
        let error_msg = if stderr.contains("The network name cannot be found") ||
                         stdout.contains("The network name cannot be found") {
            format!("无法访问共享 '{}'，请检查服务器地址和共享名称是否正确", self.config.share)
        } else if stderr.contains("Access is denied") ||
                  stdout.contains("Access is denied") {
            "访问被拒绝，请检查用户名和密码是否正确".to_string()
        } else if stderr.contains("The user name or password is incorrect") ||
                  stdout.contains("The user name or password is incorrect") {
            "用户名或密码错误".to_string()
        } else if stderr.contains("System error 53") ||
                  stdout.contains("System error 53") {
            format!("无法找到网络路径 '{}'，请检查服务器 '{}' 是否可达", unc_path, self.config.host)
        } else if stderr.contains("System error 67") ||
                  stdout.contains("System error 67") {
            "无法找到网络名称，请检查网络连接".to_string()
        } else {
            format!("挂载失败: {} {}", stdout.trim(), stderr.trim())
        };

        error!("Mount failed: {}", error_msg);
        Err(Error::Connection(error_msg))
    }

    #[cfg(not(windows))]
    async fn mount_share(&self, _drive: &str) -> Result<bool> {
        warn!("Mount not implemented for this platform");
        Err(Error::Connection("Mount not supported on this platform".to_string()))
    }

    /// Unmount the drive
    #[cfg(windows)]
    async fn unmount_share(&self, drive: &str) -> Result<()> {
        info!("Unmounting drive {}", drive);

        let output = Command::new("net")
            .args(["use", drive, "/delete", "/y"])
            .output()
            .map_err(|e| Error::Connection(format!("Failed to execute net use: {}", e)))?;

        if output.status.success() {
            {
                let mut dl = self.drive_letter.lock().await;
                *dl = None;
            }
            info!("Successfully unmounted {}", drive);
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("Unmount warning: {}", stderr);
            Ok(())
        }
    }

    #[cfg(not(windows))]
    async fn unmount_share(&self, _drive: &str) -> Result<()> {
        Ok(())
    }
}

#[async_trait]
impl Protocol for SMBClient {
    async fn connect(&mut self) -> Result<()> {
        debug!("Connecting to SMB server: {}:{}", self.config.host, self.config.port);

        // Test connection first
        match self.test_smb_connection().await {
            Ok(true) => {
                let mut state = self.state.lock().await;
                state.connected = true;
                state.server = format!("{}:{}", self.config.host, self.config.port);
                info!("Connected to SMB server at {}", state.server);
                Ok(())
            }
            Ok(false) => {
                Err(Error::Connection("无法连接到 SMB 服务器，请检查网络连接和服务器地址".to_string()))
            }
            Err(e) => {
                Err(e)
            }
        }
    }

    async fn disconnect(&mut self) -> Result<()> {
        #[cfg(windows)]
        {
            let drive_to_unmount = {
                let dl = self.drive_letter.lock().await;
                if let Some(ref drive) = *dl {
                    Some(drive.clone())
                } else {
                    None
                }
            };

            if let Some(drive) = drive_to_unmount {
                self.unmount_share(&drive).await?;
            }
        }

        let mut state = self.state.lock().await;
        state.connected = false;
        state.server = String::new();
        state.mount_success = false;

        debug!("Disconnected from SMB server");
        Ok(())
    }

    fn is_connected(&self) -> bool {
        if let Ok(state) = self.state.try_lock() {
            return state.connected;
        }
        false
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>> {
        debug!("Listing directory: {}", path);

        #[cfg(windows)]
        {
            let drive = self.drive_letter.lock().await;
            if let Some(ref drive_letter) = *drive {
                let full_path = format!("{}\\{}", drive_letter.trim_end_matches(':'), path.trim_start_matches('\\'));
                let dir_path = std::path::Path::new(&full_path);

                if dir_path.exists() {
                    let mut entries = Vec::new();

                    if let Ok(read_dir) = std::fs::read_dir(dir_path) {
                        for entry in read_dir.flatten() {
                            let metadata = entry.metadata().ok();
                            let file_name = entry.file_name().to_string_lossy().to_string();
                            let is_dir = metadata.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                            let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
                            let modified = metadata.and_then(|m| m.modified().ok())
                                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                                .map(|d| d.as_secs() as i64)
                                .unwrap_or(0);

                            entries.push(FileEntry {
                                name: file_name,
                                path: path.to_string(),
                                is_dir,
                                size,
                                modified,
                            });
                        }
                    }

                    return Ok(entries);
                }
            }
        }

        // Return empty list if can't access
        Ok(vec![])
    }

    async fn read_file(&self, path: &str, offset: u64, length: usize) -> Result<Vec<u8>> {
        debug!("Reading file: {} offset: {} length: {}", path, offset, length);

        // Check cache first
        {
            let cache = self.file_cache.lock().await;
            if let Some(data) = cache.get(path) {
                let start = offset as usize;
                let end = (start + length).min(data.len());
                if start < data.len() {
                    return Ok(data[start..end].to_vec());
                }
            }
        }

        #[cfg(windows)]
        {
            let drive = self.drive_letter.lock().await;
            if let Some(ref drive_letter) = *drive {
                let full_path = format!("{}\\{}", drive_letter.trim_end_matches(':'), path.trim_start_matches('\\'));

                if let Ok(data) = std::fs::read(&full_path) {
                    let start = offset as usize;
                    let end = (start + length).min(data.len());
                    if start < data.len() {
                        return Ok(data[start..end].to_vec());
                    }
                }
            }
        }

        Ok(vec![])
    }

    async fn write_file(&self, path: &str, offset: u64, data: &[u8]) -> Result<usize> {
        debug!("Writing to file: {} offset: {} bytes: {}", path, offset, data.len());

        #[cfg(windows)]
        {
            let drive = self.drive_letter.lock().await;
            if let Some(ref drive_letter) = *drive {
                let full_path = format!("{}\\{}", drive_letter.trim_end_matches(':'), path.trim_start_matches('\\'));

                // For now, use std::fs to write (blocking)
                // In production, would need proper async file I/O
                let parent = std::path::Path::new(&full_path).parent();
                if let Some(parent_dir) = parent {
                    let _ = std::fs::create_dir_all(parent_dir);
                }

                // Read existing file first
                let mut existing_data = std::fs::read(&full_path).unwrap_or_default();

                // Write at offset
                let offset_usize = offset as usize;
                if offset_usize + data.len() > existing_data.len() {
                    existing_data.resize(offset_usize + data.len(), 0);
                }
                existing_data[offset_usize..offset_usize + data.len()].copy_from_slice(data);

                std::fs::write(&full_path, &existing_data)
                    .map_err(|e| Error::Io(e))?;

                return Ok(data.len());
            }
        }

        // Cache the write
        let mut cache = self.file_cache.lock().await;
        let existing = cache.entry(path.to_string()).or_insert_with(Vec::new);

        let offset_usize = offset as usize;
        if offset_usize + data.len() > existing.len() {
            existing.resize(offset_usize + data.len(), 0);
        }
        existing[offset_usize..offset_usize + data.len()].copy_from_slice(data);

        Ok(data.len())
    }

    async fn create_dir(&self, path: &str) -> Result<()> {
        debug!("Creating directory: {}", path);

        #[cfg(windows)]
        {
            let drive = self.drive_letter.lock().await;
            if let Some(ref drive_letter) = *drive {
                let full_path = format!("{}\\{}", drive_letter.trim_end_matches(':'), path.trim_start_matches('\\'));
                std::fs::create_dir_all(&full_path)
                    .map_err(|e| Error::Io(e))?;
                return Ok(());
            }
        }

        Ok(())
    }

    async fn remove(&self, path: &str, recursive: bool) -> Result<()> {
        debug!("Removing: {} recursive: {}", path, recursive);

        #[cfg(windows)]
        {
            let drive = self.drive_letter.lock().await;
            if let Some(ref drive_letter) = *drive {
                let full_path = format!("{}\\{}", drive_letter.trim_end_matches(':'), path.trim_start_matches('\\'));

                if recursive {
                    std::fs::remove_dir_all(&full_path)
                        .map_err(|e| Error::Io(e))?;
                } else {
                    std::fs::remove_file(&full_path)
                        .map_err(|e| Error::Io(e))?;
                }
                return Ok(());
            }
        }

        Ok(())
    }

    async fn rename(&self, from: &str, to: &str) -> Result<()> {
        debug!("Renaming: {} -> {}", from, to);

        #[cfg(windows)]
        {
            let drive = self.drive_letter.lock().await;
            if let Some(ref drive_letter) = *drive {
                let from_path = format!("{}\\{}", drive_letter.trim_end_matches(':'), from.trim_start_matches('\\'));
                let to_path = format!("{}\\{}", drive_letter.trim_end_matches(':'), to.trim_start_matches('\\'));

                std::fs::rename(&from_path, &to_path)
                    .map_err(|e| Error::Io(e))?;
                return Ok(());
            }
        }

        Ok(())
    }

    async fn stat(&self, path: &str) -> Result<FileEntry> {
        debug!("Getting attributes: {}", path);

        #[cfg(windows)]
        {
            let drive = self.drive_letter.lock().await;
            if let Some(ref drive_letter) = *drive {
                let full_path = if path.is_empty() || path == "\\" || path == "/" {
                    format!("{}", drive_letter.trim_end_matches(':'))
                } else {
                    format!("{}\\{}", drive_letter.trim_end_matches(':'), path.trim_start_matches('\\'))
                };

                if let Ok(metadata) = std::fs::metadata(&full_path) {
                    let name = path.trim_matches('\\').trim_matches('/').split('/').last()
                        .unwrap_or(path).to_string();
                    let is_dir = metadata.is_dir();
                    let size = metadata.len();
                    let modified = metadata.modified().ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);

                    return Ok(FileEntry {
                        name,
                        path: path.to_string(),
                        is_dir,
                        size,
                        modified,
                    });
                }
            }
        }

        // Return default
        Ok(FileEntry {
            name: path.to_string(),
            path: path.to_string(),
            is_dir: false,
            size: 0,
            modified: 0,
        })
    }
}

/// Create SMB client from connection parameters
pub fn create_smb_client(
    host: &str,
    port: u16,
    share: &str,
    path: &str,
    username: &str,
    password: Option<&str>,
) -> Result<SMBClient> {
    let config = SMBConfig {
        host: host.to_string(),
        port,
        share: share.to_string(),
        path: path.to_string(),
        username: username.to_string(),
        password: password.map(String::from),
    };

    Ok(SMBClient::new(config))
}

/// Mount SMB share using net use command
pub async fn mount_smb_share(
    host: &str,
    port: u16,
    share: &str,
    path: &str,
    username: &str,
    password: Option<&str>,
    drive: &str,
) -> Result<bool> {
    let client = SMBClient::new(SMBConfig {
        host: host.to_string(),
        port,
        share: share.to_string(),
        path: path.to_string(),
        username: username.to_string(),
        password: password.map(String::from),
    });

    client.mount_share(drive).await
}
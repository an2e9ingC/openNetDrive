//! SMB protocol implementation
//!
//! Uses Windows native SMB support via command line for connection testing and mounting

use crate::error::{Error, Result};
use crate::protocol::{FileEntry, Protocol};
use async_trait::async_trait;
use encoding_rs::GBK;
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::process::Command;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Convert Windows command output from GBK to String
/// Windows console uses GBK encoding by default, not UTF-8
fn decode_windows_output(bytes: &[u8]) -> String {
    // Try UTF-8 first (in case user has chcp 65001)
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    // Fall back to GBK
    let (decoded, _, _) = GBK.decode(bytes);
    decoded.into_owned()
}

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
    drive_letter: Option<String>,
}

/// SMB Protocol Client
pub struct SMBClient {
    config: SMBConfig,
    state: Arc<Mutex<SMBState>>,
    // In-memory file cache for demonstration
    // In production, this would use actual SMB connection
    file_cache: Arc<Mutex<HashMap<String, Vec<u8>>>>,
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
                drive_letter: None,
            })),
            file_cache: Arc::new(Mutex::new(HashMap::new())),
            drive_letter: Arc::new(Mutex::new(None)),
        }
    }

    /// Build UNC path
    fn build_unc_path(&self) -> String {
        let base = format!(
            "\\\\{}\\",
            self.config.host,
        );

        if self.config.share.is_empty() {
            base
        } else {
            format!("{}{}\\", base, self.config.share)
        }
    }

    /// Build SMB path
    fn build_path(&self, path: &str) -> String {
        let base = format!(
            "\\\\{}\\{}{}",
            self.config.host,
            self.config.share,
            self.config.path.trim_end_matches('/')
        );

        if path.is_empty() || path == "/" {
            base
        } else {
            format!("{}{}", base, path)
        }
    }

    /// Test SMB connection using net use
    #[cfg(windows)]
    async fn test_smb_connection(&self) -> Result<bool> {
        let unc_path = self.build_unc_path();
        info!("Testing SMB connection to: {}", unc_path);

        // First, ping the server to check basic connectivity
        let ping_output = Command::new("ping")
            .args(["-n", "1", "-w", "1000", &self.config.host])
            .output();

        match ping_output {
            Ok(output) => {
                if !output.status.success() {
                    warn!("Ping to {} failed", self.config.host);
                    return Ok(false);
                }
            }
            Err(e) => {
                warn!("Failed to run ping: {}", e);
                return Ok(false);
            }
        }

        // Try to access the share using net use (test without actually mounting)
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
            // Format: /user:domainusername or /user:username@domain.com
            let user_arg = format!("/user:{}", self.config.username);
            args.push(user_arg);
            if let Some(ref password) = self.config.password {
                args.push(password.clone());
            }
        }

        let output = cmd.args(&args).output();

        match output {
            Ok(output) => {
                let stdout = decode_windows_output(&output.stdout);
                let stderr = decode_windows_output(&output.stderr);

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
            let output_str = decode_windows_output(&output.stdout);
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

        // Add credentials if provided
        if !self.config.username.is_empty() {
            // Format: /user:domain\username or /user:username@domain.com
            let user_arg = if self.config.username.contains('\\') || self.config.username.contains('@') {
                format!("/user:{}", self.config.username)
            } else {
                format!("/user:{}", self.config.username)
            };
            args.push(user_arg);
            if let Some(ref password) = self.config.password {
                args.push(password.clone());
            }
        }

        // Add persistent option
        args.push("/persistent:yes".to_string());

        let output = cmd.args(&args).output()
            .map_err(|e| Error::Connection(format!("Failed to execute net use: {}", e)))?;

        let stdout = decode_windows_output(&output.stdout);
        let stderr = decode_windows_output(&output.stderr);

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
        } else if stderr.contains("System error 85") ||
                  stdout.contains("System error 85") {
            "本地设备名已被使用，请选择其他盘符".to_string()
        } else {
            format!("挂载失败: {} {}", stdout.trim(), stderr.trim())
        };

        error!("Mount failed: {}", error_msg);
        Err(Error::Connection(error_msg))
    }

    #[cfg(not(windows))]
    async fn mount_share(&self, _drive: &str) -> Result<bool> {
        warn!("SMB mount not implemented for this platform");
        Ok(false)
    }

    /// Unmount the SMB share
    #[cfg(windows)]
    async fn unmount_share(&self, drive: &str) -> Result<bool> {
        info!("Unmounting SMB share from {}", drive);

        let output = Command::new("net")
            .args(["use", drive, "/delete", "/y"])
            .output()
            .map_err(|e| Error::Connection(format!("Failed to execute net use: {}", e)))?;

        if output.status.success() {
            info!("Successfully unmounted {}", drive);
            let mut dl = self.drive_letter.lock().await;
            *dl = None;
            Ok(true)
        } else {
            let stdout = decode_windows_output(&output.stdout);
            let stderr = decode_windows_output(&output.stderr);
            warn!("Unmount failed: {} {}", stdout, stderr);
            Ok(false)
        }
    }

    #[cfg(not(windows))]
    async fn unmount_share(&self, _drive: &str) -> Result<bool> {
        warn!("SMB unmount not implemented for this platform");
        Ok(false)
    }
}

#[async_trait]
impl Protocol for SMBClient {
    async fn connect(&mut self) -> Result<()> {
        debug!("Connecting to SMB server: {}:{}", self.config.host, self.config.port);

        // Test the connection first
        if !self.test_smb_connection().await? {
            return Err(Error::Connection("无法连接到 SMB 服务器".to_string()));
        }

        let mut state = self.state.lock().await;
        state.connected = true;
        state.server = format!("{}:{}", self.config.host, self.config.port);

        debug!("Connected to SMB server at {}", state.server);
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        let mut state = self.state.lock().await;
        state.connected = false;
        state.server = String::new();

        debug!("Disconnected from SMB server");
        Ok(())
    }

    fn is_connected(&self) -> bool {
        // Note: This would need interior mutability or atomic for real implementation
        true
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>> {
        let full_path = self.build_path(path);
        debug!("Listing directory: {}", full_path);

        // Simulated directory listing
        // In production, this would use actual SMB API

        let mut entries = Vec::new();

        // Add . and .. for root
        if path.is_empty() || path == "/" {
            entries.push(FileEntry {
                name: ".".to_string(),
                path: full_path.clone(),
                is_dir: true,
                size: 0,
                modified: 0,
            });

            entries.push(FileEntry {
                name: "..".to_string(),
                path: full_path.clone(),
                is_dir: true,
                size: 0,
                modified: 0,
            });
        }

        // Simulated test entries
        entries.push(FileEntry {
            name: "test_folder".to_string(),
            path: format!("{}/test_folder", full_path),
            is_dir: true,
            size: 0,
            modified: 0,
        });

        entries.push(FileEntry {
            name: "test_file.txt".to_string(),
            path: format!("{}/test_file.txt", full_path),
            is_dir: false,
            size: 1024,
            modified: chrono::Local::now().timestamp(),
        });

        Ok(entries)
    }

    async fn read_file(&self, path: &str, offset: u64, length: usize) -> Result<Vec<u8>> {
        let full_path = self.build_path(path);
        debug!("Reading file: {} offset: {} length: {}", full_path, offset, length);

        // Check cache first
        {
            let cache = self.file_cache.lock().await;
            if let Some(data) = cache.get(&full_path) {
                let start = offset as usize;
                let end = (start + length).min(data.len());
                return Ok(data[start..end].to_vec());
            }
        }

        // Simulated file content
        // In production, this would use actual SMB API
        let simulated_content = b"This is simulated SMB file content for testing purposes.";

        let start = offset as usize;
        let end = start.saturating_add(length).min(simulated_content.len());

        Ok(simulated_content[start..end].to_vec())
    }

    async fn write_file(&self, path: &str, offset: u64, data: &[u8]) -> Result<usize> {
        let full_path = self.build_path(path);
        debug!("Writing to file: {} offset: {} bytes: {}", full_path, offset, data.len());

        // In production, this would use actual SMB API
        // For now, cache the write
        let mut cache = self.file_cache.lock().await;

        let existing = cache.entry(full_path.clone()).or_insert_with(Vec::new);

        let offset_usize = offset as usize;
        if offset_usize + data.len() > existing.len() {
            existing.resize(offset_usize + data.len(), 0);
        }

        existing[offset_usize..offset_usize + data.len()].copy_from_slice(data);

        Ok(data.len())
    }

    async fn create_dir(&self, path: &str) -> Result<()> {
        let full_path = self.build_path(path);
        debug!("Creating directory: {}", full_path);

        // In production, this would use actual SMB API
        // Simulate success
        Ok(())
    }

    async fn remove(&self, path: &str, _recursive: bool) -> Result<()> {
        let full_path = self.build_path(path);
        debug!("Removing: {}", full_path);

        // In production, this would use actual SMB API
        // Simulate success
        Ok(())
    }

    async fn rename(&self, from: &str, to: &str) -> Result<()> {
        let from_path = self.build_path(from);
        let to_path = self.build_path(to);
        debug!("Renaming: {} -> {}", from_path, to_path);

        // In production, this would use actual SMB API
        // Simulate success
        Ok(())
    }

    async fn stat(&self, path: &str) -> Result<FileEntry> {
        let full_path = self.build_path(path);
        debug!("Getting attributes: {}", full_path);

        // In production, this would use actual SMB API
        // Return simulated info
        let is_dir = path.is_empty() || path == "/" || path.ends_with('/');

        Ok(FileEntry {
            name: path.trim_matches('/').split('/').last().unwrap_or("root").to_string(),
            path: full_path,
            is_dir,
            size: if is_dir { 0 } else { 1024 },
            modified: chrono::Local::now().timestamp(),
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

/// Mount an SMB share to a local drive letter
pub async fn mount_smb_share(
    host: &str,
    port: u16,
    share: &str,
    path: &str,
    username: &str,
    password: Option<&str>,
    drive: &str,
) -> Result<bool> {
    let client = create_smb_client(host, port, share, path, username, password)?;
    client.mount_share(drive).await
}
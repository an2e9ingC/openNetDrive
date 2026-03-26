//! Protocol abstraction for openNetDrive

use crate::error::Result;
use async_trait::async_trait;

/// File entry information
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified: i64,
}

/// Protocol trait that all providers must implement
#[async_trait]
pub trait Protocol: Send + Sync {
    /// Connect to the remote server
    async fn connect(&mut self) -> Result<()>;

    /// Disconnect from the server
    async fn disconnect(&mut self) -> Result<()>;

    /// Check if connected
    fn is_connected(&self) -> bool;

    /// List directory contents
    async fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>>;

    /// Read a file
    async fn read_file(&self, path: &str, offset: u64, length: usize) -> Result<Vec<u8>>;

    /// Write a file
    async fn write_file(&self, path: &str, offset: u64, data: &[u8]) -> Result<usize>;

    /// Create a directory
    async fn create_dir(&self, path: &str) -> Result<()>;

    /// Remove a file or directory
    async fn remove(&self, path: &str, recursive: bool) -> Result<()>;

    /// Rename a file or directory
    async fn rename(&self, from: &str, to: &str) -> Result<()>;

    /// Get file attributes
    async fn stat(&self, path: &str) -> Result<FileEntry>;
}

/// Protocol provider trait for creating protocol instances
pub trait ProtocolProvider: Send + Sync {
    /// Create a new protocol instance
    fn create(&self, url: &str, username: &str, password: Option<&str>) -> Result<Box<dyn Protocol>>;

    /// Get the protocol name
    fn name(&self) -> &'static str;
}

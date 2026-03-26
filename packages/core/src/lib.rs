//! openNetDrive Core Library
//!
//! This crate provides the core functionality for openNetDrive:
//! - Configuration management
//! - Protocol abstraction (WebDAV, SMB)
//! - Credential management

pub mod config;
pub mod protocol;
pub mod credentials;
pub mod error;
pub mod webdav;
pub mod smb;

pub use config::{Config, ConnectionConfig, ConnectionType};
pub use protocol::{Protocol, ProtocolProvider};
pub use credentials::CredentialManager;
pub use error::{Result, Error};
pub use webdav::WebDAVClient;
pub use smb::{SMBClient, create_smb_client};

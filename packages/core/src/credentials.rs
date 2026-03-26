//! Credential management for openNetDrive

use crate::error::{Result, Error};
use keyring::Entry;

/// Credential manager
pub struct CredentialManager {
    service: String,
}

impl CredentialManager {
    /// Create a new credential manager
    pub fn new() -> Result<Self> {
        Ok(Self {
            service: "openNetDrive".to_string(),
        })
    }

    /// Create a credential manager with custom service name
    pub fn with_service(service: &str) -> Result<Self> {
        Ok(Self {
            service: service.to_string(),
        })
    }

    /// Get credential entry key for a connection
    fn get_entry_key(connection_id: &str, username: &str) -> String {
        format!("{}:{}", connection_id, username)
    }

    /// Store credentials for a connection
    pub fn store(&self, username: &str, password: &str) -> Result<()> {
        let entry = Entry::new(&self.service, username)
            .map_err(|e| Error::Credential(format!("Failed to create credential entry: {}", e)))?;

        entry.set_password(password)
            .map_err(|e| Error::Credential(format!("Failed to store credentials: {}", e)))?;

        Ok(())
    }

    /// Store credentials for a specific connection
    pub fn store_for_connection(&self, connection_id: &str, username: &str, password: &str) -> Result<()> {
        let key = Self::get_entry_key(connection_id, username);
        let entry = Entry::new(&self.service, &key)
            .map_err(|e| Error::Credential(format!("Failed to create credential entry: {}", e)))?;

        entry.set_password(password)
            .map_err(|e| Error::Credential(format!("Failed to store credentials: {}", e)))?;

        Ok(())
    }

    /// Retrieve credentials
    pub fn get(&self, username: &str) -> Result<String> {
        let entry = Entry::new(&self.service, username)
            .map_err(|e| Error::Credential(format!("Failed to create credential entry: {}", e)))?;

        entry.get_password()
            .map_err(|e| Error::Credential(format!("Failed to retrieve credentials: {}", e)))
    }

    /// Retrieve credentials for a specific connection
    pub fn get_for_connection(&self, connection_id: &str, username: &str) -> Result<String> {
        let key = Self::get_entry_key(connection_id, username);
        let entry = Entry::new(&self.service, &key)
            .map_err(|e| Error::Credential(format!("Failed to create credential entry: {}", e)))?;

        entry.get_password()
            .map_err(|e| Error::Credential(format!("Failed to retrieve credentials: {}", e)))
    }

    /// Delete credentials
    pub fn delete(&self, username: &str) -> Result<()> {
        let entry = Entry::new(&self.service, username)
            .map_err(|e| Error::Credential(format!("Failed to create credential entry: {}", e)))?;

        entry.set_password("")
            .map_err(|e| Error::Credential(format!("Failed to clear credentials: {}", e)))?;

        Ok(())
    }

    /// Delete credentials for a specific connection
    pub fn delete_for_connection(&self, connection_id: &str, username: &str) -> Result<()> {
        let key = Self::get_entry_key(connection_id, username);
        let entry = Entry::new(&self.service, &key)
            .map_err(|e| Error::Credential(format!("Failed to create credential entry: {}", e)))?;

        entry.set_password("")
            .map_err(|e| Error::Credential(format!("Failed to clear credentials: {}", e)))?;

        Ok(())
    }
}

impl Default for CredentialManager {
    fn default() -> Self {
        Self::new().expect("Failed to create CredentialManager")
    }
}

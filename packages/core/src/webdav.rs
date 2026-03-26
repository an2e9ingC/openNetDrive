//! WebDAV protocol implementation

use crate::error::{Result, Error};
use crate::protocol::{Protocol, FileEntry};
use async_trait::async_trait;
use log::debug;
use reqwest::{Client, Method, StatusCode};

/// WebDAV protocol client
pub struct WebDAVClient {
    url: String,
    username: String,
    password: Option<String>,
    client: Client,
    connected: bool,
}

impl WebDAVClient {
    /// Create a new WebDAV client
    pub fn new(url: &str, username: &str, password: Option<&str>) -> Result<Self> {
        let client = Client::builder()
            .build()
            .map_err(|e| Error::Protocol(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self {
            url: url.trim_end_matches('/').to_string(),
            username: username.to_string(),
            password: password.map(String::from),
            client,
            connected: false,
        })
    }

    /// Build a URL for a given path
    fn build_url(&self, path: &str) -> String {
        let path = path.trim_start_matches('/');
        format!("{}/{}", self.url, path)
    }

    /// Parse WebDAV XML response
    fn parse_multistatus(&self, xml: &str) -> Result<Vec<FileEntry>> {
        // Simple XML parsing for DAV:response elements
        // In production, use a proper XML parser like quick-xml
        let mut entries = Vec::new();

        // Extract response blocks
        let response_start = "<D:response>";
        let response_end = "</D:response>";

        let mut remaining = xml;
        while let Some(start) = remaining.find(response_start) {
            let start = start + response_start.len();
            if let Some(end) = remaining[start..].find(response_end) {
                let response = &remaining[start..start + end];

                // Extract href
                let href = self.extract_xml_element(response, "D:href")
                    .or_else(|| self.extract_xml_element(response, "href"))
                    .unwrap_or_else(|| "/".to_string());

                // Extract displayname
                let name = self.extract_xml_element(response, "D:displayname")
                    .or_else(|| self.extract_xml_element(response, "displayname"))
                    .unwrap_or_else(|| {
                        // Use last part of href as fallback
                        href.split('/').filter(|s| !s.is_empty()).last().unwrap_or("Unknown").to_string()
                    });

                // Check if it's a collection (directory)
                let is_dir = response.contains("D:collection") || response.contains("<D:resourcetype><D:collection/>");

                // Extract content length
                let size: u64 = self.extract_xml_element(response, "D:getcontentlength")
                    .or_else(|| self.extract_xml_element(response, "getcontentlength"))
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);

                // Extract modified time
                let modified = self.extract_xml_element(response, "D:getlastmodified")
                    .or_else(|| self.extract_xml_element(response, "getlastmodified"))
                    .and_then(|s| chrono::DateTime::parse_from_rfc2822(&s).ok())
                    .map(|dt| dt.timestamp())
                    .unwrap_or(0);

                entries.push(FileEntry {
                    name,
                    path: href,
                    is_dir,
                    size,
                    modified,
                });

                remaining = &remaining[start + end + response_end.len()..];
            } else {
                break;
            }
        }

        Ok(entries)
    }

    /// Extract XML element content
    fn extract_xml_element(&self, xml: &str, tag: &str) -> Option<String> {
        let open_tag = format!("<{}>", tag);
        let close_tag = format!("</{}>", tag);

        if let Some(start) = xml.find(&open_tag) {
            let start = start + open_tag.len();
            if let Some(end) = xml[start..].find(&close_tag) {
                return Some(xml[start..start + end].to_string());
            }
        }

        // Try self-closing tag with value attribute
        let attr_tag = format!("<{} ", tag);
        if let Some(start) = xml.find(&attr_tag) {
            if let Some(value_start) = xml[start..].find("=\"") {
                let value_start = start + value_start + 2;
                if let Some(value_end) = xml[value_start..].find('"') {
                    return Some(xml[value_start..value_start + value_end].to_string());
                }
            }
        }

        None
    }

    /// Send a WebDAV PROPFIND request
    async fn propfind(&self, path: &str, depth: usize) -> Result<String> {
        let url = self.build_url(path);
        let body = r#"<?xml version="1.0" encoding="utf-8"?>
            <D:propfind xmlns:D="DAV:">
                <D:allprop/>
            </D:propfind>"#;

        let mut request = self.client.request(Method::from_bytes(b"PROPFIND").unwrap(), &url)
            .body(body)
            .header("Depth", depth.to_string())
            .header("Content-Type", "application/xml");

        if let Some(password) = &self.password {
            request = request.basic_auth(&self.username, Some(password));
        } else {
            request = request.basic_auth(&self.username, None::<&str>);
        }

        let response = request.send().await
            .map_err(|e| Error::Protocol(format!("PROPFIND request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::Protocol(format!("PROPFIND failed with status: {}", response.status())));
        }

        response.text().await
            .map_err(|e| Error::Protocol(format!("Failed to read response: {}", e)))
    }
}

#[async_trait]
impl Protocol for WebDAVClient {
    async fn connect(&mut self) -> Result<()> {
        debug!("Connecting to WebDAV server: {}", self.url);

        // Test connection with OPTIONS request
        let mut request = self.client.request(Method::OPTIONS, &self.url);

        if let Some(password) = &self.password {
            request = request.basic_auth(&self.username, Some(password));
        } else {
            request = request.basic_auth(&self.username, None::<&str>);
        }

        let response = request.send().await
            .map_err(|e| Error::Connection(format!("Failed to connect to WebDAV server: {}", e)))?;

        // Check if DAV header is present (indicates WebDAV server)
        let dav_supported = response.headers().contains_key("DAV")
            || response.headers().get("Allow").map(|h| h.as_bytes()).unwrap_or(&[]).windows(7).any(|w| w == b"PROPFIND");

        if !dav_supported && !response.status().is_success() {
            // Some servers return 401 for OPTIONS, try PROPFIND instead
            match self.propfind("/", 0).await {
                Ok(_) => {
                    self.connected = true;
                    debug!("Connected to WebDAV server (via PROPFIND)");
                    return Ok(());
                }
                Err(e) => return Err(e),
            }
        }

        self.connected = true;
        debug!("Connected to WebDAV server");
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        self.connected = false;
        debug!("Disconnected from WebDAV server");
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>> {
        if !self.connected {
            return Err(Error::Connection("Not connected to WebDAV server".to_string()));
        }

        let xml = self.propfind(path, 1).await?;
        let mut entries = self.parse_multistatus(&xml)?;

        // Remove the root entry (the directory itself)
        if entries.len() > 1 {
            entries.remove(0);
        }

        Ok(entries)
    }

    async fn read_file(&self, path: &str, offset: u64, length: usize) -> Result<Vec<u8>> {
        if !self.connected {
            return Err(Error::Connection("Not connected to WebDAV server".to_string()));
        }

        let url = self.build_url(path);
        let mut request = self.client.get(&url);

        if let Some(password) = &self.password {
            request = request.basic_auth(&self.username, Some(password));
        } else {
            request = request.basic_auth(&self.username, None::<&str>);
        }

        // Support range requests
        if offset > 0 || length > 0 {
            let range = format!("bytes={}-{}", offset, offset + length as u64 - 1);
            request = request.header("Range", range);
        }

        let response = request.send().await
            .map_err(|e| Error::Protocol(format!("GET request failed: {}", e)))?;

        if !response.status().is_success() && response.status() != StatusCode::PARTIAL_CONTENT {
            return Err(Error::Protocol(format!("GET failed with status: {}", response.status())));
        }

        response.bytes().await
            .map(|b| b.to_vec())
            .map_err(|e| Error::Protocol(format!("Failed to read response: {}", e)))
    }

    async fn write_file(&self, path: &str, offset: u64, data: &[u8]) -> Result<usize> {
        if !self.connected {
            return Err(Error::Connection("Not connected to WebDAV server".to_string()));
        }

        let url = self.build_url(path);
        let mut request = self.client.put(&url).body(data.to_vec());

        if let Some(password) = &self.password {
            request = request.basic_auth(&self.username, Some(password));
        } else {
            request = request.basic_auth(&self.username, None::<&str>);
        }

        // Support offset writes
        if offset > 0 {
            let content_range = format!("bytes {}-{}/{}", offset, offset + data.len() as u64 - 1, offset + data.len() as u64);
            request = request.header("Content-Range", content_range);
        }

        let response = request.send().await
            .map_err(|e| Error::Protocol(format!("PUT request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::Protocol(format!("PUT failed with status: {}", response.status())));
        }

        Ok(data.len())
    }

    async fn create_dir(&self, path: &str) -> Result<()> {
        if !self.connected {
            return Err(Error::Connection("Not connected to WebDAV server".to_string()));
        }

        let url = self.build_url(path);
        let mut request = self.client.request(Method::from_bytes(b"MKCOL").unwrap(), &url);

        if let Some(password) = &self.password {
            request = request.basic_auth(&self.username, Some(password));
        } else {
            request = request.basic_auth(&self.username, None::<&str>);
        }

        let response = request.send().await
            .map_err(|e| Error::Protocol(format!("MKCOL request failed: {}", e)))?;

        if !response.status().is_success() && response.status() != StatusCode::CONFLICT {
            return Err(Error::Protocol(format!("MKCOL failed with status: {}", response.status())));
        }

        Ok(())
    }

    async fn remove(&self, path: &str, _recursive: bool) -> Result<()> {
        if !self.connected {
            return Err(Error::Connection("Not connected to WebDAV server".to_string()));
        }

        let url = self.build_url(path);
        let mut request = self.client.delete(&url);

        if let Some(password) = &self.password {
            request = request.basic_auth(&self.username, Some(password));
        } else {
            request = request.basic_auth(&self.username, None::<&str>);
        }

        let response = request.send().await
            .map_err(|e| Error::Protocol(format!("DELETE request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::Protocol(format!("DELETE failed with status: {}", response.status())));
        }

        Ok(())
    }

    async fn rename(&self, from: &str, to: &str) -> Result<()> {
        if !self.connected {
            return Err(Error::Connection("Not connected to WebDAV server".to_string()));
        }

        let url = self.build_url(from);
        let dest_url = self.build_url(to);
        let mut request = self.client.request(Method::from_bytes(b"MOVE").unwrap(), &url)
            .header("Destination", dest_url);

        if let Some(password) = &self.password {
            request = request.basic_auth(&self.username, Some(password));
        } else {
            request = request.basic_auth(&self.username, None::<&str>);
        }

        let response = request.send().await
            .map_err(|e| Error::Protocol(format!("MOVE request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::Protocol(format!("MOVE failed with status: {}", response.status())));
        }

        Ok(())
    }

    async fn stat(&self, path: &str) -> Result<FileEntry> {
        if !self.connected {
            return Err(Error::Connection("Not connected to WebDAV server".to_string()));
        }

        let xml = self.propfind(path, 0).await?;
        let mut entries = self.parse_multistatus(&xml)?;

        entries.pop()
            .ok_or_else(|| Error::Protocol(format!("File not found: {}", path)))
    }
}

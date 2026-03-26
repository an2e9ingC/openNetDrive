//! openNetDrive Command Line Interface

use clap::{Parser, Subcommand};
use opennetdrive_core::{Config, ConnectionConfig, ConnectionType, CredentialManager};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod state;

#[cfg(windows)]
use opennetdrive_mount_win::WinFspDriver;

#[derive(Parser)]
#[command(name = "ond")]
#[command(about = "openNetDrive - Network Drive Mount Tool", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Verbose output
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// List all connections
    List,

    /// Add a new connection
    Add {
        /// Connection name
        #[arg(short, long)]
        name: String,

        /// Connection type (webdav or smb)
        #[arg(short, long, value_enum)]
        r#type: ConnectionTypeArg,

        /// For WebDAV: URL, For SMB: host
        #[arg(short = 'H', long)]
        host: String,

        /// Username
        #[arg(short, long)]
        user: Option<String>,

        /// Mount point (drive letter on Windows)
        #[arg(short, long)]
        mount: Option<String>,

        /// Auto mount on startup
        #[arg(long, default_value = "false")]
        auto_mount: bool,
    },

    /// Remove a connection
    Remove {
        /// Connection ID or name
        id: String,
    },

    /// Mount a connection
    Mount {
        /// Connection ID or name
        id: String,
    },

    /// Unmount a connection
    Unmount {
        /// Connection ID or name
        id: String,
    },

    /// Show configuration
    Config,
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum ConnectionTypeArg {
    WebDAV,
    SMB,
}

fn init_logging(verbose: bool) {
    let level = if verbose { "debug" } else { "info" };

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| level.into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
}

/// Prompt for password without echoing
fn prompt_password(prompt: &str) -> anyhow::Result<String> {
    eprint!("{}", prompt);
    rpassword::read_password()
        .map_err(|e| anyhow::anyhow!("Failed to read password: {}", e))
}

/// Get password from credential manager for a connection
fn get_password_for_connection(
    cred_manager: &CredentialManager,
    connection_id: &str,
    username: &str,
) -> Option<String> {
    cred_manager
        .get_for_connection(connection_id, username)
        .ok()
}

/// Check if a process is still running (Windows only)
#[cfg(windows)]
fn is_process_running(pid: u32) -> bool {
    use std::process::Command;
    Command::new("tasklist")
        .arg("/FI")
        .arg(format!("PID eq {}", pid))
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|stdout| stdout.contains(&pid.to_string()))
        .unwrap_or(false)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    init_logging(cli.verbose);

    let mut config = Config::load()?;

    match cli.command {
        Commands::List => {
            if config.connections.is_empty() {
                println!("No connections configured.");
            } else {
                println!("Connections:");
                for conn in &config.connections {
                    let status = if conn.enabled { "🟢" } else { "⚫" };
                    let auto = if conn.auto_mount { " [auto]" } else { "" };
                    println!(
                        "  {} {} - {}{} (mount: {:?})",
                        status,
                        conn.name,
                        conn.id,
                        auto,
                        conn.mount_point
                    );
                }
            }
        }

        Commands::Add {
            name,
            r#type,
            host,
            user,
            mount,
            auto_mount,
        } => {
            let id = uuid::Uuid::new_v4().to_string()[..8].to_string();

            // Prompt for password if username is provided
            let password = if let Some(ref username) = user {
                match prompt_password(&format!("Password for {}: ", username)) {
                    Ok(pwd) if !pwd.is_empty() => Some(pwd),
                    _ => None,
                }
            } else {
                None
            };

            let connection_type = match r#type {
                ConnectionTypeArg::WebDAV => ConnectionType::WebDAV {
                    url: host,
                    username: user.clone().unwrap_or_default(),
                    password: None,
                },
                ConnectionTypeArg::SMB => {
                    // Parse host:port or just host
                    let parts: Vec<&str> = host.split(':').collect();
                    let (host_str, port) = if parts.len() == 2 {
                        (parts[0].to_string(), parts[1].parse().unwrap_or(445))
                    } else {
                        (host, 445)
                    };

                    ConnectionType::SMB {
                        host: host_str,
                        port,
                        share: "share".to_string(),
                        path: "/".to_string(),
                        username: user.clone().unwrap_or_default(),
                        password: None,
                    }
                }
            };

            let conn = ConnectionConfig {
                id: id.clone(),
                name,
                connection_type,
                mount_point: mount,
                auto_mount,
                enabled: false,
            };

            // Store password in credential manager
            if let (Some(pwd), Some(username)) = (password, &user) {
                let cred_manager = CredentialManager::new()?;
                if let Err(e) = cred_manager.store_for_connection(&id, username, &pwd) {
                    eprintln!("Warning: Failed to store credentials: {}", e);
                } else {
                    println!("Credentials stored securely.");
                }
            }

            config.add_connection(conn);
            config.save()?;

            println!("Connection added successfully.");
        }

        Commands::Remove { id } => {
            if config.remove_connection(&id).is_some() {
                config.save()?;
                println!("Connection removed.");
            } else {
                // Try to find by name
                if let Some(pos) = config.connections.iter().position(|c| c.name == id) {
                    config.connections.remove(pos);
                    config.save()?;
                    println!("Connection removed.");
                } else {
                    println!("Connection not found: {}", id);
                }
            }
        }

        Commands::Mount { id } => {
            println!("Mounting connection: {}", id);

            // Find connection by ID or name
            let conn = config.get_connection(&id)
                .or_else(|| config.connections.iter().find(|c| c.name == id))
                .cloned();

            let conn = match conn {
                Some(c) => c,
                None => {
                    println!("Connection not found: {}", id);
                    anyhow::bail!("Connection not found");
                }
            };

            let mount_point = conn.mount_point.clone()
                .unwrap_or_else(|| "Z:".to_string());

            // Check if already mounted
            if state::is_mount_point_in_use(&mount_point)? {
                println!("Mount point {} is already in use", mount_point);
                anyhow::bail!("Mount point already in use");
            }

            // Get password from credential manager
            let cred_manager = CredentialManager::new()?;
            let password = match &conn.connection_type {
                ConnectionType::WebDAV { username, .. } => {
                    get_password_for_connection(&cred_manager, &conn.id, username)
                }
                ConnectionType::SMB { username, .. } => {
                    get_password_for_connection(&cred_manager, &conn.id, username)
                }
            };

            #[cfg(windows)]
            {
                // Create protocol instance
                let protocol: Box<dyn opennetdrive_core::protocol::Protocol> = match &conn.connection_type {
                    ConnectionType::WebDAV { url, username, .. } => {
                        let client = opennetdrive_core::webdav::WebDAVClient::new(
                            url,
                            username,
                            password.as_deref(),
                        )?;
                        Box::new(client)
                    }
                    ConnectionType::SMB { host, port, share, path, username, .. } => {
                        let client = opennetdrive_core::smb::create_smb_client(
                            host,
                            *port,
                            share,
                            path,
                            username,
                            password.as_deref(),
                        )?;
                        Box::new(client)
                    }
                };

                // Create and start WinFsp driver
                let mut driver = WinFspDriver::new(mount_point.clone(), protocol);

                println!("Starting filesystem at {}...", mount_point);
                driver.start().await?;
                println!("Successfully mounted '{}' at {}", conn.name, mount_point);

                // Save mount state
                let mount_state = state::MountState {
                    mount_point: mount_point.clone(),
                    connection_id: conn.id.clone(),
                    connection_name: conn.name.clone(),
                    pid: std::process::id(),
                };
                state::save_mount_state(&mount_state)?;

                // Keep running (this blocks until Ctrl+C)
                println!("Press Ctrl+C to unmount...");
                tokio::signal::ctrl_c().await?;

                // Cleanup on exit
                println!("\nUnmounting...");
                driver.stop().await?;
                state::remove_mount_state(&mount_point)?;
                println!("Unmounted successfully.");
            }

            #[cfg(not(windows))]
            {
                println!("Mounting is only supported on Windows with WinFsp installed.");
                anyhow::bail!("Platform not supported for mounting");
            }
        }

        Commands::Unmount { id } => {
            println!("Unmounting connection: {}", id);

            #[cfg(not(windows))]
            {
                println!("Unmounting is only supported on Windows with WinFsp installed.");
                anyhow::bail!("Platform not supported for unmounting");
            }

            #[cfg(windows)]
            {
                // Find mount state by connection ID or name
                let mount_state = state::get_mount_state_by_id(&id)?
                    .or_else(|| {
                        // Try to find by mount point directly (if user provided mount point like "Z:")
                        state::get_mount_state(&id).ok().flatten()
                    });

                match mount_state {
                    Some(state) => {
                        // Check if the process is still running
                        if is_process_running(state.pid) {
                            println!("Mount process (PID {}) is still running", state.pid);
                            println!("Sending termination signal...");

                            // Try to gracefully terminate
                            use std::process::Command;
                            Command::new("taskkill")
                                .args(["/PID", &state.pid.to_string(), "/T"])
                                .output()
                                .ok();

                            // Wait a bit for process to exit
                            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                        }

                        // Clean up state file
                        state::remove_mount_state(&state.mount_point)?;
                        println!("Unmounted '{}' from {}", state.connection_name, state.mount_point);
                    }
                    None => {
                        // Check if it's a mount point directly
                        if let Some(state) = state::get_mount_state(&id)? {
                            state::remove_mount_state(&state.mount_point)?;
                            println!("Unmounted '{}' from {}", state.connection_name, state.mount_point);
                        } else {
                            println!("No active mount found for: {}", id);
                        }
                    }
                }
            }
        }

        Commands::Config => {
            let path = Config::config_path()?;
            println!("Config file: {}", path.display());
            println!();
            println!("{}", toml::to_string_pretty(&config)?);
        }
    }

    Ok(())
}

//! Server configuration.

use std::net::SocketAddr;
use std::path::PathBuf;

/// Server configuration.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Address to bind to.
    pub bind_addr: SocketAddr,
    /// Path to the data directory.
    pub data_dir: PathBuf,
    /// Maximum number of concurrent connections.
    pub max_connections: usize,
    /// Read buffer size per connection.
    pub read_buffer_size: usize,
    /// Write buffer size per connection.
    pub write_buffer_size: usize,
}

impl ServerConfig {
    /// Creates a new server configuration.
    pub fn new(bind_addr: impl Into<SocketAddr>, data_dir: impl Into<PathBuf>) -> Self {
        Self {
            bind_addr: bind_addr.into(),
            data_dir: data_dir.into(),
            max_connections: 1024,
            read_buffer_size: 64 * 1024,  // 64 KiB
            write_buffer_size: 64 * 1024, // 64 KiB
        }
    }

    /// Sets the maximum number of concurrent connections.
    pub fn with_max_connections(mut self, max: usize) -> Self {
        self.max_connections = max;
        self
    }

    /// Sets the read buffer size.
    pub fn with_read_buffer_size(mut self, size: usize) -> Self {
        self.read_buffer_size = size;
        self
    }

    /// Sets the write buffer size.
    pub fn with_write_buffer_size(mut self, size: usize) -> Self {
        self.write_buffer_size = size;
        self
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_addr: "127.0.0.1:5432".parse().expect("valid address"),
            data_dir: PathBuf::from("./data"),
            max_connections: 1024,
            read_buffer_size: 64 * 1024,
            write_buffer_size: 64 * 1024,
        }
    }
}

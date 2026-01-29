//! Kimberlite unified CLI.
//!
//! The compliance-first database for regulated industries.
//!
//! # Quick Start
//!
//! ```bash
//! # Initialize a data directory
//! kimberlite init ./data --development
//!
//! # Start the server
//! kimberlite start --address 3000 ./data
//!
//! # Connect with the REPL (new terminal)
//! kimberlite repl
//! ```

mod commands;

use anyhow::Result;
use clap::{Parser, Subcommand};

/// Kimberlite - the compliance-first database for regulated industries.
#[derive(Parser)]
#[command(name = "kimberlite")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show version information.
    Version,

    /// Initialize a new data directory.
    Init {
        /// Path to the data directory to create.
        path: String,

        /// Enable development mode (relaxed durability, no replication).
        #[arg(long)]
        development: bool,
    },

    /// Start the Kimberlite server.
    Start {
        /// Path to the data directory.
        path: String,

        /// Address to bind to (port only: 3000, or full: 127.0.0.1:3000).
        #[arg(short, long, default_value = "5432")]
        address: String,

        /// Enable development mode (no replication).
        #[arg(long)]
        development: bool,
    },

    /// Interactive SQL REPL.
    Repl {
        /// Server address to connect to.
        #[arg(short, long, default_value = "127.0.0.1:5432")]
        address: String,

        /// Tenant ID.
        #[arg(short, long, default_value = "1")]
        tenant: u64,
    },

    /// Execute a single SQL query.
    Query {
        /// SQL query string.
        sql: String,

        /// Server address.
        #[arg(short = 's', long, default_value = "127.0.0.1:5432")]
        server: String,

        /// Tenant ID.
        #[arg(short, long, default_value = "1")]
        tenant: u64,

        /// Query at a specific position (optional).
        #[arg(short, long)]
        at: Option<u64>,
    },

    /// Stream management commands.
    #[command(subcommand)]
    Stream(StreamCommands),

    /// Show server information.
    Info {
        /// Server address.
        #[arg(short = 's', long, default_value = "127.0.0.1:5432")]
        server: String,

        /// Tenant ID.
        #[arg(short, long, default_value = "1")]
        tenant: u64,
    },
}

#[derive(Subcommand)]
enum StreamCommands {
    /// Create a new stream.
    Create {
        /// Stream name.
        name: String,

        /// Data classification (non-phi, phi, deidentified).
        #[arg(short, long, default_value = "non-phi")]
        class: String,

        /// Server address.
        #[arg(short = 's', long, default_value = "127.0.0.1:5432")]
        server: String,

        /// Tenant ID.
        #[arg(short, long, default_value = "1")]
        tenant: u64,
    },

    /// List all streams (placeholder - requires server-side support).
    List {
        /// Server address.
        #[arg(short = 's', long, default_value = "127.0.0.1:5432")]
        server: String,

        /// Tenant ID.
        #[arg(short, long, default_value = "1")]
        tenant: u64,
    },

    /// Append events to a stream.
    Append {
        /// Stream ID.
        stream_id: u64,

        /// Events to append (as JSON strings).
        events: Vec<String>,

        /// Server address.
        #[arg(short = 's', long, default_value = "127.0.0.1:5432")]
        server: String,

        /// Tenant ID.
        #[arg(short, long, default_value = "1")]
        tenant: u64,
    },

    /// Read events from a stream.
    Read {
        /// Stream ID.
        stream_id: u64,

        /// Starting offset.
        #[arg(short, long, default_value = "0")]
        from: u64,

        /// Maximum bytes to read.
        #[arg(short, long, default_value = "65536")]
        max_bytes: u64,

        /// Server address.
        #[arg(short = 's', long, default_value = "127.0.0.1:5432")]
        server: String,

        /// Tenant ID.
        #[arg(short, long, default_value = "1")]
        tenant: u64,
    },
}

fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Version => {
            commands::version::run();
            Ok(())
        }
        Commands::Init { path, development } => commands::init::run(&path, development),
        Commands::Start {
            path,
            address,
            development,
        } => commands::start::run(&path, &address, development),
        Commands::Repl { address, tenant } => commands::repl::run(&address, tenant),
        Commands::Query {
            sql,
            server,
            tenant,
            at,
        } => commands::query::run(&server, tenant, &sql, at),
        Commands::Stream(cmd) => match cmd {
            StreamCommands::Create {
                name,
                class,
                server,
                tenant,
            } => commands::stream::create(&server, tenant, &name, &class),
            StreamCommands::List { server, tenant } => commands::stream::list(&server, tenant),
            StreamCommands::Append {
                stream_id,
                events,
                server,
                tenant,
            } => commands::stream::append(&server, tenant, stream_id, events),
            StreamCommands::Read {
                stream_id,
                from,
                max_bytes,
                server,
                tenant,
            } => commands::stream::read(&server, tenant, stream_id, from, max_bytes),
        },
        Commands::Info { server, tenant } => commands::info::run(&server, tenant),
    }
}

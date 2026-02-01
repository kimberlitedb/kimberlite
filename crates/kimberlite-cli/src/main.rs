//! Kimberlite unified CLI.
//!
//! The compliance-first database for regulated industries.
//!
//! # Quick Start
//!
//! ```bash
//! # Initialize a project
//! kmb init
//!
//! # Start development server (DB + Studio)
//! kmb dev
//!
//! # Connect with the REPL
//! kmb repl --tenant 1
//! ```

mod commands;
mod style;

use anyhow::Result;
use clap::{Parser, Subcommand};
use kimberlite_dev::DevConfig;

/// Kimberlite - the compliance-first database for regulated industries.
#[derive(Parser)]
#[command(name = "kmb")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    /// Disable colored output.
    #[arg(long, global = true)]
    no_color: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show version information.
    Version,

    /// Initialize a new Kimberlite project.
    Init {
        /// Project directory path (default: current directory).
        #[arg(default_value = ".")]
        path: String,

        /// Skip interactive prompts and use defaults.
        #[arg(long)]
        yes: bool,
    },

    /// Start development server (DB + Studio + auto-migration).
    Dev {
        /// Project directory.
        #[arg(short, long, default_value = ".")]
        project: String,

        /// Skip auto-migration.
        #[arg(long)]
        no_migrate: bool,

        /// Skip Studio UI.
        #[arg(long)]
        no_studio: bool,

        /// Start in cluster mode.
        #[arg(long)]
        cluster: bool,

        /// Custom database port.
        #[arg(long)]
        port: Option<u16>,

        /// Custom Studio port.
        #[arg(long)]
        studio_port: Option<u16>,
    },

    /// Start the Kimberlite server (production mode).
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

        /// Tenant ID (required).
        #[arg(short, long)]
        tenant: u64,
    },

    /// Execute a single SQL query.
    Query {
        /// SQL query string.
        sql: String,

        /// Server address.
        #[arg(short = 's', long, default_value = "127.0.0.1:5432")]
        server: String,

        /// Tenant ID (required).
        #[arg(short, long)]
        tenant: u64,

        /// Query at a specific offset (time-travel).
        #[arg(short, long)]
        at: Option<u64>,
    },

    /// Tenant management commands.
    #[command(subcommand)]
    Tenant(TenantCommands),

    /// Cluster management commands.
    #[command(subcommand)]
    Cluster(ClusterCommands),

    /// Migration workflow commands.
    #[command(subcommand)]
    Migration(MigrationCommands),

    /// Launch Studio web UI.
    Studio {
        /// Project directory.
        #[arg(short, long, default_value = ".")]
        project: String,

        /// Custom port.
        #[arg(short, long)]
        port: Option<u16>,
    },

    /// Stream management commands.
    #[command(subcommand)]
    Stream(StreamCommands),

    /// Simulation and verification commands.
    #[command(subcommand)]
    Sim(SimCommands),

    /// Configuration management commands.
    #[command(subcommand)]
    Config(ConfigCommands),

    /// Show server information.
    Info {
        /// Server address.
        #[arg(short = 's', long, default_value = "127.0.0.1:5432")]
        server: String,

        /// Tenant ID.
        #[arg(short, long)]
        tenant: u64,
    },

    /// Generate shell completions.
    Completion {
        /// Shell to generate completions for.
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
}

#[derive(Subcommand)]
enum TenantCommands {
    /// Create a new tenant.
    Create {
        /// Tenant ID.
        #[arg(short, long)]
        id: u64,

        /// Tenant name.
        #[arg(short, long)]
        name: String,

        /// Server address.
        #[arg(short = 's', long, default_value = "127.0.0.1:5432")]
        server: String,

        /// Force creation without confirmation (production).
        #[arg(long)]
        force: bool,
    },

    /// List all tenants.
    List {
        /// Server address.
        #[arg(short = 's', long, default_value = "127.0.0.1:5432")]
        server: String,
    },

    /// Delete a tenant.
    Delete {
        /// Tenant ID.
        #[arg(short, long)]
        id: u64,

        /// Server address.
        #[arg(short = 's', long, default_value = "127.0.0.1:5432")]
        server: String,

        /// Force deletion without confirmation.
        #[arg(long)]
        force: bool,
    },

    /// Show tenant information.
    Info {
        /// Tenant ID.
        #[arg(short, long)]
        id: u64,

        /// Server address.
        #[arg(short = 's', long, default_value = "127.0.0.1:5432")]
        server: String,
    },
}

#[derive(Subcommand)]
enum ClusterCommands {
    /// Initialize a new cluster configuration.
    Init {
        /// Number of nodes.
        #[arg(short, long, default_value = "3")]
        nodes: u32,

        /// Project directory.
        #[arg(short, long, default_value = ".")]
        project: String,
    },

    /// Start all cluster nodes.
    Start {
        /// Project directory.
        #[arg(short, long, default_value = ".")]
        project: String,
    },

    /// Stop cluster node(s).
    Stop {
        /// Node ID to stop (if not specified, stops all).
        #[arg(long)]
        node: Option<u32>,

        /// Project directory.
        #[arg(short, long, default_value = ".")]
        project: String,
    },

    /// Show cluster status.
    Status {
        /// Project directory.
        #[arg(short, long, default_value = ".")]
        project: String,
    },

    /// Destroy cluster configuration.
    Destroy {
        /// Project directory.
        #[arg(short, long, default_value = ".")]
        project: String,

        /// Skip confirmation prompt.
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand)]
enum MigrationCommands {
    /// Create a new migration file.
    Create {
        /// Migration name.
        name: String,

        /// Project directory.
        #[arg(short, long, default_value = ".")]
        project: String,
    },

    /// Apply pending migrations.
    Apply {
        /// Apply up to specific migration number.
        #[arg(long)]
        to: Option<u64>,

        /// Project directory.
        #[arg(short, long, default_value = ".")]
        project: String,
    },

    /// Rollback migrations.
    Rollback {
        /// Number of migrations to rollback.
        #[arg(default_value = "1")]
        count: u64,

        /// Project directory.
        #[arg(short, long, default_value = ".")]
        project: String,
    },

    /// Show migration status.
    Status {
        /// Project directory.
        #[arg(short, long, default_value = ".")]
        project: String,
    },

    /// Validate migration files.
    Validate {
        /// Project directory.
        #[arg(short, long, default_value = ".")]
        project: String,
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

        /// Tenant ID (required).
        #[arg(short, long)]
        tenant: u64,
    },

    /// List all streams.
    List {
        /// Server address.
        #[arg(short = 's', long, default_value = "127.0.0.1:5432")]
        server: String,

        /// Tenant ID (required).
        #[arg(short, long)]
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

        /// Tenant ID (required).
        #[arg(short, long)]
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

        /// Tenant ID (required).
        #[arg(short, long)]
        tenant: u64,
    },
}

#[derive(Subcommand)]
enum SimCommands {
    /// Run simulations.
    Run {
        /// Number of iterations.
        #[arg(short, long, default_value = "100")]
        iterations: u64,

        /// Random seed.
        #[arg(short, long)]
        seed: Option<u64>,

        /// Enable verbose output.
        #[arg(short, long)]
        verbose: bool,
    },

    /// Verify a specific simulation seed.
    Verify {
        /// Seed to reproduce.
        #[arg(short, long)]
        seed: u64,
    },

    /// Generate HTML report.
    Report {
        /// Output file path.
        #[arg(short, long, default_value = "report.html")]
        output: String,
    },
}

#[derive(Subcommand)]
enum ConfigCommands {
    /// Show current configuration.
    Show {
        /// Project directory.
        #[arg(short, long, default_value = ".")]
        project: String,

        /// Output format (text, json, toml).
        #[arg(short, long, default_value = "text")]
        format: String,
    },

    /// Set a configuration value.
    Set {
        /// Configuration key (e.g., database.bind_address).
        key: String,

        /// Configuration value.
        value: String,

        /// Project directory.
        #[arg(short, long, default_value = ".")]
        project: String,
    },

    /// Validate configuration files.
    Validate {
        /// Project directory.
        #[arg(short, long, default_value = ".")]
        project: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    let cli = Cli::parse();

    // Handle --no-color flag and NO_COLOR environment variable
    let no_color = cli.no_color || std::env::var("NO_COLOR").is_ok();
    if no_color {
        owo_colors::set_override(false);
        style::set_no_color(true);
    }

    match cli.command {
        Commands::Version => {
            commands::version::run();
            Ok(())
        }
        Commands::Init { path, yes } => commands::init::run(&path, false),
        Commands::Dev {
            project,
            no_migrate,
            no_studio,
            cluster,
            port,
            studio_port,
        } => {
            let config = DevConfig {
                project_dir: project,
                no_migrate,
                no_studio,
                cluster,
                port,
                studio_port,
            };
            kimberlite_dev::run_dev_server(config).await
        }
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
        Commands::Tenant(cmd) => match cmd {
            TenantCommands::Create {
                id,
                name,
                server,
                force,
            } => commands::tenant::create(&server, id, &name, force),
            TenantCommands::List { server } => commands::tenant::list(&server),
            TenantCommands::Delete { id, server, force } => {
                commands::tenant::delete(&server, id, force)
            }
            TenantCommands::Info { id, server } => commands::tenant::info(&server, id),
        },
        Commands::Cluster(cmd) => match cmd {
            ClusterCommands::Init { nodes, project } => {
                println!("Cluster init not yet implemented (Phase 5)");
                Ok(())
            }
            ClusterCommands::Start { project } => {
                println!("Cluster start not yet implemented (Phase 5)");
                Ok(())
            }
            ClusterCommands::Stop { node, project } => {
                println!("Cluster stop not yet implemented (Phase 5)");
                Ok(())
            }
            ClusterCommands::Status { project } => {
                println!("Cluster status not yet implemented (Phase 5)");
                Ok(())
            }
            ClusterCommands::Destroy { project, force } => {
                println!("Cluster destroy not yet implemented (Phase 5)");
                Ok(())
            }
        },
        Commands::Migration(cmd) => match cmd {
            MigrationCommands::Create { name, project } => {
                println!("Migration create not yet implemented (Phase 4)");
                Ok(())
            }
            MigrationCommands::Apply { to, project } => {
                println!("Migration apply not yet implemented (Phase 4)");
                Ok(())
            }
            MigrationCommands::Rollback { count, project } => {
                println!("Migration rollback not yet implemented (Phase 4)");
                Ok(())
            }
            MigrationCommands::Status { project } => {
                println!("Migration status not yet implemented (Phase 4)");
                Ok(())
            }
            MigrationCommands::Validate { project } => {
                println!("Migration validate not yet implemented (Phase 4)");
                Ok(())
            }
        },
        Commands::Studio { project, port } => {
            println!("Studio not yet implemented (Phase 3)");
            Ok(())
        }
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
        Commands::Sim(cmd) => match cmd {
            SimCommands::Run {
                iterations,
                seed,
                verbose,
            } => {
                println!("Sim run not yet implemented (Phase 6)");
                Ok(())
            }
            SimCommands::Verify { seed } => {
                println!("Sim verify not yet implemented (Phase 6)");
                Ok(())
            }
            SimCommands::Report { output } => {
                println!("Sim report not yet implemented (Phase 6)");
                Ok(())
            }
        },
        Commands::Config(cmd) => match cmd {
            ConfigCommands::Show { project, format } => commands::config::show(&project, &format),
            ConfigCommands::Set {
                key,
                value,
                project,
            } => commands::config::set(&project, &key, &value),
            ConfigCommands::Validate { project } => commands::config::validate(&project),
        },
        Commands::Info { server, tenant } => commands::info::run(&server, tenant),
        Commands::Completion { shell } => {
            println!("Completion not yet implemented (Phase 7)");
            Ok(())
        }
    }
}

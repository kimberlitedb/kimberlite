//! Kimberlite Compliance Reporter CLI
//!
//! Generate compliance reports and verify framework requirements.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use kimberlite_compliance::{ComplianceFramework, ComplianceReport};
use std::path::PathBuf;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[derive(Parser)]
#[command(name = "kimberlite-compliance")]
#[command(version, about = "Kimberlite compliance reporting and verification", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate a compliance report
    Report {
        /// Framework to generate report for
        #[arg(short, long, value_parser = clap::value_parser!(ComplianceFramework))]
        framework: ComplianceFramework,

        /// Output format (json or pdf)
        #[arg(short, long, default_value = "pdf")]
        format: OutputFormat,

        /// Output file path
        #[arg(short, long)]
        output: PathBuf,
    },

    /// Verify compliance for a framework
    Verify {
        /// Framework to verify
        #[arg(short, long, value_parser = clap::value_parser!(ComplianceFramework))]
        framework: ComplianceFramework,

        /// Show detailed requirement status
        #[arg(short, long)]
        detailed: bool,
    },

    /// List all supported frameworks
    Frameworks,

    /// Show core compliance properties status
    Properties,

    /// Generate reports for all frameworks
    ReportAll {
        /// Output directory
        #[arg(short, long, default_value = "compliance-reports")]
        output_dir: PathBuf,

        /// Output format (json or pdf)
        #[arg(short, long, default_value = "pdf")]
        format: OutputFormat,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputFormat {
    Json,
    Pdf,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "json" => Ok(Self::Json),
            "pdf" => Ok(Self::Pdf),
            _ => Err(format!("Invalid format: {}. Use 'json' or 'pdf'", s)),
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Setup logging
    let level = if cli.verbose { Level::DEBUG } else { Level::INFO };
    let subscriber = FmtSubscriber::builder()
        .with_max_level(level)
        .finish();
    tracing::subscriber::set_global_default(subscriber)
        .context("Failed to set tracing subscriber")?;

    match cli.command {
        Commands::Report { framework, format, output } => {
            generate_report(framework, format, &output)?;
        }
        Commands::Verify { framework, detailed } => {
            verify_framework(framework, detailed)?;
        }
        Commands::Frameworks => {
            list_frameworks();
        }
        Commands::Properties => {
            show_properties()?;
        }
        Commands::ReportAll { output_dir, format } => {
            generate_all_reports(&output_dir, format)?;
        }
    }

    Ok(())
}

fn generate_report(
    framework: ComplianceFramework,
    format: OutputFormat,
    output: &PathBuf,
) -> Result<()> {
    info!("Generating {} compliance report...", framework);

    let report = ComplianceReport::generate(framework)
        .with_context(|| format!("Failed to generate {} report", framework))?;

    match format {
        OutputFormat::Json => {
            report.to_json_file(output)
                .with_context(|| format!("Failed to write JSON report to {:?}", output))?;
            println!("✓ JSON report written to: {}", output.display());
        }
        OutputFormat::Pdf => {
            report.to_pdf_file(output)
                .with_context(|| format!("Failed to write PDF report to {:?}", output))?;
            println!("✓ PDF report written to: {}", output.display());
        }
    }

    // Print summary
    println!("\nCompliance Summary:");
    println!("  Framework: {}", framework.full_name());
    println!(
        "  Requirements: {} verified / {} total ({:.1}%)",
        report.certificate.verified_count,
        report.certificate.total_requirements,
        report.certificate.verification_percentage()
    );
    println!("  Status: {}", if report.certificate.is_complete() {
        "✓ COMPLIANT"
    } else {
        "⚠ INCOMPLETE"
    });

    Ok(())
}

fn verify_framework(framework: ComplianceFramework, detailed: bool) -> Result<()> {
    info!("Verifying {} compliance...", framework);

    let report = ComplianceReport::generate(framework)
        .with_context(|| format!("Failed to generate {} report", framework))?;

    println!("Framework: {}", framework.full_name());
    println!("Specification: {}", framework.spec_path());
    println!();

    // Core properties
    println!("Core Properties:");
    for (property, satisfied) in &report.core_properties {
        let status = if *satisfied { "✓" } else { "✗" };
        println!("  {} {}", status, property);
    }
    println!();

    // Requirements
    println!("Requirements ({} total):", report.requirements.len());
    for req in &report.requirements {
        println!("  {} {} - {}", req.status, req.id, req.description);
        if detailed {
            println!("      Theorem: {}", req.theorem);
            println!("      Proof: {}", req.proof_file);
            if let Some(notes) = &req.notes {
                println!("      Notes: {}", notes);
            }
        }
    }
    println!();

    // Overall status
    let verified = report.certificate.verified_count;
    let total = report.certificate.total_requirements;
    let percentage = report.certificate.verification_percentage();

    println!("Verification Status:");
    println!("  Verified: {} / {} ({:.1}%)", verified, total, percentage);
    println!("  Toolchain: {}", report.certificate.toolchain_version);
    println!("  Generated: {}", report.generated_at.format("%Y-%m-%d %H:%M:%S UTC"));
    println!();

    if report.certificate.is_complete() {
        println!("✓ {} compliance requirements SATISFIED", framework);
    } else {
        println!("⚠ {} compliance requirements INCOMPLETE", framework);
        println!("  {}/{} requirements still need verification", total - verified, total);
    }

    Ok(())
}

fn list_frameworks() {
    println!("Supported Compliance Frameworks:");
    println!();
    for framework in ComplianceFramework::all() {
        println!("  {} - {}", framework, framework.full_name());
        println!("      Specification: {}", framework.spec_path());
        println!();
    }
}

fn show_properties() -> Result<()> {
    println!("Core Compliance Properties:");
    println!();

    // Generate a sample report to get property status
    let report = ComplianceReport::generate(ComplianceFramework::HIPAA)?;

    for (property, satisfied) in &report.core_properties {
        let status = if *satisfied { "✓ Satisfied" } else { "✗ Not satisfied" };
        println!("  {} - {}", property, status);
    }
    println!();

    println!("Meta-Framework Theorem:");
    println!("  CoreComplianceSafety => AllFrameworksCompliant");
    println!();
    println!("This means proving these 7 core properties implies compliance");
    println!("with ALL 6 frameworks (HIPAA, GDPR, SOC 2, PCI DSS, ISO 27001, FedRAMP).");
    println!();
    println!("Proof complexity reduction: ~23× fewer proofs required");

    Ok(())
}

fn generate_all_reports(output_dir: &PathBuf, format: OutputFormat) -> Result<()> {
    info!("Generating reports for all frameworks...");

    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("Failed to create output directory: {:?}", output_dir))?;

    let extension = match format {
        OutputFormat::Json => "json",
        OutputFormat::Pdf => "pdf",
    };

    for framework in ComplianceFramework::all() {
        let filename = format!("{}_compliance_report.{}",
            framework.to_string().to_lowercase().replace(' ', "_"),
            extension
        );
        let output_path = output_dir.join(filename);

        info!("Generating {} report...", framework);
        generate_report(framework, format, &output_path)?;
    }

    println!();
    println!("✓ All reports generated successfully");
    println!("  Output directory: {}", output_dir.display());
    println!("  Total reports: {}", ComplianceFramework::all().len());

    Ok(())
}

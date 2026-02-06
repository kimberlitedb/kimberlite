//! Kimberlite Compliance Reporter CLI
//!
//! Generate compliance reports and verify framework requirements.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use kimberlite_compliance::{ComplianceFramework, ComplianceReport};
use std::path::PathBuf;
use tracing::{Level, info};
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
    /// Generate a proof certificate
    Generate {
        /// Framework to generate certificate for
        #[arg(long, value_parser = clap::value_parser!(ComplianceFramework))]
        framework: ComplianceFramework,

        /// Output file path (JSON)
        #[arg(long)]
        output: PathBuf,
    },

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
            _ => Err(format!("Invalid format: {s}. Use 'json' or 'pdf'")),
        }
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Setup logging
    let level = if cli.verbose {
        Level::DEBUG
    } else {
        Level::INFO
    };
    let subscriber = FmtSubscriber::builder().with_max_level(level).finish();
    tracing::subscriber::set_global_default(subscriber)
        .context("Failed to set tracing subscriber")?;

    match cli.command {
        Commands::Generate { framework, output } => {
            generate_certificate(framework, &output)?;
        }
        Commands::Report {
            framework,
            format,
            output,
        } => {
            generate_report(framework, format, &output)?;
        }
        Commands::Verify {
            framework,
            detailed,
        } => {
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

fn generate_certificate(framework: ComplianceFramework, output: &PathBuf) -> Result<()> {
    use kimberlite_compliance::certificate;

    info!("Generating proof certificate for {framework}...");

    let cert = certificate::generate_certificate(framework)
        .with_context(|| format!("Failed to generate certificate for {framework}"))?;

    // Serialize to JSON
    let json = serde_json::to_string_pretty(&cert).context("Failed to serialize certificate")?;

    let output_display = output.display();
    std::fs::write(output, json)
        .with_context(|| format!("Failed to write certificate to {output_display}"))?;

    println!("✓ Certificate generated: {output_display}");
    println!();
    let full_name = framework.full_name();
    let spec_hash = &cert.spec_hash;
    let total_requirements = cert.total_requirements;
    let verified_count = cert.verified_count;
    let verification_pct = cert.verification_percentage();
    println!("Framework: {full_name}");
    println!("Spec Hash: {spec_hash}");
    println!("Total Requirements: {total_requirements}");
    println!("Verified Count: {verified_count}");
    println!("Verification: {verification_pct:.1}%");

    Ok(())
}

fn generate_report(
    framework: ComplianceFramework,
    format: OutputFormat,
    output: &PathBuf,
) -> Result<()> {
    info!("Generating {framework} compliance report...");

    let report = ComplianceReport::generate(framework)
        .with_context(|| format!("Failed to generate {framework} report"))?;

    let output_display = output.display();
    match format {
        OutputFormat::Json => {
            report
                .to_json_file(output)
                .with_context(|| format!("Failed to write JSON report to {output_display}"))?;
            println!("✓ JSON report written to: {output_display}");
        }
        OutputFormat::Pdf => {
            report
                .to_pdf_file(output)
                .with_context(|| format!("Failed to write PDF report to {output_display}"))?;
            println!("✓ PDF report written to: {output_display}");
        }
    }

    // Print summary
    let full_name = framework.full_name();
    let verified_count = report.certificate.verified_count;
    let total_requirements = report.certificate.total_requirements;
    let verification_pct = report.certificate.verification_percentage();
    let status = if report.certificate.is_complete() {
        "✓ COMPLIANT"
    } else {
        "⚠ INCOMPLETE"
    };
    println!("\nCompliance Summary:");
    println!("  Framework: {full_name}");
    println!(
        "  Requirements: {verified_count} verified / {total_requirements} total ({verification_pct:.1}%)"
    );
    println!("  Status: {status}");

    Ok(())
}

fn verify_framework(framework: ComplianceFramework, detailed: bool) -> Result<()> {
    info!("Verifying {framework} compliance...");

    let report = ComplianceReport::generate(framework)
        .with_context(|| format!("Failed to generate {framework} report"))?;

    let full_name = framework.full_name();
    let spec_path = framework.spec_path();
    println!("Framework: {full_name}");
    println!("Specification: {spec_path}");
    println!();

    // Core properties
    println!("Core Properties:");
    for (property, satisfied) in &report.core_properties {
        let status = if *satisfied { "✓" } else { "✗" };
        println!("  {status} {property}");
    }
    println!();

    // Requirements
    let req_count = report.requirements.len();
    println!("Requirements ({req_count} total):");
    for req in &report.requirements {
        let req_status = &req.status;
        let req_id = &req.id;
        let req_desc = &req.description;
        println!("  {req_status} {req_id} - {req_desc}");
        if detailed {
            let theorem = &req.theorem;
            let proof_file = &req.proof_file;
            println!("      Theorem: {theorem}");
            println!("      Proof: {proof_file}");
            if let Some(notes) = &req.notes {
                println!("      Notes: {notes}");
            }
        }
    }
    println!();

    // Overall status
    let verified = report.certificate.verified_count;
    let total = report.certificate.total_requirements;
    let percentage = report.certificate.verification_percentage();
    let toolchain = &report.certificate.toolchain_version;
    let generated = report.generated_at.format("%Y-%m-%d %H:%M:%S UTC");

    println!("Verification Status:");
    println!("  Verified: {verified} / {total} ({percentage:.1}%)");
    println!("  Toolchain: {toolchain}");
    println!("  Generated: {generated}");
    println!();

    if report.certificate.is_complete() {
        println!("✓ {framework} compliance requirements SATISFIED");
    } else {
        let remaining = total - verified;
        println!("⚠ {framework} compliance requirements INCOMPLETE");
        println!("  {remaining}/{total} requirements still need verification");
    }

    Ok(())
}

fn list_frameworks() {
    println!("Supported Compliance Frameworks:");
    println!();
    for framework in ComplianceFramework::all() {
        let full_name = framework.full_name();
        let spec_path = framework.spec_path();
        println!("  {framework} - {full_name}");
        println!("      Specification: {spec_path}");
        println!();
    }
}

fn show_properties() -> Result<()> {
    println!("Core Compliance Properties:");
    println!();

    // Generate a sample report to get property status
    let report = ComplianceReport::generate(ComplianceFramework::HIPAA)?;

    for (property, satisfied) in &report.core_properties {
        let status = if *satisfied {
            "✓ Satisfied"
        } else {
            "✗ Not satisfied"
        };
        println!("  {property} - {status}");
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

    let output_dir_display = output_dir.display();
    std::fs::create_dir_all(output_dir)
        .with_context(|| format!("Failed to create output directory: {output_dir_display}"))?;

    let extension = match format {
        OutputFormat::Json => "json",
        OutputFormat::Pdf => "pdf",
    };

    for framework in ComplianceFramework::all() {
        let framework_str = framework.to_string().to_lowercase().replace(' ', "_");
        let filename = format!("{framework_str}_compliance_report.{extension}");
        let output_path = output_dir.join(filename);

        info!("Generating {framework} report...");
        generate_report(framework, format, &output_path)?;
    }

    let total_reports = ComplianceFramework::all().len();
    println!();
    println!("✓ All reports generated successfully");
    println!("  Output directory: {output_dir_display}");
    println!("  Total reports: {total_reports}");

    Ok(())
}

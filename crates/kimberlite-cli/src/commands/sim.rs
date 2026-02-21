//! Simulation and verification commands (VOPR integration).

use anyhow::Result;
use kimberlite_sim::{ScenarioType, VoprConfig, VoprRunner};

use crate::style::{self, colors::SemanticStyle};

/// Runs VOPR simulations.
pub fn run(iterations: u64, seed: Option<u64>, verbose: bool) -> Result<()> {
    let config = VoprConfig {
        seed: seed.unwrap_or(0),
        iterations,
        verbose,
        ..Default::default()
    };

    if !verbose {
        println!(
            "Running {} VOPR simulations (seed: {})...",
            iterations.to_string().header(),
            config.seed.to_string().code()
        );
    }

    let runner = VoprRunner::new(config);
    let results = runner.run_batch();

    // Output summary
    println!();
    if results.all_passed() {
        println!(
            "{} All {} simulations passed",
            style::success("✓"),
            results.successes.to_string().success()
        );
    } else {
        println!(
            "{} {} passed, {} failed",
            style::error("✗"),
            results.successes.to_string().success(),
            results.failures.to_string().error()
        );
    }

    println!(
        "  Time: {:.2}s ({:.0} sims/sec)",
        results.elapsed_secs,
        results.rate()
    );

    if !results.failed_seeds.is_empty() {
        println!();
        println!("{}", "Failed seeds (reproduce with):".warning());
        for seed in &results.failed_seeds {
            println!("  {} sim verify --seed {}", "kmb".code(), seed);
        }
    }

    if results.all_passed() {
        Ok(())
    } else {
        anyhow::bail!("{} simulation(s) failed", results.failures)
    }
}

/// Verifies a specific seed with verbose output.
pub fn verify(seed: u64) -> Result<()> {
    println!(
        "Verifying seed {} with verbose output...",
        seed.to_string().code()
    );
    println!();

    let config = VoprConfig {
        seed,
        iterations: 1,
        verbose: true,
        failure_diagnosis: true,
        ..Default::default()
    };

    let runner = VoprRunner::new(config);
    let result = runner.run_single(seed);

    match result {
        kimberlite_sim::VoprResult::Success {
            events_processed,
            final_time_ns,
            ..
        } => {
            println!();
            println!("{} Simulation passed", style::success("✓"));
            println!("  Events: {}", events_processed.to_string().muted());
            #[allow(clippy::cast_precision_loss)]
            let time_ms = final_time_ns as f64 / 1_000_000.0;
            println!("  Simulated time: {time_ms:.2}ms");
            Ok(())
        }
        kimberlite_sim::VoprResult::InvariantViolation {
            invariant,
            message,
            events_processed,
            failure_report,
            ..
        } => {
            println!();
            println!(
                "{} Simulation failed at event {}",
                style::error("✗"),
                events_processed
            );
            println!("  Invariant: {}", invariant.error());
            println!("  Message: {message}");

            if let Some(report) = failure_report {
                println!();
                println!(
                    "{}",
                    kimberlite_sim::diagnosis::FailureAnalyzer::format_report(&report)
                );
            }

            anyhow::bail!("Simulation failed: {invariant}")
        }
    }
}

/// Generates HTML report from simulation results.
///
/// Runs a quick simulation batch and generates an HTML report with results.
pub fn report(output: &str) -> Result<()> {
    println!("Running simulations for report: {}...", output.code());

    // Run a batch of simulations for the report
    let config = VoprConfig {
        seed: 0,
        iterations: 100,
        verbose: false,
        ..Default::default()
    };
    let runner = VoprRunner::new(config);
    let results = runner.run_batch();

    let total = results.successes + results.failures;
    let success_rate = if total > 0 {
        (results.successes as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    let failed_seeds_html: String = if results.failed_seeds.is_empty() {
        "<p class=\"success\">No failures detected.</p>".to_string()
    } else {
        let mut s = String::from("<ul>");
        for seed in &results.failed_seeds {
            use std::fmt::Write as _;
            write!(s, "<li>Seed <code>{seed}</code> — reproduce with: <code>kmb sim verify --seed {seed}</code></li>").unwrap();
        }
        s.push_str("</ul>");
        s
    };

    let status_class = if results.all_passed() {
        "success"
    } else {
        "error"
    };

    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Kimberlite VOPR Report</title>
    <style>
        body {{
            font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
            max-width: 1200px;
            margin: 0 auto;
            padding: 2rem;
            background: #f5f5f5;
        }}
        h1 {{ color: #333; }}
        .summary {{
            background: white;
            padding: 2rem;
            border-radius: 8px;
            margin-bottom: 2rem;
            box-shadow: 0 2px 4px rgba(0,0,0,0.1);
        }}
        .stat {{
            display: inline-block;
            margin-right: 2rem;
        }}
        .stat-label {{
            color: #666;
            font-size: 0.875rem;
            text-transform: uppercase;
        }}
        .stat-value {{
            font-size: 2rem;
            font-weight: bold;
            color: #333;
        }}
        .success {{ color: #22c55e; }}
        .error {{ color: #ef4444; }}
        code {{
            background: #e5e7eb;
            padding: 0.15rem 0.4rem;
            border-radius: 4px;
            font-size: 0.875rem;
        }}
    </style>
</head>
<body>
    <h1>Kimberlite VOPR Simulation Report</h1>
    <div class="summary">
        <h2>Summary</h2>
        <div class="stat">
            <div class="stat-label">Total Runs</div>
            <div class="stat-value">{total}</div>
        </div>
        <div class="stat">
            <div class="stat-label">Successes</div>
            <div class="stat-value success">{successes}</div>
        </div>
        <div class="stat">
            <div class="stat-label">Failures</div>
            <div class="stat-value error">{failures}</div>
        </div>
        <div class="stat">
            <div class="stat-label">Success Rate</div>
            <div class="stat-value {status_class}">{rate:.1}%</div>
        </div>
        <div class="stat">
            <div class="stat-label">Time</div>
            <div class="stat-value">{elapsed:.2}s</div>
        </div>
        <div class="stat">
            <div class="stat-label">Throughput</div>
            <div class="stat-value">{throughput:.0}/s</div>
        </div>
    </div>
    <div class="summary">
        <h2>Failed Seeds</h2>
        {failed_seeds_html}
    </div>
</body>
</html>
"#,
        total = total,
        successes = results.successes,
        failures = results.failures,
        rate = success_rate,
        status_class = status_class,
        elapsed = results.elapsed_secs,
        throughput = results.rate(),
        failed_seeds_html = failed_seeds_html,
    );

    std::fs::write(output, html)?;

    println!();
    println!(
        "{} Report generated ({} runs, {:.1}% pass rate)",
        style::success("✓"),
        total,
        success_rate
    );
    println!("  File: {}", output.code());

    Ok(())
}

/// Lists available test scenarios.
pub fn list_scenarios() -> Result<()> {
    println!("Available VOPR Test Scenarios:");
    println!();

    for scenario in ScenarioType::all() {
        println!("  {}", scenario.name().header());
        println!("    {}", scenario.description().muted());
        println!();
    }

    Ok(())
}

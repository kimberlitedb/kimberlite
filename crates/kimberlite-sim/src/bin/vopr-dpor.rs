//! VOPR-DPOR: Dynamic Partial Order Reduction driver for VOPR.
//!
//! Runs a baseline VOPR simulation, computes the Mazurkiewicz equivalence
//! classes via DPOR, then fuzzes additional seeds and reports how many
//! distinct classes were covered versus predicted.
//!
//! Usage:
//!   vopr-dpor [--seed N] [--explore N] [--alternatives N] [--max-events N] [--json]
//!
//! This is an exploration tool: it does NOT yet force replay of specific
//! schedules. It measures how well seed-based fuzzing explores the state
//! space that DPOR predicts is reachable by adjacent-swap interleavings.

use kimberlite_sim::dpor_runner::{DporRunner, DporRunnerConfig};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut config = DporRunnerConfig::default();
    let mut json_mode = false;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--seed" | "-s" => {
                i += 1;
                if i < args.len() {
                    config.baseline_seed = args[i].parse().unwrap_or(0);
                }
            }
            "--explore" | "-n" => {
                i += 1;
                if i < args.len() {
                    config.exploration_seeds = args[i].parse().unwrap_or(100);
                }
            }
            "--alternatives" | "-a" => {
                i += 1;
                if i < args.len() {
                    config.max_alternatives = args[i].parse().unwrap_or(50);
                }
            }
            "--max-events" => {
                i += 1;
                if i < args.len() {
                    config.vopr_max_events = args[i].parse().unwrap_or(10_000);
                }
            }
            "--max-trace" => {
                i += 1;
                if i < args.len() {
                    config.max_events_per_trace = args[i].parse().unwrap_or(5_000);
                }
            }
            "--json" => {
                json_mode = true;
            }
            "--help" | "-h" => {
                print_usage();
                return;
            }
            _ => {
                eprintln!("Unknown argument: {}", args[i]);
                print_usage();
                std::process::exit(2);
            }
        }
        i += 1;
    }

    if !json_mode {
        println!("VOPR-DPOR — Dynamic Partial Order Reduction driver");
        println!("===================================================");
        println!("  Baseline seed: {}", config.baseline_seed);
        println!("  Exploration seeds: {}", config.exploration_seeds);
        println!("  Max alternatives: {}", config.max_alternatives);
        println!("  Max events per trace: {}", config.max_events_per_trace);
        println!();
    }

    let runner = DporRunner::new(config.clone());
    let start = std::time::Instant::now();
    let report = runner.run();
    let elapsed = start.elapsed();

    if json_mode {
        let json = serde_json::json!({
            "baseline_seed": config.baseline_seed,
            "exploration_seeds": config.exploration_seeds,
            "max_alternatives": config.max_alternatives,
            "elapsed_secs": elapsed.as_secs_f64(),
            "baseline_signature": format!("{:016x}", report.baseline_signature),
            "baseline_length": report.baseline_length,
            "classes_covered": report.classes_covered,
            "classes_total": report.classes_total,
            "classes_coverage_pct": if report.classes_total > 0 {
                100.0 * report.classes_covered as f64 / report.classes_total as f64
            } else {
                0.0
            },
            "seeds_discovered_new_class": report.seeds_discovered_new_class,
            "seeds_duplicate_class": report.seeds_duplicate_class,
            "explorer_alternatives_explored": report.explorer_stats.alternatives_explored,
            "explorer_duplicates_skipped": report.explorer_stats.duplicates_skipped,
            "explorer_dependency_checks": report.explorer_stats.dependency_checks,
            "outcomes": report.vopr_outcomes.iter().map(|o| {
                serde_json::json!({
                    "seed": o.seed,
                    "success": o.success,
                    "trace_signature": format!("{:016x}", o.trace_signature),
                    "trace_length": o.trace_length,
                    "new_class": o.new_class,
                })
            }).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&json).unwrap());
    } else {
        println!("{}", report.summary());
        println!();
        println!("  Elapsed: {:.2}s", elapsed.as_secs_f64());
        println!(
            "  Coverage: {:.1}%",
            if report.classes_total > 0 {
                100.0 * report.classes_covered as f64 / report.classes_total as f64
            } else {
                0.0
            }
        );
    }
}

fn print_usage() {
    println!(
        "VOPR-DPOR: Dynamic Partial Order Reduction driver\n\
\n\
USAGE:\n\
    vopr-dpor [OPTIONS]\n\
\n\
OPTIONS:\n\
    --seed, -s <N>          Baseline seed (default: 0)\n\
    --explore, -n <N>       Number of exploration seeds (default: 100)\n\
    --alternatives, -a <N>  Max DPOR-predicted alternatives (default: 50)\n\
    --max-events <N>        Max events per simulation (default: 10000)\n\
    --max-trace <N>         Max events captured in trace (default: 5000)\n\
    --json                  Emit JSON instead of human-readable output\n\
    --help, -h              Show this help\n\
\n\
EXAMPLES:\n\
    vopr-dpor --seed 42 --explore 1000    # Explore 1000 alternative seeds\n\
    vopr-dpor -n 100 --json               # JSON output for CI integration\n"
    );
}

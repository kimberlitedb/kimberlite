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
use kimberlite_sim::scenarios::ScenarioType;

fn parse_scenario(name: &str) -> Option<ScenarioType> {
    // Accept common aliases used by the legacy vopr binary.
    match name {
        "baseline" => Some(ScenarioType::Baseline),
        "swizzle" | "swizzle-clogging" => Some(ScenarioType::SwizzleClogging),
        "gray" | "gray-failures" => Some(ScenarioType::GrayFailures),
        "multi-tenant" | "multi-tenant-isolation" => Some(ScenarioType::MultiTenantIsolation),
        "combined" => Some(ScenarioType::Combined),
        "view-change-merge" | "byzantine-view-change-merge" => {
            Some(ScenarioType::ByzantineViewChangeMerge)
        }
        "commit-desync" | "byzantine-commit-desync" => Some(ScenarioType::ByzantineCommitDesync),
        "inflated-commit" | "byzantine-inflated-commit" => {
            Some(ScenarioType::ByzantineInflatedCommit)
        }
        "invalid-metadata" => Some(ScenarioType::ByzantineInvalidMetadata),
        "malicious-view-change" => Some(ScenarioType::ByzantineMaliciousViewChange),
        "leader-race" => Some(ScenarioType::ByzantineLeaderRace),
        "replay-old-view" => Some(ScenarioType::ByzantineReplayOldView),
        "corrupt-checksums" => Some(ScenarioType::ByzantineCorruptChecksums),
        "corruption-bit-flip" | "bit-flip" => Some(ScenarioType::CorruptionBitFlip),
        "corruption-torn-write" | "torn-write" => Some(ScenarioType::CorruptionTornWrite),
        "crash-during-commit" | "crash-commit" => Some(ScenarioType::CrashDuringCommit),
        "crash-during-view-change" | "crash-view-change" => {
            Some(ScenarioType::CrashDuringViewChange)
        }
        "recovery-corrupt-log" | "recovery-corrupt" => Some(ScenarioType::RecoveryCorruptLog),
        _ => None,
    }
}

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
            "--scenario" => {
                i += 1;
                if i < args.len() {
                    match parse_scenario(&args[i]) {
                        Some(s) => config.scenario = Some(s),
                        None => {
                            eprintln!(
                                "Unknown scenario: '{}'. Run --list-scenarios to see options.",
                                args[i]
                            );
                            std::process::exit(2);
                        }
                    }
                }
            }
            "--list-scenarios" => {
                print_scenarios();
                return;
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
        if let Some(scenario) = config.scenario {
            println!("  Scenario: {scenario:?}");
        } else {
            println!("  Scenario: (default — no faults)");
        }
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
    --scenario <NAME>       Scenario to run (default: no-fault baseline)\n\
    --list-scenarios        Print supported scenario names and exit\n\
    --json                  Emit JSON instead of human-readable output\n\
    --help, -h              Show this help\n\
\n\
EXAMPLES:\n\
    vopr-dpor --seed 42 --explore 1000                         # Default scenario\n\
    vopr-dpor --scenario view-change-merge --explore 500       # Byzantine VSR\n\
    vopr-dpor --scenario combined -n 200 --json                # JSON + full faults\n"
    );
}

fn print_scenarios() {
    println!(
        "Supported scenarios:\n\
    baseline                     No faults\n\
    swizzle                      Swizzle-clogging intermittent congestion\n\
    gray                         Gray failures (partial node failures)\n\
    multi-tenant                 Multi-tenant isolation under faults\n\
    combined                     All fault types\n\
    view-change-merge            Byzantine view change merge attack\n\
    commit-desync                Byzantine commit desynchronization\n\
    inflated-commit              Byzantine inflated commit numbers\n\
    invalid-metadata             Byzantine metadata mismatch\n\
    malicious-view-change        Byzantine inconsistent DoViewChange\n\
    leader-race                  Byzantine asymmetric partition race\n\
    replay-old-view              Byzantine replay from old view\n\
    corrupt-checksums            Byzantine checksum corruption\n\
    corruption-bit-flip          Storage bit flip\n\
    corruption-torn-write        Storage torn write\n\
    crash-during-commit          Crash mid-commit\n\
    crash-during-view-change     Crash mid-view-change\n\
    recovery-corrupt-log         Recovery with corrupt log\n"
    );
}

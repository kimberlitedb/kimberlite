//! kimberlite-chaos: run multi-cluster chaos scenarios against real VMs.

use kimberlite_chaos::{ChaosController, ScenarioCatalog};
use kimberlite_chaos::cluster_network;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        print_usage();
        std::process::exit(2);
    }

    match args[1].as_str() {
        "list" => {
            let catalog = ScenarioCatalog::builtin();
            println!("Available chaos scenarios:");
            for s in catalog.list() {
                println!("  {}", s.id);
                println!("    {}", s.description);
            }
        }
        "capabilities" => {
            println!("Host capabilities:");
            println!("{}", cluster_network::host_capabilities_report());
        }
        "run" => {
            if args.len() < 3 {
                eprintln!("error: `run` requires a scenario ID");
                std::process::exit(2);
            }
            let scenario_id = &args[2];
            let catalog = ScenarioCatalog::builtin();
            let Some(scenario) = catalog.find(scenario_id) else {
                eprintln!("error: unknown scenario '{scenario_id}'");
                std::process::exit(2);
            };

            println!("Running chaos scenario: {}", scenario.id);
            println!("  {}", scenario.description);

            let mut controller = ChaosController::new();
            match controller.run(scenario) {
                Ok(report) => {
                    println!("\nResult: {}", if report.success { "PASS" } else { "FAIL" });
                    println!("  duration_ms: {}", report.duration_ms);
                    println!("  actions_executed: {}", report.actions_executed);
                    println!("  invariant_checks: {}", report.invariant_results.len());
                    for r in &report.invariant_results {
                        let tag = if r.held { "✓" } else { "✗" };
                        println!("    {tag} {} — {}", r.invariant, r.message);
                    }
                    if !report.success {
                        std::process::exit(1);
                    }
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    std::process::exit(1);
                }
            }
        }
        _ => {
            print_usage();
            std::process::exit(2);
        }
    }
}

fn print_usage() {
    println!(
        "kimberlite-chaos: multi-cluster chaos testing runner\n\
\n\
USAGE:\n\
    kimberlite-chaos <COMMAND> [ARGS]\n\
\n\
COMMANDS:\n\
    list                     List built-in chaos scenarios\n\
    capabilities             Check host-side tool availability (qemu, iptables, tc)\n\
    run <scenario-id>        Execute a chaos scenario (requires KVM host)\n\
\n\
EXAMPLES:\n\
    kimberlite-chaos list\n\
    kimberlite-chaos capabilities\n\
    kimberlite-chaos run split_brain_prevention\n"
    );
}

//! kimberlite-chaos: run multi-cluster chaos scenarios against real VMs.
//!
//! This binary is Unix-only — see the crate-level platform note. On
//! non-Unix hosts it compiles to a stub that exits with a clear error
//! so `cargo build --workspace` stays green everywhere.

#[cfg(not(unix))]
fn main() {
    eprintln!(
        "kimberlite-chaos: real-VM chaos driver requires a Unix host (QMP UNIX sockets, iptables, tc)."
    );
    std::process::exit(2);
}

#[cfg(unix)]
use kimberlite_chaos::cluster_network;
#[cfg(unix)]
use kimberlite_chaos::{ChaosController, ScenarioCatalog};

#[cfg(unix)]
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
            let apply = args.iter().any(|a| a == "--apply");
            let output_dir = parse_flag(&args, "--output-dir");
            let catalog = ScenarioCatalog::builtin();
            let Some(scenario) = catalog.find(scenario_id) else {
                eprintln!("error: unknown scenario '{scenario_id}'");
                std::process::exit(2);
            };

            println!("Running chaos scenario: {}", scenario.id);
            println!("  {}", scenario.description);
            if apply {
                println!("  mode: APPLY (will execute real host commands)");
            } else {
                println!("  mode: DRY-RUN (pass --apply to execute real commands)");
            }
            if let Some(ref dir) = output_dir {
                println!("  output_dir: {dir}");
                if let Err(e) = std::fs::create_dir_all(dir) {
                    eprintln!("warning: could not create output dir {dir}: {e}");
                }
            }

            let mut controller = if apply {
                ChaosController::with_apply()
            } else {
                ChaosController::new()
            };
            if let Some(ref dir) = output_dir {
                controller.set_output_dir(dir);
            }
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
                    if let Some(ref dir) = output_dir {
                        let report_path = format!("{dir}/report.json");
                        match serde_json::to_string_pretty(&report) {
                            Ok(json) => {
                                if let Err(e) = std::fs::write(&report_path, json) {
                                    eprintln!("warning: failed to write {report_path}: {e}");
                                } else {
                                    println!("  report written: {report_path}");
                                }
                            }
                            Err(e) => eprintln!("warning: failed to serialize report: {e}"),
                        }
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

#[cfg(unix)]
fn parse_flag(args: &[String], name: &str) -> Option<String> {
    let mut i = 0;
    while i < args.len() {
        if args[i] == name {
            return args.get(i + 1).cloned();
        }
        if let Some(rest) = args[i].strip_prefix(&format!("{name}=")) {
            return Some(rest.to_string());
        }
        i += 1;
    }
    None
}

#[cfg(unix)]
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
    run <scenario-id>        Execute a chaos scenario (dry-run by default)\n\
    run <scenario-id> --apply   Execute with real host commands (requires root)\n\
\n\
EXAMPLES:\n\
    kimberlite-chaos list\n\
    kimberlite-chaos capabilities\n\
    kimberlite-chaos run split_brain_prevention                # dry-run\n\
    kimberlite-chaos run split_brain_prevention --apply        # real iptables/tc\n"
    );
}

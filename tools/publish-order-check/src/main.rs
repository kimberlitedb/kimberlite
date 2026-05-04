//! `publish-order-check` — pairwise topological validator for the
//! `PUBLISH_CRATES` list in the workspace `justfile`.
//!
//! AUDIT-2026-05 H-5 — closes the v0.7.0 ROADMAP item
//! "Topological-order validation for `PUBLISH_CRATES` in `justfile`
//! (compare against `cargo metadata`)".
//!
//! ## Algorithm
//!
//! Pairwise dependency walk — NOT a topological-sort comparison.
//! Multiple valid orders exist for any DAG; comparing the
//! human-curated `PUBLISH_CRATES` list against one specific
//! topo-sort would false-positive whenever the curator picks a
//! different (but still valid) ordering. The pairwise check is the
//! only correct invariant: for each pair `(A, B)` where `A` appears
//! before `B` in the list, `B` must NOT depend on `A` *transitively
//! through other workspace crates*. Equivalently: every workspace
//! dep `D` of `A` that is also in `PUBLISH_CRATES` must appear at a
//! lower index than `A`.
//!
//! ## Usage
//!
//!   publish-order-check "<space-separated crate names>"
//!
//! The justfile recipe `validate-publish-order` invokes this with
//! the `PUBLISH_CRATES` env var.

use std::collections::HashMap;
use std::env;
use std::process::{Command, ExitCode};

use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Package {
    name: String,
    id: String,
    dependencies: Vec<Dep>,
}

#[derive(Debug, Deserialize)]
struct Dep {
    name: String,
    /// `kind` is `null` for production deps, `"dev"` for
    /// dev-dependencies, `"build"` for build-dependencies.
    /// Production-published artifacts only embed prod and build
    /// deps in their dependency graph, so dev deps are safe to
    /// skip.
    kind: Option<String>,
    /// `path` is set when the dep resolves to a workspace member.
    path: Option<String>,
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!(
            "usage: publish-order-check \"<crate1> <crate2> ...\"\n\
             expected exactly one argument: a whitespace-separated \
             list of crate names in publish order"
        );
        return ExitCode::FAILURE;
    }
    let order: Vec<&str> = args
        .iter()
        .flat_map(|a| a.split_whitespace())
        .filter(|s| !s.is_empty())
        .collect();
    assert!(
        !order.is_empty(),
        "publish-order-check: no crate names parsed from arguments"
    );

    let metadata = match load_cargo_metadata() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("publish-order-check: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Index: crate name → workspace dep names. Only workspace
    // deps matter for publish order — third-party deps are
    // resolved by crates.io.
    let workspace_set: std::collections::HashSet<&str> =
        metadata.workspace_members.iter().filter_map(|id| {
            // workspace_members is opaque IDs; map back to the
            // package name via the packages table.
            metadata.packages.iter().find(|p| &p.id == id).map(|p| p.name.as_str())
        }).collect();
    let pkg_deps: HashMap<&str, Vec<&str>> = metadata
        .packages
        .iter()
        .filter(|p| workspace_set.contains(p.name.as_str()))
        .map(|p| {
            let deps: Vec<&str> = p
                .dependencies
                .iter()
                // workspace dep AND not a dev-dependency. Production
                // publishes embed prod + build deps; dev-only deps
                // are absent from the published artifact's graph.
                .filter(|d| d.path.is_some() && d.kind.as_deref() != Some("dev"))
                .map(|d| d.name.as_str())
                .collect();
            (p.name.as_str(), deps)
        })
        .collect();

    // For each crate at position i, check every workspace dep:
    // if the dep is also in PUBLISH_CRATES, its index must be <
    // i.
    let position: HashMap<&str, usize> =
        order.iter().enumerate().map(|(i, n)| (*n, i)).collect();
    let mut violations: Vec<String> = Vec::new();
    for (i, crate_name) in order.iter().enumerate() {
        let Some(deps) = pkg_deps.get(crate_name) else {
            // Unknown crate — fail loudly. Either a typo in the
            // list or a crate removed from the workspace without
            // updating PUBLISH_CRATES.
            violations.push(format!(
                "VIOLATION: '{crate_name}' (position {i}) is in PUBLISH_CRATES \
                 but not found as a workspace member in `cargo metadata`"
            ));
            continue;
        };
        for dep in deps {
            if let Some(&dep_position) = position.get(dep) {
                if dep_position >= i {
                    violations.push(format!(
                        "VIOLATION: '{crate_name}' (position {i}) depends on '{dep}' \
                         (position {dep_position}) → '{dep}' must publish BEFORE '{crate_name}'\n  \
                         fix: move '{dep}' above '{crate_name}' in justfile PUBLISH_CRATES"
                    ));
                }
            }
            // dep not in PUBLISH_CRATES — that's a separate,
            // non-fatal class (the dep is intentionally unpublished,
            // e.g. dev-only). Don't flag here.
        }
    }

    if violations.is_empty() {
        println!(
            "publish-order-check: {} crates, topological order verified ✓",
            order.len()
        );
        ExitCode::SUCCESS
    } else {
        for v in &violations {
            eprintln!("{v}");
        }
        eprintln!(
            "publish-order-check: {} violation(s) — see {} crates list",
            violations.len(),
            order.len()
        );
        ExitCode::FAILURE
    }
}

fn load_cargo_metadata() -> Result<CargoMetadata, String> {
    // Walk up from CWD until we find a Cargo.toml with [workspace].
    // The standalone tool may be invoked from anywhere.
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .map_err(|e| format!("failed to spawn `cargo metadata`: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo metadata exited {:?}\nstderr: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("failed to parse cargo metadata JSON: {e}"))
}

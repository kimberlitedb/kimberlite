//! Build script for kmb-site
//!
//! - Generates a build version hash for cache busting
//! - In release builds, minifies CSS using lightningcss

use std::process::Command;

fn main() {
    // Rerun if CSS changes
    println!("cargo:rerun-if-changed=../../public/css");
    // Rerun if git HEAD changes (new commits)
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    // Rerun if workspace version changes (fallback path for latest-release)
    println!("cargo:rerun-if-changed=../../../Cargo.toml");
    // Rerun if deploy override changes
    println!("cargo:rerun-if-env-changed=KIMBERLITE_LATEST_RELEASE");

    // Generate build version for cache busting
    generate_build_version();

    // Emit KIMBERLITE_LATEST_RELEASE so templates can render the
    // "Latest Release: vX.Y.Z" string without hard-coding a number
    // that drifts every release. Preference order mirrors the
    // ROADMAP v0.5.0 "Docker mirror + install.sh templating" item:
    //   1. KIMBERLITE_LATEST_RELEASE env var — set by
    //      .github/workflows/deploy-site.yml from the latest GitHub
    //      release tag. This is authoritative in prod.
    //   2. Workspace Cargo.toml version — safe offline fallback that
    //      always matches what this site was built from.
    emit_latest_release_version();

    // CSS minification only in release builds
    #[cfg(not(debug_assertions))]
    {
        minify_css();
    }
}

fn emit_latest_release_version() {
    // 1. Deploy-time override. The deploy workflow reads the latest
    // GitHub release tag and passes it in; this keeps the site showing
    // the real latest release even if the workspace version has
    // already advanced to the next pre-release.
    if let Ok(tag) = std::env::var("KIMBERLITE_LATEST_RELEASE") {
        let tag = tag.trim();
        if !tag.is_empty() && tag != "unknown" {
            println!("cargo:rustc-env=KIMBERLITE_LATEST_RELEASE={tag}");
            return;
        }
    }

    // 2. Fallback: read [workspace.package] version = "X.Y.Z" from the
    // top-level Cargo.toml. Small hand-rolled parser — we only want one
    // line and don't want to pull a TOML dependency into build.rs.
    let cargo_toml = match std::fs::read_to_string("../../../Cargo.toml") {
        Ok(s) => s,
        Err(_) => {
            println!("cargo:rustc-env=KIMBERLITE_LATEST_RELEASE=unknown");
            return;
        }
    };
    let mut in_workspace_package = false;
    for raw_line in cargo_toml.lines() {
        let line = raw_line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            in_workspace_package = line == "[workspace.package]";
            continue;
        }
        if !in_workspace_package {
            continue;
        }
        if let Some(rest) = line.strip_prefix("version") {
            // matches: version = "0.4.2"  (with arbitrary spaces)
            if let Some(q1) = rest.find('"') {
                if let Some(q2) = rest[q1 + 1..].find('"') {
                    let ver = &rest[q1 + 1..q1 + 1 + q2];
                    println!("cargo:rustc-env=KIMBERLITE_LATEST_RELEASE=v{ver}");
                    return;
                }
            }
        }
    }
    println!("cargo:rustc-env=KIMBERLITE_LATEST_RELEASE=unknown");
}

fn generate_build_version() {
    // Priority: 1) BUILD_VERSION env var (from Docker build arg)
    //           2) Git commit hash
    //           3) Build timestamp fallback
    let version = std::env::var("BUILD_VERSION")
        .ok()
        .filter(|v| !v.is_empty() && v != "unknown")
        .or_else(|| {
            Command::new("git")
                .args(["rev-parse", "--short=8", "HEAD"])
                .output()
                .ok()
                .and_then(|output| {
                    if output.status.success() {
                        String::from_utf8(output.stdout).ok()
                    } else {
                        None
                    }
                })
                .map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|| {
            // Fallback to build timestamp
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| format!("{:x}", d.as_secs()))
                .unwrap_or_else(|_| "unknown".to_string())
        });

    println!("cargo:rustc-env=BUILD_VERSION={version}");
}

#[cfg(not(debug_assertions))]
fn minify_css() {
    use lightningcss::stylesheet::{MinifyOptions, ParserOptions, PrinterOptions, StyleSheet};
    use std::fs;
    use std::path::Path;

    let css_dir = Path::new("../../public/css");
    let style_path = css_dir.join("style.css");

    if !style_path.exists() {
        return;
    }

    let css = match fs::read_to_string(&style_path) {
        Ok(content) => content,
        Err(_) => return,
    };

    let stylesheet = match StyleSheet::parse(&css, ParserOptions::default()) {
        Ok(s) => s,
        Err(_) => return,
    };

    let mut stylesheet = stylesheet;
    if stylesheet.minify(MinifyOptions::default()).is_err() {
        return;
    }

    let result = match stylesheet.to_css(PrinterOptions {
        minify: true,
        ..Default::default()
    }) {
        Ok(r) => r,
        Err(_) => return,
    };

    let output_path = css_dir.join("style.min.css");
    let _ = fs::write(output_path, result.code);
}

//! `audit-matrix` — generate the traceability matrix from in-source
//! `AUDIT-YYYY-NN` markers.
//!
//! AUDIT-2026-05 T1 — closes the v0.7.0 ROADMAP item "Auto-generated
//! traceability matrix from in-source AUDIT-YYYY-NN markers".
//!
//! The tool scans every `.rs` file under the workspace root, extracts
//! marker lines of the form `AUDIT-YYYY-NN <Section><Severity>-<NN>`
//! (case-insensitive, allowing surrounding comment glyphs), and emits
//! a Markdown table sorted by (file, line). Two modes:
//!
//!   - `audit-matrix --out path.md` writes / overwrites the matrix.
//!   - `audit-matrix --check` re-generates in memory and exits 1 if
//!     the on-disk matrix is stale relative to current sources.
//!
//! Marker taxonomy (extends the April-2026 `S/H/L/M` set with `T`):
//!
//!     S — Safety (assertion or invariant)
//!     H — Hardening (defensive bound, capacity check)
//!     L — Liveness (a workflow path that must terminate)
//!     M — Methodology (meta-property: coverage, density)
//!     T — Traceability hook (explicit pointer to an audit-listed control)
//!
//! Pure stdlib — no `walkdir` / `ignore` / `regex` dep. The
//! implementation is a bounded recursive walk skipping `target/`,
//! `.git/`, and `node_modules/`.

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    name = "audit-matrix",
    about = "Generate the AUDIT-YYYY-NN traceability matrix from source markers."
)]
struct Args {
    /// Workspace root to scan. Defaults to the current directory.
    #[arg(long, default_value = ".")]
    root: PathBuf,

    /// Output path for the generated matrix. Required unless --check.
    #[arg(long)]
    out: Option<PathBuf>,

    /// Re-generate in memory and exit 1 if the on-disk matrix is
    /// stale. Used as a CI gate; pairs with --out.
    #[arg(long)]
    check: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditMarker {
    /// E.g. "AUDIT-2026-04".
    campaign: String,
    /// E.g. "S3.4".
    code: String,
    /// File path, relative to the scan root.
    file: PathBuf,
    /// 1-based line number where the marker was found.
    line: u32,
    /// One-line context — the comment text following the marker, or
    /// the next non-blank line if the marker is the only content on
    /// the comment line.
    context: String,
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("audit-matrix: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &Args) -> io::Result<()> {
    let markers = scan_workspace(&args.root)?;
    let matrix = render_matrix(&markers);

    if args.check {
        let out = args.out.as_ref().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "--check requires --out (the path to compare against)",
            )
        })?;
        let on_disk = fs::read_to_string(out).unwrap_or_default();
        if on_disk.trim() != matrix.trim() {
            eprintln!("audit-matrix: {} is stale — re-run without --check to regenerate", out.display());
            return Err(io::Error::new(io::ErrorKind::InvalidData, "stale matrix"));
        }
        println!("audit-matrix: {} is up to date ({} markers)", out.display(), markers.len());
        return Ok(());
    }

    if let Some(path) = &args.out {
        fs::write(path, &matrix)?;
        println!("audit-matrix: wrote {} markers to {}", markers.len(), path.display());
    } else {
        io::stdout().write_all(matrix.as_bytes())?;
    }
    Ok(())
}

/// Public for tests + future programmatic consumers (the
/// `kimberlite-compliance::audit_matrix` library entry point).
pub fn scan_workspace(root: &Path) -> io::Result<Vec<AuditMarker>> {
    assert!(
        root.exists(),
        "audit-matrix scan root does not exist: {}",
        root.display()
    );
    let mut out = Vec::new();
    visit_dir(root, root, &mut out)?;
    out.sort_by(|a, b| (a.file.as_path(), a.line).cmp(&(b.file.as_path(), b.line)));
    // Postcondition: sorted, deterministic order.
    debug_assert!(
        out.windows(2)
            .all(|w| (w[0].file.as_path(), w[0].line) <= (w[1].file.as_path(), w[1].line)),
        "audit-matrix scan_workspace must return sorted markers"
    );
    Ok(out)
}

fn visit_dir(root: &Path, dir: &Path, out: &mut Vec<AuditMarker>) -> io::Result<()> {
    // Bounded depth — stop before symlink loops or unreasonable
    // hierarchies. PRESSURECRAFT §5: explicit bounds.
    const MAX_DEPTH: usize = 16;
    fn helper(
        root: &Path,
        dir: &Path,
        depth: usize,
        out: &mut Vec<AuditMarker>,
    ) -> io::Result<()> {
        if depth >= MAX_DEPTH {
            return Ok(());
        }
        let entries = match fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(_) => return Ok(()), // skip unreadable dirs
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name = name.to_string_lossy();
            // Skip noise.
            if matches!(
                name.as_ref(),
                "target"
                    | "node_modules"
                    | ".git"
                    | ".artifacts"
                    | "dist"
                    | "build"
                    | "vendor"
            ) {
                continue;
            }
            let ft = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            if ft.is_dir() {
                helper(root, &path, depth + 1, out)?;
            } else if ft.is_file() {
                if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                    scan_file(root, &path, out)?;
                }
            }
        }
        Ok(())
    }
    helper(root, dir, 0, out)
}

fn scan_file(root: &Path, file: &Path, out: &mut Vec<AuditMarker>) -> io::Result<()> {
    let bytes = match fs::read(file) {
        Ok(b) => b,
        Err(_) => return Ok(()),
    };
    let text = match std::str::from_utf8(&bytes) {
        Ok(s) => s,
        Err(_) => return Ok(()), // skip non-UTF-8 files
    };
    let rel = file
        .strip_prefix(root)
        .unwrap_or(file)
        .to_path_buf();
    for (idx, line) in text.lines().enumerate() {
        if let Some(marker) = parse_marker_line(line) {
            out.push(AuditMarker {
                campaign: marker.0,
                code: marker.1,
                file: rel.clone(),
                line: (idx + 1) as u32,
                context: marker.2,
            });
        }
    }
    Ok(())
}

/// Parses an `AUDIT-YYYY-NN <code>` marker out of a single source
/// line. Returns `(campaign, code, context)` on a hit.
///
/// Format accepted (case-insensitive on `AUDIT`): a comment line
/// containing `AUDIT-` followed by a 4-digit year, `-`, two-digit
/// campaign number, whitespace, then a code matching `[SHLMT]\d+(\.\d+)?`.
fn parse_marker_line(line: &str) -> Option<(String, String, String)> {
    let l = line.trim();
    let upper = l.to_ascii_uppercase();
    let pos = upper.find("AUDIT-")?;
    let after = &l[pos..];
    // YYYY-NN: positions 6..10 = year digits, '-', positions 11..13
    let bytes = after.as_bytes();
    if bytes.len() < 14 {
        return None;
    }
    if !bytes[6..10].iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if bytes[10] != b'-' {
        return None;
    }
    if !bytes[11..13].iter().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let campaign = String::from_utf8_lossy(&bytes[..13]).into_owned();
    let rest = &after[13..].trim_start();
    // Code is the next whitespace-delimited token. Drop trailing
    // punctuation (commas, dashes used as separators in prose).
    let code_token = rest.split_whitespace().next()?;
    let code = code_token
        .trim_end_matches(|c: char| c == ',' || c == '.' || c == ';' || c == '—' || c == '-')
        .to_string();
    if code.is_empty() {
        return None;
    }
    // Code must start with one of S/H/L/M/T.
    if !matches!(code.chars().next()?.to_ascii_uppercase(), 'S' | 'H' | 'L' | 'M' | 'T') {
        return None;
    }
    // Context: anything after the code, with comment glyphs trimmed.
    let context = rest
        .trim_start_matches(code_token)
        .trim_start()
        .trim_start_matches('—')
        .trim()
        .trim_end_matches(|c: char| c == '*' || c == '/' || c == ' ')
        .to_string();
    Some((campaign, code, context))
}

fn render_matrix(markers: &[AuditMarker]) -> String {
    let mut by_campaign: BTreeMap<&str, Vec<&AuditMarker>> = BTreeMap::new();
    for m in markers {
        by_campaign.entry(m.campaign.as_str()).or_default().push(m);
    }

    let mut out = String::new();
    out.push_str("# Traceability Matrix\n\n");
    out.push_str(
        "_Generated by `cargo run -p kimberlite-compliance --bin audit-matrix`. \
        Do not edit by hand — re-run the tool. CI gates against drift via \
        `audit-matrix --check`._\n\n",
    );
    out.push_str("Marker taxonomy:\n\n");
    out.push_str("- **S** — Safety (assertion or invariant)\n");
    out.push_str("- **H** — Hardening (defensive bound, capacity check)\n");
    out.push_str("- **L** — Liveness (workflow that must terminate)\n");
    out.push_str("- **M** — Methodology (coverage, density)\n");
    out.push_str("- **T** — Traceability hook\n\n");
    out.push_str(&format!("Total markers: {}\n\n", markers.len()));

    for (campaign, list) in &by_campaign {
        out.push_str(&format!("## {campaign}\n\n"));
        out.push_str(&format!("Markers in this campaign: {}\n\n", list.len()));
        out.push_str("| Code | File | Line | Context |\n");
        out.push_str("|------|------|------|---------|\n");
        for m in list {
            let ctx = m.context.replace('|', "\\|");
            // Truncate context to keep table readable.
            let ctx_trunc = if ctx.len() > 120 {
                format!("{}…", &ctx[..120])
            } else {
                ctx
            };
            out.push_str(&format!(
                "| `{}` | `{}` | {} | {} |\n",
                m.code,
                m.file.display(),
                m.line,
                ctx_trunc
            ));
        }
        out.push('\n');
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn parse_marker_line_canonical_form() {
        let r = parse_marker_line("// AUDIT-2026-04 S3.4 — a simple LRU cache");
        assert!(r.is_some(), "must parse the canonical comment form");
        let (campaign, code, context) = r.unwrap();
        assert_eq!(campaign, "AUDIT-2026-04");
        assert_eq!(code, "S3.4");
        assert!(context.contains("simple LRU cache"));
    }

    #[test]
    fn parse_marker_line_doc_comment_form() {
        let r = parse_marker_line("/// AUDIT-2026-05 S3.6 — symmetric with TableMetadataWrite");
        assert!(r.is_some());
        assert_eq!(r.unwrap().0, "AUDIT-2026-05");
    }

    #[test]
    fn parse_marker_line_traceability_hook_t() {
        let r = parse_marker_line("// AUDIT-2026-05 T1 — audit-matrix tool");
        assert!(r.is_some());
        assert_eq!(r.unwrap().1, "T1");
    }

    #[test]
    fn parse_marker_line_rejects_non_marker() {
        assert!(parse_marker_line("// some random comment").is_none());
        assert!(parse_marker_line("let x = 5; // AUDIT-bad-form").is_none());
        // Year not all digits.
        assert!(parse_marker_line("// AUDIT-202X-04 S1.0").is_none());
    }

    #[test]
    fn parse_marker_line_rejects_unknown_severity_letter() {
        assert!(parse_marker_line("// AUDIT-2026-04 X1 — bogus").is_none());
    }

    #[test]
    fn scan_workspace_finds_markers_in_temp_tree() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src = dir.path().join("foo.rs");
        let mut f = std::fs::File::create(&src).unwrap();
        writeln!(f, "// AUDIT-2026-05 S1 — first marker").unwrap();
        writeln!(f, "fn main() {{}}").unwrap();
        writeln!(f, "// AUDIT-2026-05 H2 — second marker").unwrap();
        drop(f);

        let markers = scan_workspace(dir.path()).expect("scan");
        assert_eq!(markers.len(), 2);
        assert_eq!(markers[0].code, "S1");
        assert_eq!(markers[1].code, "H2");
        // Postcondition: file paths are rooted relative to the scan
        // root (Path::strip_prefix); line numbers are 1-based.
        assert_eq!(markers[0].line, 1);
        assert_eq!(markers[1].line, 3);
    }

    #[test]
    fn scan_workspace_skips_target_and_dotgit() {
        let dir = tempfile::tempdir().expect("tempdir");
        for sub in &["target", "node_modules", ".git", ".artifacts"] {
            let d = dir.path().join(sub);
            std::fs::create_dir_all(&d).unwrap();
            let mut f = std::fs::File::create(d.join("ignored.rs")).unwrap();
            writeln!(f, "// AUDIT-2026-05 S99 — should be skipped").unwrap();
        }
        let real = dir.path().join("real.rs");
        let mut f = std::fs::File::create(&real).unwrap();
        writeln!(f, "// AUDIT-2026-05 S1 — real marker").unwrap();
        drop(f);
        let markers = scan_workspace(dir.path()).expect("scan");
        // Only the real marker should appear.
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].code, "S1");
    }

    #[test]
    fn render_matrix_groups_by_campaign() {
        let markers = vec![
            AuditMarker {
                campaign: "AUDIT-2026-04".into(),
                code: "S3.4".into(),
                file: PathBuf::from("a.rs"),
                line: 1,
                context: "first".into(),
            },
            AuditMarker {
                campaign: "AUDIT-2026-05".into(),
                code: "T1".into(),
                file: PathBuf::from("b.rs"),
                line: 2,
                context: "second".into(),
            },
        ];
        let rendered = render_matrix(&markers);
        assert!(rendered.contains("## AUDIT-2026-04"));
        assert!(rendered.contains("## AUDIT-2026-05"));
        assert!(rendered.contains("`S3.4`"));
        assert!(rendered.contains("`T1`"));
    }
}

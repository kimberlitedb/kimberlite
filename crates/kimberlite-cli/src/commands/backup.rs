//! Backup and restore commands.
//!
//! Provides offline full backup with BLAKE3 checksum verification.
//! Incremental and online backup is planned for v0.8.0.

use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::time::SystemTime;

use anyhow::{Context, Result, bail};

use crate::style::{create_spinner, finish_success, print_labeled, print_success};

/// Creates a full backup of a Kimberlite data directory.
///
/// Copies all files from the data directory to a timestamped backup directory,
/// computing BLAKE3 checksums for each file and writing a manifest.
pub fn create(data_dir: &str, backup_dir: &str) -> Result<()> {
    let data_path = Path::new(data_dir);
    let backup_root = Path::new(backup_dir);

    if !data_path.exists() {
        bail!("Data directory '{data_dir}' does not exist");
    }

    // Create timestamped backup directory
    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let backup_name = format!("backup-{timestamp}");
    let backup_path = backup_root.join(&backup_name);

    let sp = create_spinner("Creating backup...");

    fs::create_dir_all(&backup_path)
        .with_context(|| format!("Failed to create backup directory: {}", backup_path.display()))?;

    // Collect all files in data directory
    let mut manifest_entries = Vec::new();
    copy_directory_recursive(data_path, &backup_path, data_path, &mut manifest_entries)?;

    // Write manifest
    let manifest_path = backup_path.join("MANIFEST");
    let mut manifest_file = fs::File::create(&manifest_path)
        .context("Failed to create MANIFEST file")?;

    writeln!(manifest_file, "# Kimberlite Backup Manifest")?;
    writeln!(manifest_file, "# Created: {timestamp}")?;
    writeln!(manifest_file, "# Source: {data_dir}")?;
    writeln!(manifest_file, "# Files: {}", manifest_entries.len())?;
    writeln!(manifest_file)?;

    for (relative_path, hash) in &manifest_entries {
        writeln!(manifest_file, "{hash}  {relative_path}")?;
    }

    finish_success(&sp, &format!("Backup created: {}", backup_path.display()));
    print_labeled("Files", &manifest_entries.len().to_string());
    print_labeled("Location", &backup_path.display().to_string());

    Ok(())
}

/// Restores a backup to a target data directory.
///
/// Verifies checksums before restoring to ensure backup integrity.
pub fn restore(backup_dir: &str, target_dir: &str, force: bool) -> Result<()> {
    let backup_path = Path::new(backup_dir);
    let target_path = Path::new(target_dir);

    if !backup_path.exists() {
        bail!("Backup directory '{backup_dir}' does not exist");
    }

    let manifest_path = backup_path.join("MANIFEST");
    if !manifest_path.exists() {
        bail!("No MANIFEST found in backup directory — not a valid Kimberlite backup");
    }

    // Verify backup integrity first
    let sp = create_spinner("Verifying backup integrity...");
    let entries = parse_manifest(&manifest_path)?;
    verify_entries(backup_path, &entries)?;
    finish_success(&sp, "Backup integrity verified");

    // Check target directory
    if target_path.exists() && target_path.read_dir()?.next().is_some() && !force {
        bail!("Target directory '{target_dir}' is not empty. Use --force to overwrite.");
    }

    // Restore files
    let sp = create_spinner("Restoring backup...");
    fs::create_dir_all(target_path)
        .with_context(|| format!("Failed to create target directory: {}", target_path.display()))?;

    for (relative_path, _hash) in &entries {
        let src = backup_path.join(relative_path);
        let dst = target_path.join(relative_path);

        if let Some(parent) = dst.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::copy(&src, &dst)
            .with_context(|| format!("Failed to copy: {} -> {}", src.display(), dst.display()))?;
    }

    finish_success(&sp, "Backup restored");
    print_labeled("Files restored", &entries.len().to_string());
    print_labeled("Target", &target_path.display().to_string());

    Ok(())
}

/// Lists available backups in a backup directory.
pub fn list(backup_dir: &str) -> Result<()> {
    let backup_root = Path::new(backup_dir);

    if !backup_root.exists() {
        bail!("Backup directory '{backup_dir}' does not exist");
    }

    let mut backups: Vec<(String, usize, u64)> = Vec::new();

    for entry in fs::read_dir(backup_root)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() && path.join("MANIFEST").exists() {
            let name = entry.file_name().to_string_lossy().to_string();
            let manifest = parse_manifest(&path.join("MANIFEST")).unwrap_or_default();
            let file_count = manifest.len();

            // Extract timestamp from directory name
            let timestamp = name
                .strip_prefix("backup-")
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(0);

            backups.push((name, file_count, timestamp));
        }
    }

    backups.sort_by(|a, b| b.2.cmp(&a.2)); // Sort newest first

    if backups.is_empty() {
        println!("No backups found in '{backup_dir}'");
        return Ok(());
    }

    println!("{} backup(s) found:\n", backups.len());
    for (name, file_count, _ts) in &backups {
        println!("  {name}  ({file_count} files)");
    }

    Ok(())
}

/// Verifies the integrity of a backup by checking BLAKE3 checksums.
pub fn verify(backup_dir: &str) -> Result<()> {
    let backup_path = Path::new(backup_dir);

    if !backup_path.exists() {
        bail!("Backup directory '{backup_dir}' does not exist");
    }

    let manifest_path = backup_path.join("MANIFEST");
    if !manifest_path.exists() {
        bail!("No MANIFEST found — not a valid Kimberlite backup");
    }

    let sp = create_spinner("Verifying backup checksums...");
    let entries = parse_manifest(&manifest_path)?;
    verify_entries(backup_path, &entries)?;

    finish_success(&sp, &format!("All {} files verified", entries.len()));
    print_success("Backup integrity OK");

    Ok(())
}

// ============================================================================
// Internal helpers
// ============================================================================

/// Recursively copies a directory, computing BLAKE3 hashes for each file.
fn copy_directory_recursive(
    src_dir: &Path,
    dst_dir: &Path,
    base_dir: &Path,
    manifest: &mut Vec<(String, String)>,
) -> Result<()> {
    for entry in fs::read_dir(src_dir)? {
        let entry = entry?;
        let src_path = entry.path();
        let relative = src_path
            .strip_prefix(base_dir)
            .unwrap_or(&src_path)
            .to_string_lossy()
            .to_string();

        let dst_path = dst_dir.join(&relative);

        if src_path.is_dir() {
            fs::create_dir_all(&dst_path)?;
            copy_directory_recursive(&src_path, dst_dir, base_dir, manifest)?;
        } else {
            if let Some(parent) = dst_path.parent() {
                fs::create_dir_all(parent)?;
            }

            // Read file, compute BLAKE3 hash, and copy
            let mut contents = Vec::new();
            fs::File::open(&src_path)
                .with_context(|| format!("Failed to open: {}", src_path.display()))?
                .read_to_end(&mut contents)?;

            let hash = blake3::hash(&contents);
            let hash_hex = hash.to_hex().to_string();

            fs::File::create(&dst_path)
                .with_context(|| format!("Failed to create: {}", dst_path.display()))?
                .write_all(&contents)?;

            manifest.push((relative, hash_hex));
        }
    }

    Ok(())
}

/// Parses a MANIFEST file into (`relative_path`, `blake3_hash`) pairs.
fn parse_manifest(manifest_path: &Path) -> Result<Vec<(String, String)>> {
    let content = fs::read_to_string(manifest_path)
        .context("Failed to read MANIFEST")?;

    let mut entries = Vec::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Format: "hash  relative_path"
        if let Some((hash, path)) = line.split_once("  ") {
            entries.push((path.to_string(), hash.to_string()));
        }
    }

    Ok(entries)
}

/// Verifies that all files in a backup match their BLAKE3 checksums.
fn verify_entries(backup_dir: &Path, entries: &[(String, String)]) -> Result<()> {
    for (relative_path, expected_hash) in entries {
        let file_path = backup_dir.join(relative_path);

        if !file_path.exists() {
            bail!("Missing file: {relative_path}");
        }

        let mut contents = Vec::new();
        fs::File::open(&file_path)?.read_to_end(&mut contents)?;

        let actual_hash = blake3::hash(&contents).to_hex().to_string();

        if actual_hash != *expected_hash {
            bail!(
                "Checksum mismatch for {relative_path}: expected {expected_hash}, got {actual_hash}"
            );
        }
    }

    Ok(())
}

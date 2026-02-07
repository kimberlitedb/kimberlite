//! Initialize command - creates a new Kimberlite project.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use kimberlite_config::{KimberliteConfig, Paths};

use crate::style::{
    colors::SemanticStyle, create_spinner, finish_success, print_code_example, print_hint,
    print_labeled, print_spacer, print_success,
};

use super::templates::Template;

pub fn run(path: &str, _development: bool, template: Option<&str>) -> Result<()> {
    let project_dir = Path::new(path);

    // Parse template if provided
    let template = match template {
        Some(name) => {
            let t: Template = name
                .parse()
                .map_err(|e: String| anyhow::anyhow!("{e}"))?;
            Some(t)
        }
        None => None,
    };

    // Check if already initialized
    if Paths::is_initialized(project_dir) {
        anyhow::bail!(
            "Project already initialized in {}. kimberlite.toml already exists.",
            project_dir.display()
        );
    }

    // Print header
    print_spacer();
    if let Some(ref t) = template {
        println!("Initializing new Kimberlite project (template: {t})...");
    } else {
        println!("Initializing new Kimberlite project...");
    }
    print_spacer();

    // Step 1: Create project directories
    let sp = create_spinner("Creating project structure...");
    fs::create_dir_all(project_dir).context("Failed to create project directory")?;

    // Create migrations/ directory
    let migrations_dir = Paths::migrations_dir(project_dir);
    fs::create_dir_all(&migrations_dir).context("Failed to create migrations directory")?;

    // Create .kimberlite/ state directory and subdirectories
    let state_dir = Paths::state_dir(project_dir);
    fs::create_dir_all(state_dir.join("data"))?;
    fs::create_dir_all(state_dir.join("logs"))?;
    fs::create_dir_all(state_dir.join("tmp"))?;

    finish_success(&sp, "Created project structure");

    // Step 2: Write kimberlite.toml
    let sp = create_spinner("Writing configuration...");
    let config = KimberliteConfig::development();
    let config_path = Paths::project_config_file(project_dir);
    let config_content =
        toml::to_string_pretty(&config).context("Failed to serialize configuration")?;
    fs::write(&config_path, config_content).context("Failed to write kimberlite.toml")?;
    finish_success(&sp, "Wrote kimberlite.toml");

    // Step 3: Create .gitignore
    let sp = create_spinner("Creating .gitignore...");
    let gitignore_content = r"# Kimberlite local state and data
.kimberlite/data/
.kimberlite/logs/
.kimberlite/tmp/
.kimberlite/cluster/

# Local config overrides (not tracked in git)
kimberlite.local.toml

# Build artifacts
target/
*.db
*.db-shm
*.db-wal
";
    let gitignore_path = project_dir.join(".gitignore");
    if gitignore_path.exists() {
        sp.finish_with_message("⏭  .gitignore already exists");
    } else {
        fs::write(&gitignore_path, gitignore_content).context("Failed to write .gitignore")?;
        finish_success(&sp, "Created .gitignore");
    }

    // Step 4: Create README.md (template-specific if applicable)
    let sp = create_spinner("Creating README.md...");
    let readme_content = template
        .as_ref()
        .map_or(Template::Default.readme(), |t| t.readme());
    let readme_path = project_dir.join("README.md");
    if readme_path.exists() {
        sp.finish_with_message("⏭  README.md already exists");
    } else {
        fs::write(&readme_path, readme_content).context("Failed to write README.md")?;
        finish_success(&sp, "Created README.md");
    }

    // Step 5: Write template migration if applicable
    if let Some(ref t) = template {
        if let Some((migration_name, sql)) = t.migration_sql() {
            let sp = create_spinner(&format!("Writing {t} migration..."));
            let migration_file = migrations_dir.join(format!("{migration_name}.sql"));
            fs::write(&migration_file, sql)
                .context("Failed to write template migration")?;
            finish_success(&sp, &format!("Created migrations/{migration_name}.sql"));
        }
    }

    // Summary
    print_spacer();
    print_success("Project initialized successfully!");
    print_spacer();

    let canonical_path = project_dir
        .canonicalize()
        .unwrap_or(project_dir.to_path_buf());
    print_labeled("Location", &canonical_path.display().to_string());
    print_labeled("Config", "kimberlite.toml");
    print_labeled("Migrations", "migrations/");
    if let Some(ref t) = template {
        print_labeled("Template", &t.to_string());
    }

    // Next steps
    print_spacer();
    println!("{}", "Next steps:".header());
    print_spacer();

    print_hint("Start the development server:");
    if path == "." {
        print_code_example("kmb dev");
    } else {
        print_code_example(&format!("cd {path} && kmb dev"));
    }
    print_spacer();

    print_hint("Or connect with the REPL:");
    print_code_example("kmb repl --tenant 1");

    Ok(())
}

//! Initialize command - creates a new Kimberlite project.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use kimberlite_config::{KimberliteConfig, Paths};

use crate::style::{
    colors::SemanticStyle, create_spinner, finish_success, print_code_example, print_hint,
    print_spacer, print_success,
    wizard,
};

use super::templates::Template;

/// Entry point: routes to interactive or non-interactive based on flags and TTY.
pub fn run(path: Option<&str>, yes: bool, template: Option<&str>) -> Result<()> {
    // Parse template early so errors surface before any prompts
    let parsed_template = match template {
        Some(name) => {
            let t: Template = name.parse().map_err(|e: String| anyhow::anyhow!("{e}"))?;
            Some(t)
        }
        None => None,
    };

    if yes || !wizard::is_interactive() {
        run_non_interactive(path, parsed_template)
    } else {
        run_interactive(path, parsed_template)
    }
}

/// Interactive wizard flow (Svelte/Astro-inspired).
fn run_interactive(
    cli_path: Option<&str>,
    cli_template: Option<Template>,
) -> Result<()> {
    let version = env!("CARGO_PKG_VERSION");
    wizard::print_wizard_welcome(version);

    // Step 1: Resolve project path
    let path_str = if let Some(p) = cli_path {
        p.to_string()
    } else {
        wizard::print_step("Where should we create your project?");

        let input: String = match dialoguer::Input::new()
            .with_prompt("  │")
            .default("./my-app".to_string())
            .interact_text()
        {
            Ok(v) => v,
            Err(dialoguer::Error::IO(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                wizard::print_wizard_canceled();
                return Ok(());
            }
            Err(_) => {
                wizard::print_wizard_canceled();
                return Ok(());
            }
        };
        wizard::print_bar();
        input
    };

    let project_dir = Path::new(&path_str);

    // Check if already initialized
    if Paths::is_initialized(project_dir) {
        anyhow::bail!(
            "Project already initialized in {}. kimberlite.toml already exists.",
            project_dir.display()
        );
    }

    // Non-empty directory: ask instead of hard error
    if project_dir.exists() {
        let has_files = fs::read_dir(project_dir)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false);
        if has_files {
            let proceed = dialoguer::Confirm::new()
                .with_prompt("Directory is not empty. Initialize here anyway?")
                .default(false)
                .interact()
                .unwrap_or(false);
            if !proceed {
                wizard::print_wizard_canceled();
                return Ok(());
            }
            wizard::print_bar();
        }
    }

    // Step 2: Resolve template
    let template = if let Some(t) = cli_template {
        t
    } else {
        wizard::print_step("Which template would you like?");

        let templates = Template::all();
        let items: Vec<String> = templates
            .iter()
            .map(|t| format!("{:<15} {}", t.to_string().header(), t.description().muted()))
            .collect();

        let selection = match dialoguer::Select::new()
            .with_prompt("  │")
            .items(&items)
            .default(0)
            .interact()
        {
            Ok(idx) => idx,
            Err(_) => {
                wizard::print_wizard_canceled();
                return Ok(());
            }
        };
        wizard::print_bar();
        templates[selection]
    };

    // Step 3: Scaffold
    wizard::print_step("Creating project...");
    scaffold_project(project_dir, Some(template))?;

    // Step 4: Summary
    let canonical_path = project_dir
        .canonicalize()
        .unwrap_or_else(|_| project_dir.to_path_buf());

    wizard::print_wizard_summary(
        &canonical_path.display().to_string(),
        &template.to_string(),
        "kimberlite.toml",
    );

    if path_str == "." {
        wizard::print_wizard_next("kmb dev");
    } else {
        wizard::print_wizard_next(&format!("cd {path_str} && kmb dev"));
    }
    println!();

    Ok(())
}

/// Non-interactive mode: preserves original behavior for --yes and CI.
fn run_non_interactive(path: Option<&str>, template: Option<Template>) -> Result<()> {
    let explicit_path = path.is_some();
    let path = path.unwrap_or(".");
    let project_dir = Path::new(path);

    // Check if already initialized
    if Paths::is_initialized(project_dir) {
        anyhow::bail!(
            "Project already initialized in {}. kimberlite.toml already exists.",
            project_dir.display()
        );
    }

    // When no path was given, refuse if the current directory is not empty.
    if !explicit_path && project_dir.exists() {
        let has_files = fs::read_dir(project_dir)
            .map(|mut entries| entries.next().is_some())
            .unwrap_or(false);
        if has_files {
            anyhow::bail!(
                "Refusing to initialize in the current directory \u{2014} it is not empty.\n\n\
                 Use: kimberlite init <project-name>\n\n\
                 This creates a new directory with your project files.\n\
                 To explicitly initialize in the current directory: kimberlite init ."
            );
        }
    }

    // Print header
    print_spacer();
    if let Some(ref t) = template {
        println!("Initializing new Kimberlite project (template: {t})...");
    } else {
        println!("Initializing new Kimberlite project...");
    }
    print_spacer();

    scaffold_project(project_dir, template)?;

    // Summary
    print_spacer();
    print_success("Project initialized successfully!");
    print_spacer();

    let canonical_path = project_dir
        .canonicalize()
        .unwrap_or_else(|_| project_dir.to_path_buf());
    crate::style::print_labeled("Location", &canonical_path.display().to_string());
    crate::style::print_labeled("Config", "kimberlite.toml");
    crate::style::print_labeled("Migrations", "migrations/");
    if let Some(ref t) = template {
        crate::style::print_labeled("Template", &t.to_string());
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

/// Shared scaffolding logic used by both interactive and non-interactive paths.
fn scaffold_project(project_dir: &Path, template: Option<Template>) -> Result<()> {
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
        sp.finish_with_message("\u{23ed}  .gitignore already exists");
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
        sp.finish_with_message("\u{23ed}  README.md already exists");
    } else {
        fs::write(&readme_path, readme_content).context("Failed to write README.md")?;
        finish_success(&sp, "Created README.md");
    }

    // Step 5: Write template migration if applicable
    if let Some(ref t) = template {
        if let Some((migration_name, sql)) = t.migration_sql() {
            let sp = create_spinner(&format!("Writing {t} migration..."));
            let migration_file = migrations_dir.join(format!("{migration_name}.sql"));
            fs::write(&migration_file, sql).context("Failed to write template migration")?;
            finish_success(&sp, &format!("Created migrations/{migration_name}.sql"));
        }
    }

    Ok(())
}

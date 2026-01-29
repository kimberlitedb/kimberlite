//! Interactive SQL REPL.

use std::io::{self, BufRead, Write};

use anyhow::{Context, Result};
use kmb_client::{Client, ClientConfig, QueryParam};
use kmb_types::TenantId;

use super::query::{format_value, print_query_result};

/// REPL prompt.
const PROMPT: &str = "kimberlite> ";

/// Help text for the REPL.
const HELP_TEXT: &str = r"
Kimberlite SQL REPL

Commands:
  .help          Show this help message
  .tables        List all tables (when supported)
  .exit          Exit the REPL
  .quit          Exit the REPL

SQL Examples:
  CREATE TABLE patients (id BIGINT, name TEXT);
  INSERT INTO patients VALUES (1, 'Jane Doe');
  SELECT * FROM patients;
  SELECT * FROM patients WHERE id = 1;

Tips:
  - End SQL statements with a semicolon
  - Press Ctrl+C to cancel a query
  - Press Ctrl+D to exit
";

pub fn run(address: &str, tenant: u64) -> Result<()> {
    let config = ClientConfig::default();
    let tenant_id = TenantId::new(tenant);

    let mut client = Client::connect(address, tenant_id, config)
        .with_context(|| format!("Failed to connect to {address}"))?;

    println!("Kimberlite SQL REPL");
    println!("Connected to: {address}");
    println!("Tenant ID:    {tenant}");
    println!();
    println!("Type .help for help, .exit to quit.");
    println!();

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    let mut input_buffer = String::new();

    loop {
        // Print prompt
        let prompt = if input_buffer.is_empty() {
            PROMPT
        } else {
            "       ...> "
        };
        print!("{prompt}");
        stdout.flush()?;

        // Read line
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => {
                // EOF (Ctrl+D)
                println!();
                println!("Goodbye!");
                break;
            }
            Ok(_) => {}
            Err(e) => {
                eprintln!("Error reading input: {e}");
                continue;
            }
        }

        let trimmed = line.trim();

        // Handle empty input
        if trimmed.is_empty() {
            continue;
        }

        // Handle meta-commands
        if trimmed.starts_with('.') && input_buffer.is_empty() {
            match handle_meta_command(trimmed, &mut client) {
                MetaResult::Continue => continue,
                MetaResult::Exit => break,
            }
        }

        // Accumulate input
        input_buffer.push_str(&line);

        // Check if statement is complete (ends with semicolon)
        let trimmed_buffer = input_buffer.trim();
        if !trimmed_buffer.ends_with(';') {
            continue;
        }

        // Execute the query
        let sql = trimmed_buffer.trim_end_matches(';').trim();
        if !sql.is_empty() {
            execute_query(&mut client, sql);
        }

        input_buffer.clear();
    }

    Ok(())
}

enum MetaResult {
    Continue,
    Exit,
}

fn handle_meta_command(cmd: &str, client: &mut Client) -> MetaResult {
    let cmd_lower = cmd.to_lowercase();
    let parts: Vec<&str> = cmd_lower.split_whitespace().collect();

    match parts.first().copied() {
        Some(".help" | ".h") => {
            println!("{HELP_TEXT}");
            MetaResult::Continue
        }
        Some(".exit" | ".quit" | ".q") => {
            println!("Goodbye!");
            MetaResult::Exit
        }
        Some(".tables") => {
            // Try to query for tables
            if let Ok(result) = client.query("SELECT name FROM _tables", &[]) {
                if result.rows.is_empty() {
                    println!("No tables found.");
                } else {
                    println!("Tables:");
                    for row in &result.rows {
                        if let Some(value) = row.first() {
                            println!("  {}", format_value(value));
                        }
                    }
                }
            } else {
                println!("Table listing not yet supported.");
                println!("Use: SELECT * FROM _tables (when available)");
            }
            MetaResult::Continue
        }
        Some(other) => {
            println!("Unknown command: {other}");
            println!("Type .help for available commands.");
            MetaResult::Continue
        }
        None => MetaResult::Continue,
    }
}

fn execute_query(client: &mut Client, sql: &str) {
    let params: Vec<QueryParam> = vec![];

    match client.query(sql, &params) {
        Ok(result) => {
            print_query_result(&result);
        }
        Err(e) => {
            eprintln!("Error: {e}");
        }
    }
}

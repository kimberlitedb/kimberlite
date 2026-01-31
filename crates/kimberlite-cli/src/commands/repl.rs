//! Interactive SQL REPL.

use std::io::{self, BufRead, Write};

use super::query::format_value;
use crate::style::{
    banner::print_mini_banner, colors::SemanticStyle, create_spinner, finish_and_clear,
    finish_error, finish_success, no_color, print_error, print_query_table, print_spacer,
};
use anyhow::{Context, Result};
use kmb_client::{Client, ClientConfig, QueryParam};
use kmb_types::TenantId;

/// Help text for the REPL.
const HELP_TEXT: &str = r"
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

    // Connect with spinner
    let sp = create_spinner(&format!("Connecting to {address}..."));
    let mut client = match Client::connect(address, tenant_id, config) {
        Ok(c) => {
            finish_success(&sp, &format!("Connected to {address}"));
            c
        }
        Err(e) => {
            finish_error(&sp, "Connection failed");
            return Err(e).with_context(|| format!("Failed to connect to {address}"));
        }
    };

    print_spacer();
    print_mini_banner();
    println!(" {}", "SQL REPL".muted());
    print_spacer();

    println!("  {}: {}", "Server".muted(), address);
    println!("  {}: {}", "Tenant".muted(), tenant);
    print_spacer();

    println!("{}", "Type .help for help, .exit to quit.".muted());
    print_spacer();

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    let mut input_buffer = String::new();

    loop {
        // Print colored prompt
        let prompt = if input_buffer.is_empty() {
            if no_color() {
                "kimberlite> ".to_string()
            } else {
                "kimberlite> ".info()
            }
        } else if no_color() {
            "       ...> ".to_string()
        } else {
            "       ...> ".warning()
        };
        print!("{prompt}");
        stdout.flush()?;

        // Read line
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => {
                // EOF (Ctrl+D)
                print_spacer();
                println!("{}", "Goodbye!".muted());
                break;
            }
            Ok(_) => {}
            Err(e) => {
                print_error(&format!("Error reading input: {e}"));
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
            println!("{}", "Kimberlite SQL REPL".header());
            println!("{HELP_TEXT}");
            MetaResult::Continue
        }
        Some(".exit" | ".quit" | ".q") => {
            println!("{}", "Goodbye!".muted());
            MetaResult::Exit
        }
        Some(".tables") => {
            let sp = create_spinner("Listing tables...");
            if let Ok(result) = client.query("SELECT name FROM _tables", &[]) {
                finish_and_clear(&sp);
                if result.rows.is_empty() {
                    println!("{}", "No tables found.".muted());
                } else {
                    println!("{}:", "Tables".header());
                    for row in &result.rows {
                        if let Some(value) = row.first() {
                            println!("  {}", format_value(value).code());
                        }
                    }
                }
            } else {
                finish_and_clear(&sp);
                println!("{}", "Table listing not yet supported.".muted());
                println!("{}", "Use: SELECT * FROM _tables (when available)".muted());
            }
            MetaResult::Continue
        }
        Some(other) => {
            print_error(&format!("Unknown command: {other}"));
            println!("{}", "Type .help for available commands.".muted());
            MetaResult::Continue
        }
        None => MetaResult::Continue,
    }
}

fn execute_query(client: &mut Client, sql: &str) {
    let params: Vec<QueryParam> = vec![];

    let sp = create_spinner("Executing query...");
    match client.query(sql, &params) {
        Ok(result) => {
            finish_and_clear(&sp);
            // Convert to strings for display
            let columns = result.columns.clone();
            let rows: Vec<Vec<String>> = result
                .rows
                .iter()
                .map(|row| row.iter().map(format_value).collect())
                .collect();
            print_query_table(&columns, &rows);
        }
        Err(e) => {
            finish_error(&sp, "Query failed");
            print_error(&e.to_string());
        }
    }
}

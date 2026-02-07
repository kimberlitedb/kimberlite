//! Interactive SQL REPL with syntax highlighting and tab completion.

use std::borrow::Cow;

use super::query::format_value;
use crate::style::{
    banner::print_mini_banner, colors::SemanticStyle, create_spinner, finish_and_clear,
    finish_error, finish_success, no_color, print_error, print_query_table, print_spacer,
};
use anyhow::{Context, Result};
use kimberlite_client::{Client, ClientConfig, QueryParam};
use kimberlite_types::TenantId;
use rustyline::completion::{Completer, Pair};
use rustyline::highlight::{CmdKind, Highlighter};
use rustyline::hint::Hinter;
use rustyline::history::DefaultHistory;
use rustyline::validate::Validator;
use rustyline::{ColorMode, Config, Editor, Helper};

/// Help text for the REPL.
const HELP_TEXT: &str = r"
Commands:
  .help          Show this help message
  .tables        List all tables (when supported)
  .exit          Exit the REPL

SQL Examples:
  CREATE TABLE patients (id BIGINT, name TEXT);
  INSERT INTO patients VALUES (1, 'Jane Doe');
  SELECT * FROM patients;
  SELECT * FROM patients WHERE id = 1;

Tips:
  - End SQL statements with a semicolon
  - Use Tab for keyword and table name completion
  - Use Up/Down arrows to browse history
  - Press Ctrl+R to search history
  - Press Ctrl+D to exit
";

/// SQL keywords for tab completion.
const SQL_KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "INSERT", "INTO", "VALUES", "UPDATE", "SET", "DELETE",
    "CREATE", "TABLE", "DROP", "ALTER", "ADD", "COLUMN", "INDEX", "PRIMARY", "KEY",
    "NOT", "NULL", "AND", "OR", "IN", "LIKE", "BETWEEN", "IS", "AS", "ON",
    "JOIN", "INNER", "LEFT", "RIGHT", "OUTER", "CROSS", "UNION", "ALL",
    "ORDER", "BY", "ASC", "DESC", "LIMIT", "OFFSET", "GROUP", "HAVING",
    "COUNT", "SUM", "AVG", "MIN", "MAX", "DISTINCT",
    "WITH", "CASE", "WHEN", "THEN", "ELSE", "END",
    "BIGINT", "TEXT", "BOOLEAN", "TIMESTAMP", "DECIMAL", "BYTES",
    "TRUE", "FALSE",
];

/// Rustyline helper with SQL completion and highlighting.
struct SqlHelper {
    /// Known table names (populated from server).
    table_names: Vec<String>,
}

impl SqlHelper {
    fn new() -> Self {
        Self {
            table_names: Vec::new(),
        }
    }

    fn set_tables(&mut self, tables: Vec<String>) {
        self.table_names = tables;
    }
}

impl Completer for SqlHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &rustyline::Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        // Find the start of the current word
        let start = line[..pos]
            .rfind(|c: char| c.is_whitespace() || c == '(' || c == ',')
            .map_or(0, |i| i + 1);

        let prefix = &line[start..pos];
        if prefix.is_empty() {
            return Ok((start, vec![]));
        }

        let prefix_upper = prefix.to_uppercase();
        let mut candidates: Vec<Pair> = Vec::new();

        // Complete SQL keywords
        for &kw in SQL_KEYWORDS {
            if kw.starts_with(&prefix_upper) {
                // Match case: if user typed lowercase, suggest lowercase
                let display = if prefix.chars().next().is_some_and(|c| c.is_lowercase()) {
                    kw.to_lowercase()
                } else {
                    kw.to_string()
                };
                candidates.push(Pair {
                    display: display.clone(),
                    replacement: display,
                });
            }
        }

        // Complete table names
        for name in &self.table_names {
            let name_upper = name.to_uppercase();
            if name_upper.starts_with(&prefix_upper) {
                candidates.push(Pair {
                    display: name.clone(),
                    replacement: name.clone(),
                });
            }
        }

        // Complete meta-commands
        if prefix.starts_with('.') {
            for cmd in &[".help", ".tables", ".exit", ".quit"] {
                if cmd.starts_with(prefix) {
                    candidates.push(Pair {
                        display: (*cmd).to_string(),
                        replacement: (*cmd).to_string(),
                    });
                }
            }
        }

        Ok((start, candidates))
    }
}

impl Highlighter for SqlHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        if no_color() {
            return Cow::Borrowed(line);
        }

        let mut result = String::with_capacity(line.len() * 2);
        let mut chars = line.char_indices().peekable();

        while let Some(&(i, c)) = chars.peek() {
            if c == '\'' {
                // String literal — highlight in green
                result.push_str("\x1b[32m'");
                chars.next();
                while let Some(&(_, ch)) = chars.peek() {
                    chars.next();
                    result.push(ch);
                    if ch == '\'' {
                        break;
                    }
                }
                result.push_str("\x1b[0m");
            } else if c.is_ascii_digit() || (c == '-' && line[i..].len() > 1 && line.as_bytes().get(i + 1).is_some_and(|b| b.is_ascii_digit())) {
                // Number — highlight in yellow
                result.push_str("\x1b[33m");
                while let Some(&(_, ch)) = chars.peek() {
                    if ch.is_ascii_digit() || ch == '.' || ch == '-' {
                        result.push(ch);
                        chars.next();
                    } else {
                        break;
                    }
                }
                result.push_str("\x1b[0m");
            } else if c.is_alphabetic() || c == '_' || c == '.' {
                // Word — check if it's a keyword
                let word_start = i;
                let mut word_end = i;
                while let Some(&(j, ch)) = chars.peek() {
                    if ch.is_alphanumeric() || ch == '_' || ch == '.' {
                        word_end = j + ch.len_utf8();
                        chars.next();
                    } else {
                        break;
                    }
                }
                let word = &line[word_start..word_end];
                let word_upper = word.to_uppercase();

                if SQL_KEYWORDS.contains(&word_upper.as_str()) {
                    // SQL keyword — highlight in blue bold
                    result.push_str("\x1b[1;34m");
                    result.push_str(word);
                    result.push_str("\x1b[0m");
                } else {
                    result.push_str(word);
                }
            } else {
                result.push(c);
                chars.next();
            }
        }

        Cow::Owned(result)
    }

    fn highlight_prompt<'b, 's: 'b, 'p: 'b>(&'s self, prompt: &'p str, _default: bool) -> Cow<'b, str> {
        if no_color() {
            Cow::Borrowed(prompt)
        } else if prompt.contains("...") {
            Cow::Owned(format!("\x1b[33m{prompt}\x1b[0m"))
        } else {
            Cow::Owned(format!("\x1b[36m{prompt}\x1b[0m"))
        }
    }

    fn highlight_char(&self, _line: &str, _pos: usize, _kind: CmdKind) -> bool {
        true
    }
}

impl Hinter for SqlHelper {
    type Hint = String;

    fn hint(&self, _line: &str, _pos: usize, _ctx: &rustyline::Context<'_>) -> Option<String> {
        None
    }
}

impl Validator for SqlHelper {}

impl Helper for SqlHelper {}

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

    println!("{}", "Type .help for help, .exit to quit. Tab for completion.".muted());
    print_spacer();

    // Set up rustyline editor
    let rl_config = Config::builder()
        .color_mode(if no_color() {
            ColorMode::Disabled
        } else {
            ColorMode::Enabled
        })
        .completion_type(rustyline::CompletionType::List)
        .build();

    let mut helper = SqlHelper::new();

    // Try to discover table names for completion
    if let Ok(result) = client.query("SELECT name FROM _tables", &[]) {
        let tables: Vec<String> = result
            .rows
            .iter()
            .filter_map(|row| row.first().map(format_value))
            .collect();
        helper.set_tables(tables);
    }

    let mut rl: Editor<SqlHelper, DefaultHistory> =
        Editor::with_config(rl_config).context("Failed to initialize REPL editor")?;
    rl.set_helper(Some(helper));

    // Load history from ~/.kimberlite/repl_history if it exists
    let history_path = dirs_history_path();
    if let Some(ref path) = history_path {
        let _ = rl.load_history(path);
    }

    let mut input_buffer = String::new();

    loop {
        let prompt = if input_buffer.is_empty() {
            "kimberlite> "
        } else {
            "       ...> "
        };

        match rl.readline(prompt) {
            Ok(line) => {
                let trimmed = line.trim();

                // Handle empty input
                if trimmed.is_empty() {
                    continue;
                }

                // Handle meta-commands
                if trimmed.starts_with('.') && input_buffer.is_empty() {
                    rl.add_history_entry(&line).ok();
                    match handle_meta_command(trimmed, &mut client) {
                        MetaResult::Continue => continue,
                        MetaResult::Exit => break,
                        MetaResult::TablesUpdated(tables) => {
                            if let Some(ref mut helper) = rl.helper_mut() {
                                helper.set_tables(tables);
                            }
                            continue;
                        }
                    }
                }

                // Accumulate input
                if !input_buffer.is_empty() {
                    input_buffer.push(' ');
                }
                input_buffer.push_str(trimmed);

                // Check if statement is complete (ends with semicolon)
                if !input_buffer.trim().ends_with(';') {
                    continue;
                }

                // Execute the query
                let sql = input_buffer.trim().trim_end_matches(';').trim();
                if !sql.is_empty() {
                    rl.add_history_entry(&input_buffer).ok();
                    execute_query(&mut client, sql);
                }

                input_buffer.clear();
            }
            Err(rustyline::error::ReadlineError::Interrupted) => {
                // Ctrl+C — cancel current input
                if !input_buffer.is_empty() {
                    input_buffer.clear();
                    println!("{}", "Query cancelled.".muted());
                }
                continue;
            }
            Err(rustyline::error::ReadlineError::Eof) => {
                // Ctrl+D
                print_spacer();
                println!("{}", "Goodbye!".muted());
                break;
            }
            Err(e) => {
                print_error(&format!("Error reading input: {e}"));
                continue;
            }
        }
    }

    // Save history
    if let Some(ref path) = history_path {
        let _ = rl.save_history(path);
    }

    Ok(())
}

enum MetaResult {
    Continue,
    Exit,
    TablesUpdated(Vec<String>),
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
                    MetaResult::Continue
                } else {
                    println!("{}:", "Tables".header());
                    let mut tables = Vec::new();
                    for row in &result.rows {
                        if let Some(value) = row.first() {
                            let name = format_value(value);
                            println!("  {}", name.code());
                            tables.push(name);
                        }
                    }
                    MetaResult::TablesUpdated(tables)
                }
            } else {
                finish_and_clear(&sp);
                println!("{}", "Table listing not yet supported.".muted());
                println!("{}", "Use: SELECT * FROM _tables (when available)".muted());
                MetaResult::Continue
            }
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

/// Returns the history file path (~/.kimberlite/repl_history).
fn dirs_history_path() -> Option<std::path::PathBuf> {
    let home = std::env::var("HOME").ok()?;
    let dir = std::path::Path::new(&home).join(".kimberlite");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("repl_history"))
}

//! Table formatting using comfy-table.

use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Attribute, Cell, Color, ContentArrangement, Table};

use super::colors::SemanticStyle;

/// Creates a styled table for query results.
pub fn query_result_table(columns: &[String], rows: &[Vec<String>]) -> Table {
    let mut table = Table::new();

    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic);

    // Add header row with bold styling
    let header_cells: Vec<Cell> = columns
        .iter()
        .map(|col| {
            if super::no_color() {
                Cell::new(col)
            } else {
                Cell::new(col)
                    .add_attribute(Attribute::Bold)
                    .fg(Color::Cyan)
            }
        })
        .collect();
    table.set_header(header_cells);

    // Add data rows
    for row in rows {
        table.add_row(row);
    }

    table
}

/// Prints query results as a formatted table.
pub fn print_query_table(columns: &[String], rows: &[Vec<String>]) {
    if columns.is_empty() {
        println!("{}", "Query executed successfully (no results).".muted());
        return;
    }

    let table = query_result_table(columns, rows);
    println!("{table}");

    // Print row count footer
    let count = rows.len();
    let row_word = if count == 1 { "row" } else { "rows" };
    println!("{}", format!("({count} {row_word})").muted());
}

/// Creates a key-value info table (two columns: key and value).
pub fn info_table(entries: &[(&str, &str)]) -> Table {
    let mut table = Table::new();

    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_content_arrangement(ContentArrangement::Dynamic);

    for (key, value) in entries {
        let key_cell = if super::no_color() {
            Cell::new(key)
        } else {
            Cell::new(key).fg(Color::DarkGrey)
        };
        table.add_row(vec![key_cell, Cell::new(value)]);
    }

    table
}

/// Prints a key-value info table.
pub fn print_info_table(entries: &[(&str, &str)]) {
    let table = info_table(entries);
    println!("{table}");
}

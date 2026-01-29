//! Query command - execute a single SQL query.

use anyhow::{Context, Result};
use kmb_client::{Client, ClientConfig, QueryParam};
use kmb_types::{Offset, TenantId};

pub fn run(server: &str, tenant: u64, sql: &str, at: Option<u64>) -> Result<()> {
    let config = ClientConfig::default();
    let tenant_id = TenantId::new(tenant);

    let mut client = Client::connect(server, tenant_id, config)
        .with_context(|| format!("Failed to connect to {server}"))?;

    let params: Vec<QueryParam> = vec![];

    let result = if let Some(position) = at {
        client.query_at(sql, &params, Offset::new(position))?
    } else {
        client.query(sql, &params)?
    };

    print_query_result(&result);

    Ok(())
}

/// Prints query results in a formatted table.
pub fn print_query_result(result: &kmb_wire::QueryResponse) {
    if result.columns.is_empty() {
        println!("Query executed successfully (no results).");
        return;
    }

    // Calculate column widths
    let mut widths: Vec<usize> = result.columns.iter().map(String::len).collect();

    for row in &result.rows {
        for (i, value) in row.iter().enumerate() {
            let len = format_value(value).len();
            if len > widths[i] {
                widths[i] = len;
            }
        }
    }

    // Print header
    let header: Vec<String> = result
        .columns
        .iter()
        .enumerate()
        .map(|(i, col)| format!("{:width$}", col, width = widths[i]))
        .collect();
    println!("{}", header.join(" | "));

    // Print separator
    let sep: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
    println!("{}", sep.join("-+-"));

    // Print rows
    for row in &result.rows {
        let values: Vec<String> = row
            .iter()
            .enumerate()
            .map(|(i, v)| format!("{:width$}", format_value(v), width = widths[i]))
            .collect();
        println!("{}", values.join(" | "));
    }

    // Print row count
    println!();
    let row_word = if result.rows.len() == 1 {
        "row"
    } else {
        "rows"
    };
    println!("({} {})", result.rows.len(), row_word);
}

/// Formats a query value for display.
pub fn format_value(value: &kmb_wire::QueryValue) -> String {
    match value {
        kmb_wire::QueryValue::Null => "NULL".to_string(),
        kmb_wire::QueryValue::BigInt(n) => n.to_string(),
        kmb_wire::QueryValue::Text(s) => s.clone(),
        kmb_wire::QueryValue::Boolean(b) => b.to_string(),
        kmb_wire::QueryValue::Timestamp(t) => t.to_string(),
    }
}

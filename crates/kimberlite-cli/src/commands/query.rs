//! Query command - execute a single SQL query.

use anyhow::{Context, Result};
use kimberlite_client::{Client, ClientConfig, QueryParam};
use kimberlite_types::{Offset, TenantId};

use crate::style::{create_spinner, finish_and_clear, print_query_table};

pub fn run(server: &str, tenant: u64, sql: &str, at: Option<u64>) -> Result<()> {
    let config = ClientConfig::default();
    let tenant_id = TenantId::new(tenant);

    let sp = create_spinner(&format!("Connecting to {server}..."));
    let mut client = Client::connect(server, tenant_id, config)
        .with_context(|| format!("Failed to connect to {server}"))?;
    finish_and_clear(&sp);

    let params: Vec<QueryParam> = vec![];

    let sp = create_spinner("Executing query...");
    let result = if let Some(position) = at {
        client.query_at(sql, &params, Offset::new(position))?
    } else {
        client.query(sql, &params)?
    };
    finish_and_clear(&sp);

    // Convert to strings for display
    let columns = result.columns.clone();
    let rows: Vec<Vec<String>> = result
        .rows
        .iter()
        .map(|row| row.iter().map(format_value).collect())
        .collect();
    print_query_table(&columns, &rows);

    Ok(())
}

/// Formats a query value for display.
pub fn format_value(value: &kimberlite_wire::QueryValue) -> String {
    match value {
        kimberlite_wire::QueryValue::Null => "NULL".to_string(),
        kimberlite_wire::QueryValue::BigInt(n) => n.to_string(),
        kimberlite_wire::QueryValue::Text(s) => s.clone(),
        kimberlite_wire::QueryValue::Boolean(b) => b.to_string(),
        kimberlite_wire::QueryValue::Timestamp(t) => t.to_string(),
    }
}

//! Server-side HTML rendering templates.
//!
//! These functions render HTML fragments that are sent via SSE to update the UI.
//! All user-provided content is escaped to prevent XSS attacks.

use html_escape::encode_text;

/// Renders a query results table.
///
/// # Arguments
/// * `columns` - Column names
/// * `rows` - Row data (each row is a vector of cell values)
pub fn render_query_results(columns: &[String], rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return render_empty_results();
    }

    let mut html = String::from("<div class=\"results-table\">");
    html.push_str("<table>");

    // Header
    html.push_str("<thead><tr>");
    for col in columns {
        html.push_str(&format!("<th>{}</th>", encode_text(col)));
    }
    html.push_str("</tr></thead>");

    // Body
    html.push_str("<tbody>");
    for row in rows {
        html.push_str("<tr>");
        for cell in row {
            // Detect data type for styling
            let data_type = detect_data_type(cell);
            html.push_str(&format!(
                "<td data-type=\"{}\">{}</td>",
                data_type,
                encode_text(cell)
            ));
        }
        html.push_str("</tr>");
    }
    html.push_str("</tbody>");

    html.push_str("</table>");

    // Metadata footer
    html.push_str(&format!(
        "<div class=\"results-table__meta\">\
            <span class=\"results-table__count\">{} rows</span>\
        </div>",
        rows.len()
    ));

    html.push_str("</div>");
    html
}

/// Renders empty results state.
fn render_empty_results() -> String {
    String::from(
        "<div class=\"results-table\">\
            <div class=\"results-table__empty\">No results found</div>\
        </div>",
    )
}

/// Detects data type for styling purposes.
fn detect_data_type(value: &str) -> &'static str {
    if value.is_empty() || value == "null" || value == "NULL" {
        "null"
    } else if value == "true" || value == "false" {
        "boolean"
    } else if value.parse::<f64>().is_ok() {
        "number"
    } else {
        "string"
    }
}

/// Renders a schema tree for a tenant.
///
/// # Arguments
/// * `tenant_id` - Tenant ID
/// * `tenant_name` - Tenant name
/// * `tables` - List of tables with their columns
pub fn render_schema_tree(
    tenant_id: u64,
    tenant_name: &str,
    tables: &[(String, Vec<String>)],
) -> String {
    let mut html = String::from("<div class=\"schema-tree\">");

    // Tenant node
    html.push_str(&format!(
        "<div class=\"schema-tree__item\" data-level=\"0\" data-type=\"tenant\">\
            <span class=\"schema-tree__label\">{} (ID: {})</span>\
        </div>",
        encode_text(tenant_name),
        tenant_id
    ));

    // Table nodes
    for (table_name, columns) in tables {
        html.push_str(&format!(
            "<div class=\"schema-tree__item\" data-level=\"1\" data-type=\"table\">\
                <span class=\"schema-tree__label\">{}</span>\
                <span class=\"schema-tree__type\">table</span>\
            </div>",
            encode_text(table_name)
        ));

        // Column nodes
        for column in columns {
            html.push_str(&format!(
                "<div class=\"schema-tree__item\" data-level=\"2\" data-type=\"column\">\
                    <span class=\"schema-tree__label\">{}</span>\
                </div>",
                encode_text(column)
            ));
        }
    }

    html.push_str("</div>");
    html
}

/// Renders a tenant selector dropdown.
///
/// # Arguments
/// * `tenants` - List of (tenant_id, tenant_name) tuples
/// * `selected_id` - Currently selected tenant ID (if any)
pub fn render_tenant_selector(tenants: &[(u64, String)], selected_id: Option<u64>) -> String {
    let mut html = String::from(
        "<select class=\"tenant-selector__select\" data-model=\"tenant_id\">\
            <option value=\"\">Select tenant...</option>",
    );

    for (id, name) in tenants {
        let selected = if Some(*id) == selected_id {
            " selected"
        } else {
            ""
        };
        html.push_str(&format!(
            "<option value=\"{}\"{}>{} (ID: {})</option>",
            id,
            selected,
            encode_text(name),
            id
        ));
    }

    html.push_str("</select>");
    html
}

/// Renders an error message banner.
///
/// # Arguments
/// * `title` - Error title
/// * `message` - Error details
pub fn render_error(title: &str, message: &str) -> String {
    format!(
        "<div class=\"error-banner\">\
            <div class=\"error-banner__title\">{}</div>\
            <div class=\"error-banner__message\">{}</div>\
        </div>",
        encode_text(title),
        encode_text(message)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_query_results() {
        let columns = vec!["id".to_string(), "name".to_string()];
        let rows = vec![
            vec!["1".to_string(), "Alice".to_string()],
            vec!["2".to_string(), "Bob".to_string()],
        ];

        let html = render_query_results(&columns, &rows);

        assert!(html.contains("<th>id</th>"));
        assert!(html.contains("<th>name</th>"));
        assert!(html.contains("<td data-type=\"number\">1</td>"));
        assert!(html.contains("<td data-type=\"string\">Alice</td>"));
        assert!(html.contains("2 rows"));
    }

    #[test]
    fn test_render_empty_results() {
        let html = render_query_results(&[], &[]);
        assert!(html.contains("No results found"));
    }

    #[test]
    fn test_detect_data_type() {
        assert_eq!(detect_data_type("123"), "number");
        assert_eq!(detect_data_type("true"), "boolean");
        assert_eq!(detect_data_type("null"), "null");
        assert_eq!(detect_data_type("text"), "string");
    }

    #[test]
    fn test_render_schema_tree() {
        let tables = vec![
            (
                "patients".to_string(),
                vec!["id".to_string(), "name".to_string()],
            ),
            (
                "visits".to_string(),
                vec!["id".to_string(), "date".to_string()],
            ),
        ];

        let html = render_schema_tree(1, "dev-tenant", &tables);

        assert!(html.contains("dev-tenant"));
        assert!(html.contains("patients"));
        assert!(html.contains("visits"));
        assert!(html.contains("data-type=\"table\""));
        assert!(html.contains("data-type=\"column\""));
    }

    #[test]
    fn test_render_tenant_selector() {
        let tenants = vec![(1, "tenant-1".to_string()), (2, "tenant-2".to_string())];

        let html = render_tenant_selector(&tenants, Some(1));

        assert!(html.contains("tenant-1"));
        assert!(html.contains("tenant-2"));
        assert!(html.contains("selected"));
    }

    #[test]
    fn test_render_error_escapes_html() {
        let html = render_error("Error", "<script>alert('xss')</script>");
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }

    #[test]
    fn test_xss_prevention_in_results() {
        let columns = vec!["col".to_string()];
        let rows = vec![vec!["<script>alert('xss')</script>".to_string()]];

        let html = render_query_results(&columns, &rows);

        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
    }
}

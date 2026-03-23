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
/// * `tables` - List of tables with their columns (column format: "name (TYPE)")
pub fn render_schema_tree(
    _tenant_id: u64,
    _tenant_name: &str,
    tables: &[(String, Vec<String>)],
) -> String {
    let mut html = String::from("<div class=\"schema-tree\">");

    if tables.is_empty() {
        html.push_str(
            "<div class=\"schema-tree__item schema-tree__empty\" data-level=\"0\" data-type=\"info\">\
                <span class=\"schema-tree__label\">No tables found</span>\
            </div>",
        );
        html.push_str("</div>");
        return html;
    }

    // Table nodes — clickable to browse data
    for (table_name, columns) in tables {
        let escaped_name = encode_text(table_name);
        html.push_str(&format!(
            "<div class=\"schema-tree__item schema-tree__table\" data-level=\"0\" data-type=\"table\" \
                 data-on-click=\"$active_table = '{escaped_name}'; $browse_page = 0; $sort_column = null; $sort_dir = 'ASC'; @post('/studio/browse')\">\
                <span class=\"schema-tree__toggle\">&#9654;</span>\
                <span class=\"schema-tree__label\">{escaped_name}</span>\
                <span class=\"schema-tree__badge\">{}</span>\
            </div>",
            columns.len()
        ));

        // Column nodes (collapsed by default, expanded via CSS toggle)
        for column in columns {
            // Parse "name (TYPE)" format
            let (col_name, col_type) = if let Some(paren_pos) = column.find(" (") {
                let name = &column[..paren_pos];
                let typ = column[paren_pos + 2..].trim_end_matches(')');
                (name, typ)
            } else {
                (column.as_str(), "")
            };

            html.push_str(&format!(
                "<div class=\"schema-tree__item schema-tree__column\" data-level=\"1\" data-type=\"column\" \
                     data-parent=\"{escaped_name}\">\
                    <span class=\"schema-tree__label\">{}</span>\
                    <span class=\"schema-tree__type\">{}</span>\
                </div>",
                encode_text(col_name),
                encode_text(col_type),
            ));
        }
    }

    html.push_str("</div>");
    html
}

/// Renders browse results table with sortable column headers.
pub fn render_browse_results(
    table_name: &str,
    columns: &[String],
    rows: &[Vec<String>],
    sort_column: Option<&str>,
    sort_dir: &str,
) -> String {
    let mut html = String::new();

    // Table header with name
    html.push_str(&format!(
        "<div class=\"data-grid__header\">\
            <h3 class=\"data-grid__title\">{}</h3>\
        </div>",
        encode_text(table_name),
    ));

    if rows.is_empty() {
        html.push_str(
            "<div class=\"data-grid\">\
                <div class=\"data-grid__empty\">No rows in this table</div>\
            </div>",
        );
        return html;
    }

    html.push_str("<div class=\"data-grid\"><table>");

    // Sortable header
    html.push_str("<thead><tr>");
    for col in columns {
        let escaped_col = encode_text(col);
        let is_sorted = sort_column == Some(col.as_str());
        let next_dir = if is_sorted && sort_dir == "ASC" {
            "DESC"
        } else {
            "ASC"
        };
        let arrow = if is_sorted {
            if sort_dir == "ASC" { " &#9650;" } else { " &#9660;" }
        } else {
            ""
        };
        let sorted_class = if is_sorted { " data-grid__th--sorted" } else { "" };

        html.push_str(&format!(
            "<th class=\"data-grid__th{sorted_class}\" \
                data-on-click=\"$sort_column = '{escaped_col}'; $sort_dir = '{next_dir}'; @post('/studio/browse')\">\
                {escaped_col}{arrow}\
            </th>",
        ));
    }
    html.push_str("</tr></thead>");

    // Body
    html.push_str("<tbody>");
    for row in rows {
        html.push_str("<tr>");
        for cell in row {
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

    html.push_str("</table></div>");
    html
}

/// Renders pagination controls.
pub fn render_pagination(current_page: u64, total_rows: u64, page_size: u64) -> String {
    let total_pages = if total_rows == 0 {
        1
    } else {
        total_rows.div_ceil(page_size)
    };

    let mut html = String::from("<div class=\"pagination\">");

    // Previous button
    if current_page > 0 {
        html.push_str(&format!(
            "<button type=\"button\" class=\"button pagination__btn\" data-variant=\"ghost\" \
                data-on-click=\"$browse_page = {}; @post('/studio/browse')\">&#8592; Previous</button>",
            current_page - 1
        ));
    } else {
        html.push_str(
            "<button type=\"button\" class=\"button pagination__btn\" data-variant=\"ghost\" disabled>&#8592; Previous</button>",
        );
    }

    // Page indicator
    html.push_str(&format!(
        "<span class=\"pagination__info\">Page {} of {} ({} rows)</span>",
        current_page + 1,
        total_pages,
        total_rows,
    ));

    // Next button
    if current_page + 1 < total_pages {
        html.push_str(&format!(
            "<button type=\"button\" class=\"button pagination__btn\" data-variant=\"ghost\" \
                data-on-click=\"$browse_page = {}; @post('/studio/browse')\">Next &#8594;</button>",
            current_page + 1
        ));
    } else {
        html.push_str(
            "<button type=\"button\" class=\"button pagination__btn\" data-variant=\"ghost\" disabled>Next &#8594;</button>",
        );
    }

    html.push_str("</div>");
    html
}

/// Renders a skeleton loading table.
pub fn render_skeleton_table() -> String {
    let mut html = String::from("<div class=\"data-grid data-grid--loading\">");
    html.push_str("<table><thead><tr>");
    for _ in 0..4 {
        html.push_str("<th><div class=\"skeleton skeleton--text\"></div></th>");
    }
    html.push_str("</tr></thead><tbody>");
    for _ in 0..5 {
        html.push_str("<tr>");
        for _ in 0..4 {
            html.push_str("<td><div class=\"skeleton skeleton--text\"></div></td>");
        }
        html.push_str("</tr>");
    }
    html.push_str("</tbody></table></div>");
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
                vec!["id (BIGINT)".to_string(), "name (TEXT)".to_string()],
            ),
            (
                "visits".to_string(),
                vec!["id (BIGINT)".to_string(), "date (TEXT)".to_string()],
            ),
        ];

        let html = render_schema_tree(1, "dev-tenant", &tables);

        assert!(html.contains("patients"));
        assert!(html.contains("visits"));
        assert!(html.contains("data-type=\"table\""));
        assert!(html.contains("data-type=\"column\""));
        // Column type badges
        assert!(html.contains("BIGINT"));
        assert!(html.contains("TEXT"));
        // Column count badges
        assert!(html.contains(">2</span>"));
    }

    #[test]
    fn test_render_schema_tree_empty() {
        let html = render_schema_tree(1, "dev-tenant", &[]);
        assert!(html.contains("No tables found"));
    }

    #[test]
    fn test_render_browse_results() {
        let columns = vec!["id".to_string(), "name".to_string()];
        let rows = vec![
            vec!["1".to_string(), "Alice".to_string()],
        ];
        let html = render_browse_results("patients", &columns, &rows, None, "ASC");
        assert!(html.contains("patients"));
        assert!(html.contains("Alice"));
        assert!(html.contains("data-grid__th"));
    }

    #[test]
    fn test_render_pagination() {
        let html = render_pagination(0, 100, 50);
        assert!(html.contains("Page 1 of 2"));
        assert!(html.contains("100 rows"));
        assert!(html.contains("disabled"));  // Previous disabled on first page
    }

    #[test]
    fn test_render_skeleton_table() {
        let html = render_skeleton_table();
        assert!(html.contains("skeleton"));
        assert!(html.contains("data-grid--loading"));
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

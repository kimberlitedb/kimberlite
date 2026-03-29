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

/// Parses a column info string in format "name (TYPE)" or "name (TYPE) [CLASS]".
///
/// Returns `(col_name, col_type, Option<classification>)`.
fn parse_column_info(column: &str) -> (&str, &str, Option<&str>) {
    // Check for classification suffix: "name (TYPE) [CLASS]"
    if let Some(bracket_pos) = column.rfind(" [") {
        let classification = column[bracket_pos + 2..].trim_end_matches(']');
        let before_bracket = &column[..bracket_pos];
        if let Some(paren_pos) = before_bracket.find(" (") {
            let name = &before_bracket[..paren_pos];
            let typ = before_bracket[paren_pos + 2..].trim_end_matches(')');
            return (name, typ, Some(classification));
        }
        return (before_bracket, "", Some(classification));
    }
    // Standard format: "name (TYPE)"
    if let Some(paren_pos) = column.find(" (") {
        let name = &column[..paren_pos];
        let typ = column[paren_pos + 2..].trim_end_matches(')');
        (name, typ, None)
    } else {
        (column, "", None)
    }
}

/// Renders a data classification badge for the schema tree.
///
/// Returns an HTML string for the badge, or empty string if no classification.
fn render_classification_badge(classification: Option<&str>) -> String {
    let Some(class) = classification else {
        return String::new();
    };

    // Map classification to color
    let (color, bg) = match class.to_uppercase().as_str() {
        "PHI" => ("oklch(0.45 0.15 25)", "oklch(0.92 0.05 25)"),
        "PII" => ("oklch(0.45 0.15 50)", "oklch(0.92 0.05 50)"),
        "PCI" => ("oklch(0.45 0.15 80)", "oklch(0.92 0.05 80)"),
        "FINANCIAL" => ("oklch(0.45 0.12 250)", "oklch(0.92 0.04 250)"),
        "CONFIDENTIAL" => ("oklch(0.45 0.12 290)", "oklch(0.92 0.04 290)"),
        "SENSITIVE" => ("oklch(0.45 0.15 25)", "oklch(0.92 0.05 25)"),
        "PUBLIC" => ("oklch(0.45 0.12 145)", "oklch(0.92 0.04 145)"),
        "DEIDENTIFIED" | "DE-IDENTIFIED" => ("oklch(0.45 0.08 200)", "oklch(0.92 0.03 200)"),
        _ => ("oklch(0.5 0.0 0)", "oklch(0.92 0.0 0)"),
    };

    format!(
        "<span class=\"schema-tree__classification\" \
              style=\"font-size: 9px; padding: 1px 4px; border-radius: 3px; \
                     font-weight: 600; text-transform: uppercase; letter-spacing: 0.03em; \
                     color: {color}; background: {bg}; margin-left: 4px;\">{}</span>",
        encode_text(class)
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

        // Column nodes (collapsed by default, expanded via JS toggle)
        for column in columns {
            // Parse "name (TYPE)" or "name (TYPE) [CLASSIFICATION]" format
            let (col_name, col_type, classification) = parse_column_info(column);

            let escaped_col = encode_text(col_name);
            let class_badge = render_classification_badge(classification);
            html.push_str(&format!(
                "<div class=\"schema-tree__item schema-tree__column\" data-level=\"1\" data-type=\"column\" \
                     data-parent=\"{escaped_name}\" style=\"display: none;\">\
                    <span class=\"schema-tree__label\" \
                          onclick=\"event.stopPropagation(); window._insertAtCursor('{escaped_col}')\" \
                          style=\"cursor: pointer;\" title=\"Click to insert\">{escaped_col}</span>\
                    <span class=\"schema-tree__type\">{}</span>{class_badge}\
                </div>",
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

/// Renders audit log results.
pub fn render_audit_log(columns: &[String], rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::from(
            "<div class=\"results-table\">\
                <div class=\"results-table__empty\">No audit events found</div>\
            </div>",
        );
    }

    let mut html = String::from("<div class=\"results-table\">");
    html.push_str("<table>");

    // Header with special styling for audit columns
    html.push_str("<thead><tr>");
    for col in columns {
        html.push_str(&format!("<th>{}</th>", encode_text(col)));
    }
    html.push_str("</tr></thead>");

    // Body
    html.push_str("<tbody>");
    for row in rows {
        html.push_str("<tr>");
        for (i, cell) in row.iter().enumerate() {
            let col_name = columns.get(i).map(|s| s.as_str()).unwrap_or("");
            let data_type = if col_name.eq_ignore_ascii_case("action")
                || col_name.eq_ignore_ascii_case("action_type")
            {
                "audit-action"
            } else {
                detect_data_type(cell)
            };
            html.push_str(&format!(
                "<td data-type=\"{}\">{}</td>",
                data_type,
                encode_text(cell)
            ));
        }
        html.push_str("</tr>");
    }
    html.push_str("</tbody></table>");

    html.push_str(&format!(
        "<div class=\"results-table__meta\"><span class=\"results-table__count\">{} events</span></div>",
        rows.len()
    ));
    html.push_str("</div>");
    html
}

/// Renders the compliance dashboard with status cards.
pub fn render_compliance_dashboard(
    tenant_id: u64,
    table_count: u64,
    total_rows: u64,
    classified_columns: u64,
    total_columns: u64,
) -> String {
    let classification_pct = if total_columns > 0 {
        (classified_columns as f64 / total_columns as f64 * 100.0) as u64
    } else {
        0
    };

    let classification_status = if classification_pct >= 80 {
        ("Compliant", "oklch(0.45 0.12 145)", "oklch(0.92 0.04 145)")
    } else if classification_pct >= 50 {
        ("Warning", "oklch(0.45 0.12 80)", "oklch(0.92 0.04 80)")
    } else {
        ("Action Required", "oklch(0.45 0.12 25)", "oklch(0.92 0.04 25)")
    };

    let mut html = String::new();

    // Overview stats
    html.push_str(&format!(
        "<div style=\"display: grid; grid-template-columns: repeat(auto-fit, minmax(180px, 1fr)); gap: var(--space-m); margin-bottom: var(--space-l);\">\
            <div style=\"padding: var(--space-m); border: 1px solid var(--border-default); border-radius: 8px;\">\
                <div style=\"font-size: 28px; font-weight: 700;\">{tenant_id}</div>\
                <div style=\"font-size: 12px; color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.05em;\">Tenant ID</div>\
            </div>\
            <div style=\"padding: var(--space-m); border: 1px solid var(--border-default); border-radius: 8px;\">\
                <div style=\"font-size: 28px; font-weight: 700;\">{table_count}</div>\
                <div style=\"font-size: 12px; color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.05em;\">Tables</div>\
            </div>\
            <div style=\"padding: var(--space-m); border: 1px solid var(--border-default); border-radius: 8px;\">\
                <div style=\"font-size: 28px; font-weight: 700;\">{total_rows}</div>\
                <div style=\"font-size: 12px; color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.05em;\">Total Rows</div>\
            </div>\
            <div style=\"padding: var(--space-m); border: 1px solid var(--border-default); border-radius: 8px;\">\
                <div style=\"font-size: 28px; font-weight: 700;\">{classified_columns}/{total_columns}</div>\
                <div style=\"font-size: 12px; color: var(--text-secondary); text-transform: uppercase; letter-spacing: 0.05em;\">Classified Columns</div>\
            </div>\
        </div>"
    ));

    // Compliance framework cards
    let frameworks = [
        ("HIPAA", "Health Insurance Portability and Accountability Act", "PHI protection, access controls, audit trails"),
        ("GDPR", "General Data Protection Regulation", "Data subject rights, consent, erasure (Art. 17)"),
        ("SOX", "Sarbanes-Oxley Act", "Financial data integrity, audit trails, access controls"),
        ("PCI DSS", "Payment Card Industry Data Security Standard", "Cardholder data protection, encryption, access control"),
    ];

    html.push_str("<div style=\"display: grid; grid-template-columns: repeat(auto-fit, minmax(280px, 1fr)); gap: var(--space-m);\">");

    for (name, full_name, requirements) in &frameworks {
        html.push_str(&format!(
            "<div style=\"padding: var(--space-m); border: 1px solid var(--border-default); border-radius: 8px;\">\
                <div style=\"display: flex; justify-content: space-between; align-items: center; margin-bottom: var(--space-s);\">\
                    <h3 style=\"font-size: 16px; margin: 0; font-weight: 700;\">{name}</h3>\
                    <span style=\"font-size: 10px; padding: 2px 8px; border-radius: 4px; font-weight: 600; \
                                 text-transform: uppercase; letter-spacing: 0.05em; \
                                 color: {}; background: {};\">{}</span>\
                </div>\
                <div style=\"font-size: 12px; color: var(--text-secondary); margin-bottom: var(--space-xs);\">{full_name}</div>\
                <div style=\"font-size: 12px; color: var(--text-tertiary);\">{requirements}</div>\
                <div style=\"margin-top: var(--space-s);\">\
                    <div style=\"font-size: 11px; color: var(--text-secondary); margin-bottom: 4px;\">Data Classification: {classification_pct}%</div>\
                    <div style=\"height: 4px; background: var(--surface-secondary); border-radius: 2px; overflow: hidden;\">\
                        <div style=\"height: 100%; width: {classification_pct}%; background: {}; border-radius: 2px;\"></div>\
                    </div>\
                </div>\
            </div>",
            classification_status.1,
            classification_status.2,
            classification_status.0,
            classification_status.1,
        ));
    }

    html.push_str("</div>");

    // Compliance features info
    html.push_str(
        "<div style=\"margin-top: var(--space-l); padding: var(--space-m); border: 1px solid var(--border-default); border-radius: 8px; background: var(--surface-secondary, rgba(0,0,0,0.02));\">\
            <h3 style=\"font-size: 14px; margin: 0 0 var(--space-s); font-weight: 600;\">Built-in Compliance Features</h3>\
            <div style=\"display: grid; grid-template-columns: repeat(auto-fit, minmax(200px, 1fr)); gap: var(--space-s); font-size: 13px;\">\
                <div><strong>Immutable Audit Log</strong> &mdash; All operations logged with hash-chain integrity</div>\
                <div><strong>Data Classification</strong> &mdash; Column-level PHI/PII/PCI/Financial tagging</div>\
                <div><strong>Time-Travel Queries</strong> &mdash; Query data at any historical point</div>\
                <div><strong>Consent Management</strong> &mdash; GDPR-compliant consent tracking</div>\
                <div><strong>Data Erasure</strong> &mdash; Right to be forgotten (GDPR Art. 17)</div>\
                <div><strong>Breach Detection</strong> &mdash; 6 automatic breach indicators</div>\
                <div><strong>RBAC/ABAC</strong> &mdash; Role and attribute-based access control</div>\
                <div><strong>Data Masking</strong> &mdash; 5 masking strategies for sensitive data</div>\
            </div>\
        </div>"
    );

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

    #[test]
    fn test_parse_column_info_basic() {
        let (name, typ, class) = parse_column_info("id (BIGINT)");
        assert_eq!(name, "id");
        assert_eq!(typ, "BIGINT");
        assert_eq!(class, None);
    }

    #[test]
    fn test_parse_column_info_with_classification() {
        let (name, typ, class) = parse_column_info("ssn (TEXT) [PHI]");
        assert_eq!(name, "ssn");
        assert_eq!(typ, "TEXT");
        assert_eq!(class, Some("PHI"));
    }

    #[test]
    fn test_classification_badge_phi() {
        let badge = render_classification_badge(Some("PHI"));
        assert!(badge.contains("PHI"));
        assert!(badge.contains("schema-tree__classification"));
    }

    #[test]
    fn test_classification_badge_none() {
        let badge = render_classification_badge(None);
        assert!(badge.is_empty());
    }

    #[test]
    fn test_schema_tree_with_classifications() {
        let tables = vec![
            (
                "patients".to_string(),
                vec!["id (BIGINT)".to_string(), "ssn (TEXT) [PHI]".to_string()],
            ),
        ];

        let html = render_schema_tree(1, "dev-tenant", &tables);

        assert!(html.contains("patients"));
        assert!(html.contains("BIGINT"));
        assert!(html.contains("PHI"));
        assert!(html.contains("schema-tree__classification"));
        // Columns should be hidden by default
        assert!(html.contains("display: none;"));
        // Click-to-insert should be present
        assert!(html.contains("_insertAtCursor"));
    }
}

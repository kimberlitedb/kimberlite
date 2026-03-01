//! Full-Text Search Index
//!
//! SQLite FTS5-backed search across all documentation and blog content.
//! The index is built in-memory at startup from the `ContentStore`.

use std::sync::Mutex;

use rusqlite::{params, Connection};

use crate::content::ContentStore;

/// A single search result with title, section, link, and context snippet.
pub struct SearchResult {
    pub title: String,
    pub section_name: String,
    pub href: String,
    pub snippet: String,
}

/// In-memory SQLite FTS5 search index.
///
/// Wrapped in a `Mutex` because `rusqlite::Connection` is not `Sync`.
/// Contention is negligible — queries take <1ms on ~120 pages.
pub struct SearchIndex {
    conn: Mutex<Connection>,
}

impl SearchIndex {
    /// Build a search index from all docs and blog posts in the content store.
    pub fn build(content: &ContentStore) -> Self {
        let conn = Connection::open_in_memory().expect("failed to open in-memory SQLite");

        conn.execute_batch(
            "CREATE VIRTUAL TABLE search_idx USING fts5(\
                title, section, path, body, \
                tokenize='porter unicode61'\
            );",
        )
        .expect("failed to create FTS5 table");

        // Scope the prepared statement so it's dropped before we move conn
        {
            let mut stmt = conn
                .prepare(
                    "INSERT INTO search_idx (title, section, path, body) VALUES (?1, ?2, ?3, ?4)",
                )
                .expect("failed to prepare insert");

            for (key, page) in content.all_docs() {
                let body = strip_html_tags(&page.content_html);
                stmt.execute(params![page.title, page.section, key, body])
                    .expect("failed to insert doc page");
            }

            for (slug, post) in content.all_blog_posts() {
                let body = strip_html_tags(&post.content_html);
                stmt.execute(params![post.title, "blog", format!("blog/{slug}"), body])
                    .expect("failed to insert blog post");
            }
        }

        let count: i64 = conn
            .query_row("SELECT count(*) FROM search_idx", [], |row| row.get(0))
            .unwrap_or(0);
        tracing::info!("Search index built with {count} pages");

        Self {
            conn: Mutex::new(conn),
        }
    }

    /// Query the search index with BM25 ranking.
    ///
    /// Returns up to `limit` results with highlighted snippets.
    pub fn query(&self, q: &str, limit: usize) -> Vec<SearchResult> {
        let sanitized = sanitize_fts_query(q);
        if sanitized.is_empty() {
            return Vec::new();
        }

        let conn = self.conn.lock().expect("search index lock poisoned");

        let mut stmt = conn
            .prepare(
                "SELECT title, section, path, \
                    snippet(search_idx, 3, '<mark>', '</mark>', '...', 32) \
                 FROM search_idx \
                 WHERE search_idx MATCH ?1 \
                 ORDER BY bm25(search_idx, 50.0, 0.0, 10.0, 1.0) \
                 LIMIT ?2",
            )
            .expect("failed to prepare search query");

        let results = stmt
            .query_map(params![sanitized, limit as i64], |row| {
                let path: String = row.get(2)?;
                let section: String = row.get(1)?;
                let href = if section == "blog" {
                    format!("/{path}")
                } else {
                    format!("/docs/{path}")
                };

                Ok(SearchResult {
                    title: row.get(0)?,
                    section_name: section_display_name(&section),
                    href,
                    snippet: row.get(3)?,
                })
            })
            .expect("search query failed");

        results.filter_map(|r| r.ok()).collect()
    }
}

/// Strip HTML tags from content for plain-text indexing.
fn strip_html_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;

    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(ch),
            _ => {}
        }
    }

    out
}

/// Sanitize user input for FTS5 MATCH syntax.
///
/// Strips FTS5 special characters and appends `*` for prefix matching,
/// so "conse" matches "consensus". Each token becomes `word*` which
/// FTS5 interprets as a prefix query.
fn sanitize_fts_query(q: &str) -> String {
    q.split_whitespace()
        .filter_map(|word| {
            // Strip characters that have special meaning in FTS5
            let clean: String = word
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                .collect();
            if clean.is_empty() {
                None
            } else {
                Some(format!("{clean}*"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Convert a section key to a human-readable name for search results.
fn section_display_name(section: &str) -> String {
    match section {
        "start" => "Getting started".to_string(),
        "concepts" => "Concepts".to_string(),
        "coding" => "Coding".to_string(),
        "coding/quickstarts" => "Quickstarts".to_string(),
        "coding/guides" => "Guides".to_string(),
        "coding/recipes" => "Recipes".to_string(),
        "reference" => "Reference".to_string(),
        "reference/cli" => "CLI reference".to_string(),
        "reference/sql" => "SQL reference".to_string(),
        "reference/sdk" => "SDK reference".to_string(),
        "operating" => "Operations".to_string(),
        "operating/cloud" => "Cloud deployment".to_string(),
        "internals" => "Internals".to_string(),
        "internals/architecture" => "Architecture".to_string(),
        "internals/design" => "Design docs".to_string(),
        "internals/testing" => "Testing".to_string(),
        "internals/formal-verification" => "Formal verification".to_string(),
        "compliance" => "Compliance".to_string(),
        "blog" => "Blog".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_html_tags() {
        assert_eq!(strip_html_tags("<p>hello</p>"), " hello ");
        assert_eq!(strip_html_tags("<b>bold</b> text"), " bold  text");
        assert_eq!(strip_html_tags("no tags"), "no tags");
        assert_eq!(strip_html_tags("<a href=\"x\">link</a>"), " link ");
    }

    #[test]
    fn test_sanitize_fts_query() {
        assert_eq!(sanitize_fts_query("hello world"), "hello* world*");
        assert_eq!(sanitize_fts_query("conse"), "conse*");
        assert_eq!(sanitize_fts_query("con*"), "con*");
        assert_eq!(sanitize_fts_query(""), "");
        // Special chars stripped
        assert_eq!(sanitize_fts_query("OR AND"), "OR* AND*");
        assert_eq!(sanitize_fts_query("\"test\""), "test*");
    }

    #[test]
    fn test_section_display_name() {
        assert_eq!(section_display_name("start"), "Getting started");
        assert_eq!(section_display_name("blog"), "Blog");
        assert_eq!(section_display_name("unknown"), "unknown");
    }
}

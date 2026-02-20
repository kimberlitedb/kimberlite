//! Documentation Handlers

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Redirect},
};

use crate::{
    state::AppState,
    templates::{DocsPageTemplate, SidebarLink, SidebarSection},
    BUILD_VERSION,
};

/// Section display configuration.
struct SectionConfig {
    key: &'static str,
    name: &'static str,
    default_expanded: bool,
}

const SECTION_CONFIGS: &[SectionConfig] = &[
    SectionConfig { key: "start", name: "GETTING STARTED", default_expanded: true },
    SectionConfig { key: "concepts", name: "CONCEPTS", default_expanded: true },
    SectionConfig { key: "coding", name: "CODING", default_expanded: false },
    SectionConfig { key: "coding/quickstarts", name: "QUICKSTARTS", default_expanded: false },
    SectionConfig { key: "coding/guides", name: "GUIDES", default_expanded: false },
    SectionConfig { key: "coding/recipes", name: "RECIPES", default_expanded: false },
    SectionConfig { key: "reference", name: "REFERENCE", default_expanded: true },
    SectionConfig { key: "reference/cli", name: "CLI REFERENCE", default_expanded: false },
    SectionConfig { key: "reference/sql", name: "SQL REFERENCE", default_expanded: false },
    SectionConfig { key: "reference/sdk", name: "SDK REFERENCE", default_expanded: false },
    SectionConfig { key: "operating", name: "OPERATIONS", default_expanded: false },
    SectionConfig { key: "operating/cloud", name: "CLOUD DEPLOYMENT", default_expanded: false },
    SectionConfig { key: "internals", name: "INTERNALS", default_expanded: false },
    SectionConfig { key: "internals/architecture", name: "ARCHITECTURE", default_expanded: false },
    SectionConfig { key: "internals/design", name: "DESIGN DOCS", default_expanded: false },
    SectionConfig { key: "internals/testing", name: "TESTING", default_expanded: false },
    SectionConfig { key: "internals/formal-verification", name: "FORMAL VERIFICATION", default_expanded: false },
    SectionConfig { key: "compliance", name: "COMPLIANCE", default_expanded: false },
];

/// Handler for /docs - redirects to quick-start.
pub async fn docs_index() -> impl IntoResponse {
    Redirect::to("/docs/start/quick-start")
}

/// Handler for /docs/{*path} - renders a doc page from markdown.
pub async fn docs_page(
    State(state): State<AppState>,
    Path(path): Path<String>,
) -> Result<impl IntoResponse, StatusCode> {
    let page = state.content().doc_page(&path).ok_or(StatusCode::NOT_FOUND)?;

    let sidebar_sections = build_sidebar(state.content(), &path);

    Ok(DocsPageTemplate {
        title: format!("{} | Kimberlite Docs", page.title),
        page_title: page.title.clone(),
        active_page: path,
        content_html: page.content_html.clone(),
        headings: page.headings.clone(),
        sidebar_sections,
        v: BUILD_VERSION,
    })
}

/// Build sidebar sections from the `ContentStore`.
fn build_sidebar(content: &crate::content::ContentStore, active_path: &str) -> Vec<SidebarSection> {
    let doc_sections = content.doc_sections();
    let mut sections = Vec::new();

    for config in SECTION_CONFIGS {
        if let Some(page_keys) = doc_sections.get(config.key) {
            let links: Vec<SidebarLink> = page_keys
                .iter()
                .filter_map(|key| {
                    let page = content.doc_page(key)?;
                    // Skip README entries from sidebar
                    if page.slug == "README" {
                        return None;
                    }
                    Some(SidebarLink {
                        title: page.title.clone(),
                        href: format!("/docs/{key}"),
                        is_active: key == active_path,
                    })
                })
                .collect();

            if !links.is_empty() {
                // Determine if this section contains the active page
                let contains_active = links.iter().any(|l| l.is_active);

                sections.push(SidebarSection {
                    name: config.name.to_string(),
                    key: to_camel_case(config.key),
                    links,
                    default_expanded: config.default_expanded || contains_active,
                });
            }
        }
    }

    sections
}

/// Convert a section key like "coding/quickstarts" to camelCase "codingQuickstarts".
fn to_camel_case(key: &str) -> String {
    let parts: Vec<&str> = key.split('/').collect();
    let mut result = String::new();
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            result.push_str(part);
        } else {
            let mut chars = part.chars();
            if let Some(first) = chars.next() {
                result.push(first.to_ascii_uppercase());
                result.extend(chars);
            }
        }
    }
    result
}

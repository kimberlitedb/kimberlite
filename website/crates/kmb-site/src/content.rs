//! Content Loading and Parsing
//!
//! Markdown content with YAML frontmatter support and syntax highlighting.

use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

use chrono::NaiveDate;
use gray_matter::{engine::YAML, Matter, ParsedEntity};
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use serde::Deserialize;
use syntect::{
    html::{ClassStyle, ClassedHTMLGenerator},
    parsing::SyntaxSet,
};

/// A blog post with metadata and rendered content.
#[derive(Clone, Debug)]
pub struct BlogPost {
    pub slug: String,
    pub title: String,
    pub date: NaiveDate,
    pub excerpt: String,
    pub content_html: String,
    pub author_name: Option<String>,
    pub author_avatar: Option<String>,
}

/// Frontmatter for blog posts.
#[derive(Deserialize)]
struct BlogFrontmatter {
    title: String,
    slug: String,
    date: String,
    excerpt: String,
    author_name: Option<String>,
    author_avatar: Option<String>,
}

/// A documentation page with metadata and rendered content.
#[derive(Clone, Debug)]
pub struct DocPage {
    pub slug: String,
    pub section: String,
    pub title: String,
    pub order: i32,
    pub content_html: String,
    pub headings: Vec<TocHeading>,
}

/// Frontmatter for documentation pages.
#[derive(Deserialize)]
struct DocFrontmatter {
    title: String,
    section: String,
    slug: String,
    #[serde(default)]
    order: i32,
}

/// A heading extracted from rendered markdown for table-of-contents.
#[derive(Clone, Debug)]
pub struct TocHeading {
    pub id: String,
    pub text: String,
    pub level: String,
}

/// Rendered markdown content with extracted headings.
struct RenderedContent {
    html: String,
    headings: Vec<TocHeading>,
}

/// Store for all content (blog posts, docs, etc.).
#[derive(Clone, Debug, Default)]
pub struct ContentStore {
    posts: HashMap<String, BlogPost>,
    posts_sorted: Vec<String>,
    docs: HashMap<String, DocPage>,
    doc_sections: BTreeMap<String, Vec<String>>,
}

impl ContentStore {
    /// Load all content from the filesystem.
    pub fn load() -> Self {
        let mut store = Self::default();
        store.load_blog_posts();

        let docs_path = std::env::var("DOCS_PATH").unwrap_or_else(|_| "../docs".to_string());
        store.load_docs(Path::new(&docs_path));

        store
    }

    fn load_blog_posts(&mut self) {
        let blog_dir = Path::new("content/blog");

        if !blog_dir.exists() {
            tracing::warn!("Blog directory does not exist: {:?}", blog_dir);
            return;
        }

        let Ok(entries) = fs::read_dir(blog_dir) else {
            tracing::error!("Failed to read blog directory");
            return;
        };

        let matter = Matter::<YAML>::new();

        for entry in entries.flatten() {
            let path = entry.path();

            if path.extension().is_some_and(|ext| ext == "md") {
                if let Some(post) = Self::parse_blog_post(&path, &matter) {
                    self.posts_sorted.push(post.slug.clone());
                    self.posts.insert(post.slug.clone(), post);
                }
            }
        }

        // Sort by date descending
        self.posts_sorted.sort_by(|a, b| {
            let post_a = self.posts.get(a);
            let post_b = self.posts.get(b);
            match (post_a, post_b) {
                (Some(a), Some(b)) => b.date.cmp(&a.date),
                _ => std::cmp::Ordering::Equal,
            }
        });
    }

    fn parse_blog_post(path: &Path, matter: &Matter<YAML>) -> Option<BlogPost> {
        let content = fs::read_to_string(path).ok()?;
        let parsed: ParsedEntity<BlogFrontmatter> = matter.parse(&content).ok()?;

        let frontmatter = parsed.data?;

        let date = NaiveDate::parse_from_str(&frontmatter.date, "%Y-%m-%d").ok()?;

        let rendered = render_markdown_with_highlighting(&parsed.content, "");

        Some(BlogPost {
            slug: frontmatter.slug,
            title: frontmatter.title,
            date,
            excerpt: frontmatter.excerpt,
            content_html: rendered.html,
            author_name: frontmatter.author_name,
            author_avatar: frontmatter.author_avatar,
        })
    }

    fn load_docs(&mut self, docs_dir: &Path) {
        if !docs_dir.exists() {
            tracing::warn!("Docs directory does not exist: {:?}", docs_dir);
            return;
        }

        let matter = Matter::<YAML>::new();
        let mut files = Vec::new();
        Self::collect_md_files(docs_dir, &mut files);

        for path in files {
            if let Some(page) = Self::parse_doc_page(&path, &matter) {
                let key = format!("{}/{}", page.section, page.slug);
                self.doc_sections
                    .entry(page.section.clone())
                    .or_default()
                    .push(key.clone());
                self.docs.insert(key, page);
            }
        }

        // Sort each section's pages by order
        for slugs in self.doc_sections.values_mut() {
            slugs.sort_by(|a, b| {
                let page_a = self.docs.get(a);
                let page_b = self.docs.get(b);
                match (page_a, page_b) {
                    (Some(a), Some(b)) => a.order.cmp(&b.order),
                    _ => std::cmp::Ordering::Equal,
                }
            });
        }

        tracing::info!(
            "Loaded {} doc pages across {} sections",
            self.docs.len(),
            self.doc_sections.len()
        );
    }

    fn collect_md_files(dir: &Path, files: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                Self::collect_md_files(&path, files);
            } else if path.extension().is_some_and(|ext| ext == "md") {
                files.push(path);
            }
        }
    }

    fn parse_doc_page(path: &Path, matter: &Matter<YAML>) -> Option<DocPage> {
        let content = fs::read_to_string(path).ok()?;
        let parsed: ParsedEntity<DocFrontmatter> = matter.parse(&content).ok()?;

        let frontmatter = parsed.data?;
        let doc_key = format!("{}/{}", frontmatter.section, frontmatter.slug);
        let rendered = render_markdown_with_highlighting(&parsed.content, &doc_key);

        Some(DocPage {
            slug: frontmatter.slug,
            section: frontmatter.section,
            title: frontmatter.title,
            order: frontmatter.order,
            content_html: rendered.html,
            headings: rendered.headings,
        })
    }

    /// Get all blog posts sorted by date (newest first).
    pub fn blog_posts(&self) -> Vec<&BlogPost> {
        self.posts_sorted.iter().filter_map(|slug| self.posts.get(slug)).collect()
    }

    /// Get a single blog post by slug.
    pub fn blog_post(&self, slug: &str) -> Option<&BlogPost> {
        self.posts.get(slug)
    }

    /// Get a single doc page by path (e.g., "start/quick-start").
    pub fn doc_page(&self, path: &str) -> Option<&DocPage> {
        self.docs.get(path)
    }

    /// Get all doc sections with their ordered page keys.
    pub fn doc_sections(&self) -> &BTreeMap<String, Vec<String>> {
        &self.doc_sections
    }
}

/// Render markdown to HTML with syntax highlighting and heading extraction.
///
/// `doc_path` is the path of this document relative to the docs root (e.g.,
/// `"start/quick-start"`). It is used to rewrite relative `.md` links to web URLs.
/// Pass `""` for content that is not part of the docs hierarchy (e.g. blog posts).
fn render_markdown_with_highlighting(markdown: &str, doc_path: &str) -> RenderedContent {
    let ss = SyntaxSet::load_defaults_newlines();

    let options = Options::all();
    let parser = Parser::new_ext(markdown, options);

    let mut html_output = String::new();
    let mut headings = Vec::new();
    let mut in_code_block = false;
    let mut code_block_lang: Option<String> = None;
    let mut code_block_content = String::new();
    let mut in_heading = false;
    let mut heading_text = String::new();
    let mut seen_h1 = false;

    for event in parser {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                let lvl = level as u8;
                if lvl == 1 && !seen_h1 {
                    // Skip the first h1 (it's rendered in the template header)
                    in_heading = true;
                    heading_text.clear();
                } else if lvl == 2 || lvl == 3 {
                    in_heading = true;
                    heading_text.clear();
                } else {
                    pulldown_cmark::html::push_html(
                        &mut html_output,
                        std::iter::once(Event::Start(Tag::Heading {
                            level,
                            id: None,
                            classes: Vec::new(),
                            attrs: Vec::new(),
                        })),
                    );
                }
            }
            Event::End(TagEnd::Heading(level)) => {
                let lvl = level as u8;
                if in_heading && lvl == 1 && !seen_h1 {
                    // Skip rendering the first h1 entirely
                    seen_h1 = true;
                    in_heading = false;
                } else if in_heading && (lvl == 2 || lvl == 3) {
                    let id = slugify(&heading_text);
                    headings.push(TocHeading {
                        id: id.clone(),
                        text: heading_text.clone(),
                        level: format!("h{lvl}"),
                    });
                    let _ = write!(
                        html_output,
                        "<h{lvl} id=\"{id}\">{text}</h{lvl}>",
                        text = html_escape(&heading_text)
                    );
                    in_heading = false;
                } else {
                    pulldown_cmark::html::push_html(
                        &mut html_output,
                        std::iter::once(Event::End(TagEnd::Heading(level))),
                    );
                }
            }
            Event::Text(ref text) if in_heading => {
                heading_text.push_str(text);
            }
            Event::Code(ref code) if in_heading => {
                heading_text.push_str(code);
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                in_code_block = true;
                code_block_lang = match kind {
                    CodeBlockKind::Fenced(lang) => {
                        let lang_str = lang.to_string();
                        // Strip attributes like "rust,ignore" -> "rust"
                        let clean = lang_str.split(',').next().unwrap_or("").trim().to_string();
                        if clean.is_empty() { None } else { Some(clean) }
                    }
                    CodeBlockKind::Indented => None,
                };
                code_block_content.clear();
            }
            Event::End(TagEnd::CodeBlock) => {
                let highlighted =
                    highlight_code(&code_block_content, code_block_lang.as_deref(), &ss);

                let lang_class = code_block_lang
                    .as_ref()
                    .map(|l| format!(" language-{l}"))
                    .unwrap_or_default();

                let _ = write!(
                    html_output,
                    "<pre class=\"highlight{lang_class}\"><code>{highlighted}</code></pre>"
                );
                in_code_block = false;
                code_block_lang = None;
            }
            Event::Text(text) if in_code_block => {
                code_block_content.push_str(&text);
            }
            Event::Text(text) => {
                // Replace emojis with SVG icons in regular text
                let processed = replace_emojis_with_icons(&text);
                html_output.push_str(&processed);
            }
            Event::Start(Tag::Link { link_type, dest_url, title, id }) => {
                let rewritten = rewrite_doc_link(&dest_url, doc_path);
                let new_event = Event::Start(Tag::Link {
                    link_type,
                    dest_url: rewritten.into(),
                    title,
                    id,
                });
                pulldown_cmark::html::push_html(&mut html_output, std::iter::once(new_event));
            }
            other => {
                pulldown_cmark::html::push_html(&mut html_output, std::iter::once(other));
            }
        }
    }

    RenderedContent {
        html: html_output,
        headings,
    }
}

/// Slugify a heading text for use as an HTML id.
fn slugify(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

/// Minimal HTML escaping for heading text.
fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Replace emojis with SVG icons from sustyicons.
fn replace_emojis_with_icons(text: &str) -> String {
    text
        // Check marks and X marks
        .replace("✅", "<svg class=\"inline-icon\" width=\"16\" height=\"16\" aria-label=\"Yes\"><use href=\"/public/icons/sustyicons-all-v1-1.svg#circle-tick\"/></svg>")
        .replace("✓", "<svg class=\"inline-icon\" width=\"16\" height=\"16\" aria-label=\"Yes\"><use href=\"/public/icons/sustyicons-all-v1-1.svg#tick\"/></svg>")
        .replace("✔️", "<svg class=\"inline-icon\" width=\"16\" height=\"16\" aria-label=\"Yes\"><use href=\"/public/icons/sustyicons-all-v1-1.svg#circle-tick\"/></svg>")
        .replace("❌", "<svg class=\"inline-icon\" width=\"16\" height=\"16\" aria-label=\"No\"><use href=\"/public/icons/sustyicons-all-v1-1.svg#circle-cross\"/></svg>")
        .replace("❎", "<svg class=\"inline-icon\" width=\"16\" height=\"16\" aria-label=\"No\"><use href=\"/public/icons/sustyicons-all-v1-1.svg#circle-cross\"/></svg>")
        .replace("✖️", "<svg class=\"inline-icon\" width=\"16\" height=\"16\" aria-label=\"No\"><use href=\"/public/icons/sustyicons-all-v1-1.svg#close\"/></svg>")
        // Warning and alerts
        .replace("⚠️", "<svg class=\"inline-icon\" width=\"16\" height=\"16\" aria-label=\"Warning\"><use href=\"/public/icons/sustyicons-all-v1-1.svg#warning\"/></svg>")
        .replace("⚠", "<svg class=\"inline-icon\" width=\"16\" height=\"16\" aria-label=\"Warning\"><use href=\"/public/icons/sustyicons-all-v1-1.svg#warning\"/></svg>")
        // Other common emojis
        .replace("🔄", "<svg class=\"inline-icon\" width=\"16\" height=\"16\" aria-label=\"Refresh\"><use href=\"/public/icons/sustyicons-all-v1-1.svg#arrow-sync\"/></svg>")
        .replace("🧪", "<svg class=\"inline-icon\" width=\"16\" height=\"16\" aria-label=\"Testing\"><use href=\"/public/icons/sustyicons-all-v1-1.svg#testtube\"/></svg>")
        .replace("⏮️", "<svg class=\"inline-icon\" width=\"16\" height=\"16\" aria-label=\"Rewind\"><use href=\"/public/icons/sustyicons-all-v1-1.svg#back-arrow\"/></svg>")
        .replace("🐛", "<svg class=\"inline-icon\" width=\"16\" height=\"16\" aria-label=\"Bug\"><use href=\"/public/icons/sustyicons-all-v1-1.svg#bug\"/></svg>")
        .replace("⚙️", "<svg class=\"inline-icon\" width=\"16\" height=\"16\" aria-label=\"Settings\"><use href=\"/public/icons/sustyicons-all-v1-1.svg#cog\"/></svg>")
        .replace("⚙", "<svg class=\"inline-icon\" width=\"16\" height=\"16\" aria-label=\"Settings\"><use href=\"/public/icons/sustyicons-all-v1-1.svg#cog\"/></svg>")
        .replace("🔒", "<svg class=\"inline-icon\" width=\"16\" height=\"16\" aria-label=\"Locked\"><use href=\"/public/icons/sustyicons-all-v1-1.svg#locked\"/></svg>")
        .replace("🔓", "<svg class=\"inline-icon\" width=\"16\" height=\"16\" aria-label=\"Unlocked\"><use href=\"/public/icons/sustyicons-all-v1-1.svg#unlocked\"/></svg>")
}

/// Rewrites a Markdown link destination for website rendering.
///
/// - External links (`http`/`https`/`mailto`) → unchanged
/// - Anchor-only links (`#...`) → unchanged
/// - Relative `.md` links within `docs/` → `/docs/{resolved-path-without-.md}{#anchor}`
/// - Relative `.md` links pointing outside `docs/` → GitHub blob URL
fn rewrite_doc_link(url: &str, current_doc_path: &str) -> String {
    // Leave external and anchor-only links alone
    if url.starts_with("http://")
        || url.starts_with("https://")
        || url.starts_with('#')
        || url.starts_with("mailto:")
    {
        return url.to_string();
    }

    // Split off anchor fragment (e.g., "quick-start.md#step-1")
    let (path_part, anchor) = match url.find('#') {
        Some(i) => (&url[..i], &url[i..]),
        None => (url, ""),
    };

    // Only process .md links
    if !path_part.ends_with(".md") {
        return url.to_string();
    }

    // Resolve relative path from current doc's directory within docs/
    // current_doc_path = "start/quick-start" → directory = ["start"]
    let mut parts: Vec<&str> = current_doc_path.split('/').collect();
    parts.pop(); // drop the slug, keep directory segments

    let mut went_above_root = false;
    for segment in path_part.split('/') {
        match segment {
            ".." => {
                if parts.pop().is_none() {
                    went_above_root = true;
                }
            }
            "." | "" => {}
            s => parts.push(s),
        }
    }

    if went_above_root {
        // Points outside docs/ (e.g., ../ROADMAP.md) → GitHub
        format!(
            "https://github.com/kimberlitedb/kimberlite/blob/main/{}{}",
            path_part.trim_start_matches("../"),
            anchor
        )
    } else {
        let web_path = parts.join("/");
        let web_path = web_path.strip_suffix(".md").unwrap_or(&web_path);
        format!("/docs/{}{}", web_path, anchor)
    }
}

/// Highlight code using syntect with CSS classes.
fn highlight_code(code: &str, lang: Option<&str>, ss: &SyntaxSet) -> String {
    let syntax = lang
        .and_then(|l| ss.find_syntax_by_token(l))
        .unwrap_or_else(|| ss.find_syntax_plain_text());

    let mut html_generator = ClassedHTMLGenerator::new_with_class_style(
        syntax,
        ss,
        ClassStyle::Spaced,
    );

    for line in code.lines() {
        // ClassedHTMLGenerator expects lines without trailing newlines
        let _ = html_generator.parse_html_for_line_which_includes_newline(&format!("{line}\n"));
    }

    html_generator.finalize()
}

#[cfg(test)]
mod tests {
    use super::rewrite_doc_link;

    #[test]
    fn test_external_url_unchanged() {
        assert_eq!(
            rewrite_doc_link("https://rust-lang.org", "start/quick-start"),
            "https://rust-lang.org"
        );
    }

    #[test]
    fn test_anchor_only_unchanged() {
        assert_eq!(
            rewrite_doc_link("#step-1", "start/quick-start"),
            "#step-1"
        );
    }

    #[test]
    fn test_same_directory_rewrite() {
        assert_eq!(
            rewrite_doc_link("quick-start.md", "start/installation"),
            "/docs/start/quick-start"
        );
    }

    #[test]
    fn test_parent_directory_rewrite() {
        assert_eq!(
            rewrite_doc_link("../concepts/rbac.md", "start/first-app"),
            "/docs/concepts/rbac"
        );
    }

    #[test]
    fn test_anchor_fragment_preserved() {
        assert_eq!(
            rewrite_doc_link("quick-start.md#step-1", "start/installation"),
            "/docs/start/quick-start#step-1"
        );
    }

    #[test]
    fn test_outside_docs_falls_back_to_github() {
        // From start/quick-start, one `..` reaches docs root; two go above it.
        assert_eq!(
            rewrite_doc_link("../../ROADMAP.md", "start/quick-start"),
            "https://github.com/kimberlitedb/kimberlite/blob/main/ROADMAP.md"
        );
    }

    #[test]
    fn test_single_parent_within_docs() {
        // From start/quick-start, one `..` resolves to docs root — stays in /docs/
        assert_eq!(
            rewrite_doc_link("../ROADMAP.md", "start/quick-start"),
            "/docs/ROADMAP"
        );
    }

    #[test]
    fn test_non_md_link_unchanged() {
        assert_eq!(
            rewrite_doc_link("image.png", "start/quick-start"),
            "image.png"
        );
    }
}

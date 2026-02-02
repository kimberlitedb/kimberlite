//! Askama Templates
//!
//! Template structs for rendering HTML pages.

use askama::Template;
use askama_web::WebTemplate;

use crate::BUILD_VERSION;

/// Home page template.
#[derive(Template, WebTemplate)]
#[template(path = "home.html")]
pub struct HomeTemplate {
    pub title: String,
    pub tagline: String,
    /// Build version for cache busting static assets.
    pub v: &'static str,
}

impl HomeTemplate {
    pub fn new(title: impl Into<String>, tagline: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            tagline: tagline.into(),
            v: BUILD_VERSION,
        }
    }
}

/// Blog list page template.
#[derive(Template, WebTemplate)]
#[template(path = "blog/index.html")]
pub struct BlogListTemplate {
    pub title: String,
    pub posts: Vec<PostSummary>,
    /// Build version for cache busting static assets.
    pub v: &'static str,
}

impl BlogListTemplate {
    pub fn new(title: impl Into<String>, posts: Vec<PostSummary>) -> Self {
        Self {
            title: title.into(),
            posts,
            v: BUILD_VERSION,
        }
    }
}

/// Summary of a blog post for listing.
pub struct PostSummary {
    pub slug: String,
    pub title: String,
    pub date: String,
    pub excerpt: String,
    pub author_name: Option<String>,
    pub author_avatar: Option<String>,
}

/// Individual blog post template.
#[derive(Template, WebTemplate)]
#[template(path = "blog/post.html")]
pub struct BlogPostTemplate {
    pub title: String,
    pub post_title: String,
    pub date: String,
    pub content_html: String,
    pub author_name: Option<String>,
    pub author_avatar: Option<String>,
    /// Build version for cache busting static assets.
    pub v: &'static str,
}

impl BlogPostTemplate {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        title: impl Into<String>,
        post_title: impl Into<String>,
        date: impl Into<String>,
        content_html: impl Into<String>,
        author_name: Option<String>,
        author_avatar: Option<String>,
    ) -> Self {
        Self {
            title: title.into(),
            post_title: post_title.into(),
            date: date.into(),
            content_html: content_html.into(),
            author_name,
            author_avatar,
            v: BUILD_VERSION,
        }
    }
}

/// Architecture deep dive page template.
#[derive(Template, WebTemplate)]
#[template(path = "architecture.html")]
pub struct ArchitectureTemplate {
    pub title: String,
    /// Build version for cache busting static assets.
    pub v: &'static str,
}

impl ArchitectureTemplate {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            v: BUILD_VERSION,
        }
    }
}

/// Table of contents heading entry.
#[derive(Clone)]
pub struct TocHeading {
    pub id: String,
    pub text: String,
    pub level: String,
}

/// Documentation Quick Start page template.
#[derive(Template, WebTemplate)]
#[template(path = "docs/quick-start.html")]
pub struct DocsQuickStartTemplate {
    pub title: String,
    pub active_page: String,
    pub headings: Vec<TocHeading>,
    /// Build version for cache busting static assets.
    pub v: &'static str,
}

impl DocsQuickStartTemplate {
    pub fn new(title: impl Into<String>) -> Self {
        let headings = vec![
            TocHeading {
                id: "installation".to_string(),
                text: "1. Download".to_string(),
                level: "h2".to_string(),
            },
            TocHeading {
                id: "initialize".to_string(),
                text: "2. Initialize".to_string(),
                level: "h2".to_string(),
            },
            TocHeading {
                id: "start".to_string(),
                text: "3. Start the Server".to_string(),
                level: "h2".to_string(),
            },
            TocHeading {
                id: "connect".to_string(),
                text: "4. Connect and Query".to_string(),
                level: "h2".to_string(),
            },
            TocHeading {
                id: "next-steps".to_string(),
                text: "Next Steps".to_string(),
                level: "h2".to_string(),
            },
        ];
        Self {
            title: title.into(),
            active_page: "quick-start".to_string(),
            headings,
            v: BUILD_VERSION,
        }
    }
}

/// Documentation CLI Reference page template.
#[derive(Template, WebTemplate)]
#[template(path = "docs/reference/cli.html")]
pub struct DocsCliTemplate {
    pub title: String,
    pub active_page: String,
    pub headings: Vec<TocHeading>,
    /// Build version for cache busting static assets.
    pub v: &'static str,
}

impl DocsCliTemplate {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            active_page: "cli".to_string(),
            headings: vec![],
            v: BUILD_VERSION,
        }
    }
}

/// Documentation SQL Reference page template.
#[derive(Template, WebTemplate)]
#[template(path = "docs/reference/sql.html")]
pub struct DocsSqlTemplate {
    pub title: String,
    pub active_page: String,
    pub headings: Vec<TocHeading>,
    /// Build version for cache busting static assets.
    pub v: &'static str,
}

impl DocsSqlTemplate {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            active_page: "sql".to_string(),
            headings: vec![],
            v: BUILD_VERSION,
        }
    }
}

/// Download page template.
#[derive(Template, WebTemplate)]
#[template(path = "download.html")]
pub struct DownloadTemplate {
    pub title: String,
    /// Build version for cache busting static assets.
    pub v: &'static str,
}

impl DownloadTemplate {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            v: BUILD_VERSION,
        }
    }
}

/// Pressurecraft: FCIS Flow diagram page template.
#[derive(Template, WebTemplate)]
#[template(path = "pressurecraft/fcis-flow.html")]
pub struct PressurecraftFcisFlowTemplate {
    pub title: String,
    /// Build version for cache busting static assets.
    pub v: &'static str,
}

impl PressurecraftFcisFlowTemplate {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            v: BUILD_VERSION,
        }
    }
}

/// Pressurecraft: Determinism demo page template.
#[derive(Template, WebTemplate)]
#[template(path = "pressurecraft/determinism-demo.html")]
pub struct PressurecraftDeterminismTemplate {
    pub title: String,
    /// Build version for cache busting static assets.
    pub v: &'static str,
}

impl PressurecraftDeterminismTemplate {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            v: BUILD_VERSION,
        }
    }
}

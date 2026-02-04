//! Documentation tests
//!
//! This module runs tests on code examples in documentation files.
//! Each documentation file gets its own test module that includes
//! the markdown file and validates that Rust code blocks compile.
//!
//! Code blocks are tested based on their annotations:
//! - ``` rust ``` - Compiles and runs
//! - ``` rust,no_run ``` - Compiles but doesn't run
//! - ``` rust,ignore ``` - Skipped (for pseudocode)
//!
//! Usage:
//! ```bash
//! cargo test --test doc_tests
//! just test-docs
//! ```

// Quickstarts
#[doc = include_str!("../../docs/coding/quickstarts/rust.md")]
#[cfg(doctest)]
pub struct _DocTestRustQuickstart;

// Guides
#[doc = include_str!("../../docs/coding/guides/migrations.md")]
#[cfg(doctest)]
pub struct _DocTestMigrationsGuide;

// Recipes - All new recipe files with Rust code examples
#[doc = include_str!("../../docs/coding/recipes/time-travel-queries.md")]
#[cfg(doctest)]
pub struct _DocTestTimeTravelQueries;

#[doc = include_str!("../../docs/coding/recipes/audit-trails.md")]
#[cfg(doctest)]
pub struct _DocTestAuditTrails;

#[doc = include_str!("../../docs/coding/recipes/encryption.md")]
#[cfg(doctest)]
pub struct _DocTestEncryption;

#[doc = include_str!("../../docs/coding/recipes/data-classification.md")]
#[cfg(doctest)]
pub struct _DocTestDataClassification;

#[doc = include_str!("../../docs/coding/recipes/multi-tenant-queries.md")]
#[cfg(doctest)]
pub struct _DocTestMultiTenantQueries;

// NOTE: Python and TypeScript quickstarts contain non-Rust code
// and are not included here. Add language-specific doc tests when
// implementing Python/TypeScript test infrastructure.

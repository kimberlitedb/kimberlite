//! Audit context propagation.
//!
//! AUDIT-2026-04 S2.4 — provides an ambient context carrier so
//! apps can set `{ actor, reason, request_id, correlation_id }`
//! once per request and have every nested Kimberlite operation
//! pick it up for structured logging / distributed tracing.
//!
//! The sync variant uses `thread_local!`. When the async client
//! lands (S2.1) an async-aware variant can be added using
//! `tokio::task_local!` without breaking this one.
//!
//! # Example
//!
//! ```
//! use kimberlite_client::audit_context::{AuditContext, run_with_audit, current_audit};
//!
//! let ctx = AuditContext::new("alice@example.com", "chart-review");
//! run_with_audit(ctx, || {
//!     let active = current_audit().expect("set by enclosing scope");
//!     assert_eq!(active.actor(), "alice@example.com");
//! });
//!
//! // Outside the scope, no context is active.
//! assert!(current_audit().is_none());
//! ```

use std::cell::RefCell;

thread_local! {
    static AUDIT_CTX: RefCell<Option<AuditContext>> = const { RefCell::new(None) };
}

/// Structured audit context carried through a call chain.
///
/// `actor` and `reason` are mandatory in regulated-industry apps
/// (HIPAA minimum-necessary, GDPR purpose limitation, FedRAMP
/// audit-trail completeness). `request_id` correlates with server
/// logs; `correlation_id` ties together a span of related calls
/// (typically an HTTP trace ID).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditContext {
    actor: String,
    reason: String,
    request_id: Option<String>,
    correlation_id: Option<String>,
}

impl AuditContext {
    /// Build a context with mandatory actor + reason and no IDs.
    /// Use the builder methods for optional fields.
    pub fn new(actor: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            actor: actor.into(),
            reason: reason.into(),
            request_id: None,
            correlation_id: None,
        }
    }

    #[must_use]
    pub fn with_request_id(mut self, id: impl Into<String>) -> Self {
        self.request_id = Some(id.into());
        self
    }

    #[must_use]
    pub fn with_correlation_id(mut self, id: impl Into<String>) -> Self {
        self.correlation_id = Some(id.into());
        self
    }

    pub fn actor(&self) -> &str {
        &self.actor
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    pub fn correlation_id(&self) -> Option<&str> {
        self.correlation_id.as_deref()
    }

    /// Project the in-process context to the wire `AuditMetadata`
    /// carried on every client request. Empty strings are normalised
    /// to `None` so servers can distinguish "caller provided nothing"
    /// from "caller provided blank".
    pub fn to_wire(&self) -> kimberlite_wire::AuditMetadata {
        fn nonempty(s: &str) -> Option<String> {
            if s.is_empty() {
                None
            } else {
                Some(s.to_string())
            }
        }
        kimberlite_wire::AuditMetadata {
            actor: nonempty(&self.actor),
            reason: nonempty(&self.reason),
            correlation_id: self.correlation_id.clone(),
            idempotency_key: self.request_id.clone(),
        }
    }
}

/// Run `fn` with `ctx` as the active audit context on the current
/// thread.
///
/// Nested calls see the innermost context; outer contexts are
/// restored on return even if `fn` panics (via `Drop`).
pub fn run_with_audit<T, F>(ctx: AuditContext, f: F) -> T
where
    F: FnOnce() -> T,
{
    let previous = AUDIT_CTX.with(|slot| slot.borrow_mut().replace(ctx));
    let _guard = CtxGuard { previous };
    f()
}

/// Return a clone of the current audit context, or `None` if no
/// context is active.
pub fn current_audit() -> Option<AuditContext> {
    AUDIT_CTX.with(|slot| slot.borrow().clone())
}

/// Like [`current_audit`] but panics if no context is active.
/// Use at sites that refuse to run without attribution
/// (break-glass queries, PHI exports, compliance reports).
///
/// # Panics
///
/// Panics with a clear diagnostic if no context is active. This
/// is intentional — a PHI export with no audit attribution is a
/// compliance bug that must be caught early.
pub fn require_audit() -> AuditContext {
    current_audit().expect(
        "require_audit(): no audit context active — wrap the call in \
         run_with_audit(ctx, || ...)",
    )
}

/// Directly install `ctx` as the active audit context on the current
/// thread **without** RAII scoping. Typically you want
/// [`run_with_audit`] instead — it restores the previous context on
/// return. This variant exists for foreign-function bindings that
/// can't express a Rust closure lifetime (Python / TS / Go / Java FFI
/// wrappers call `set_thread_audit` → invoke → `clear_thread_audit`).
///
/// Returns the previously-active context so callers that want to
/// implement their own RAII discipline can restore it later.
pub fn set_thread_audit(ctx: AuditContext) -> Option<AuditContext> {
    AUDIT_CTX.with(|slot| slot.borrow_mut().replace(ctx))
}

/// Clear the thread-local audit context, returning whatever was
/// previously active (or `None`).
pub fn clear_thread_audit() -> Option<AuditContext> {
    AUDIT_CTX.with(|slot| slot.borrow_mut().take())
}

/// RAII guard that restores the previous audit context when
/// `run_with_audit` returns (including on panic).
struct CtxGuard {
    previous: Option<AuditContext>,
}

impl Drop for CtxGuard {
    fn drop(&mut self) {
        AUDIT_CTX.with(|slot| {
            *slot.borrow_mut() = self.previous.take();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Barrier};
    use std::thread;

    fn ctx(actor: &str, reason: &str) -> AuditContext {
        AuditContext::new(actor, reason)
    }

    #[test]
    fn no_context_returns_none() {
        assert!(current_audit().is_none());
    }

    #[test]
    fn run_with_audit_exposes_context() {
        let observed = run_with_audit(ctx("alice", "review"), || {
            current_audit().map(|c| (c.actor().to_string(), c.reason().to_string()))
        });
        assert_eq!(
            observed,
            Some(("alice".to_string(), "review".to_string()))
        );
    }

    #[test]
    fn context_is_cleared_after_scope() {
        run_with_audit(ctx("alice", "review"), || {});
        assert!(current_audit().is_none());
    }

    #[test]
    fn nested_scopes_restore_outer_context() {
        run_with_audit(ctx("alice", "outer"), || {
            assert_eq!(current_audit().unwrap().actor(), "alice");

            run_with_audit(ctx("bob", "inner"), || {
                assert_eq!(current_audit().unwrap().actor(), "bob");
            });

            // Outer restored.
            assert_eq!(current_audit().unwrap().actor(), "alice");
        });
    }

    #[test]
    #[should_panic(expected = "no audit context active")]
    fn require_audit_panics_without_context() {
        let _ = require_audit();
    }

    #[test]
    fn require_audit_returns_context_when_active() {
        run_with_audit(ctx("alice", "review"), || {
            let c = require_audit();
            assert_eq!(c.actor(), "alice");
        });
    }

    #[test]
    fn contexts_are_thread_isolated() {
        // Parallel threads each set their own context; they must
        // not see each other's values. Uses a barrier to ensure
        // both threads are concurrently inside their scopes.
        let barrier = Arc::new(Barrier::new(2));
        let leaks = Arc::new(AtomicUsize::new(0));

        let b1 = Arc::clone(&barrier);
        let l1 = Arc::clone(&leaks);
        let t1 = thread::spawn(move || {
            run_with_audit(ctx("alice", "t1"), || {
                b1.wait();
                if current_audit().unwrap().actor() != "alice" {
                    l1.fetch_add(1, Ordering::SeqCst);
                }
                b1.wait();
            });
        });

        let b2 = Arc::clone(&barrier);
        let l2 = Arc::clone(&leaks);
        let t2 = thread::spawn(move || {
            run_with_audit(ctx("bob", "t2"), || {
                b2.wait();
                if current_audit().unwrap().actor() != "bob" {
                    l2.fetch_add(1, Ordering::SeqCst);
                }
                b2.wait();
            });
        });

        t1.join().unwrap();
        t2.join().unwrap();
        assert_eq!(leaks.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn builder_fields_populate_correctly() {
        let c = AuditContext::new("alice", "export")
            .with_request_id("req-123")
            .with_correlation_id("corr-456");
        assert_eq!(c.actor(), "alice");
        assert_eq!(c.reason(), "export");
        assert_eq!(c.request_id(), Some("req-123"));
        assert_eq!(c.correlation_id(), Some("corr-456"));
    }
}

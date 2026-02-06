//! # kimberlite-abac: Attribute-Based Access Control
//!
//! Provides context-aware access decisions based on user, resource, and environment attributes.
//! Extends RBAC with fine-grained, dynamic access control.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │  Access Request                              │
//! │  (User + Resource + Environment Attributes)  │
//! └─────────────────┬───────────────────────────┘
//!                   │
//!                   ▼
//! ┌─────────────────────────────────────────────┐
//! │  ABAC Evaluator                              │
//! │  ├─ Evaluate rules by priority               │
//! │  ├─ Match conditions against attributes      │
//! │  └─ Return Allow/Deny decision               │
//! └─────────────────┬───────────────────────────┘
//!                   │
//!                   ▼
//! ┌─────────────────────────────────────────────┐
//! │  Decision                                    │
//! │  - Effect (Allow/Deny)                       │
//! │  - Matched rule name                         │
//! │  - Human-readable reason                     │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! ## Standard Policies
//!
//! Pre-built policies for common compliance frameworks:
//!
//! - **HIPAA**: PHI access only during business hours with clearance >= 2
//! - **`FedRAMP`**: Deny access from outside the US
//! - **PCI DSS**: PCI data only from server devices with clearance >= 2
//!
//! ## Examples
//!
//! ```
//! use kimberlite_abac::policy::{AbacPolicy, Rule, Condition, Effect};
//! use kimberlite_abac::attributes::{UserAttributes, ResourceAttributes, EnvironmentAttributes, DeviceType};
//! use kimberlite_abac::evaluator;
//! use kimberlite_types::DataClass;
//! use chrono::Utc;
//!
//! // Create a policy that denies access outside business hours
//! let policy = AbacPolicy::new(Effect::Allow)
//!     .with_rule(Rule {
//!         name: "deny-after-hours".to_string(),
//!         effect: Effect::Deny,
//!         conditions: vec![
//!             Condition::Not(Box::new(Condition::BusinessHoursOnly)),
//!         ],
//!         priority: 10,
//!     });
//!
//! let user = UserAttributes::new("analyst", "engineering", 1);
//! let resource = ResourceAttributes::new(DataClass::Confidential, 1, "metrics");
//! let env = EnvironmentAttributes::from_timestamp(Utc::now(), "US");
//!
//! let decision = evaluator::evaluate(&policy, &user, &resource, &env);
//! // Decision depends on whether it is currently business hours (UTC)
//! ```

pub mod attributes;
pub mod evaluator;
pub mod policy;

// Kani proofs for bounded model checking
#[cfg(any(test, kani))]
mod kani_proofs;

pub use attributes::{EnvironmentAttributes, ResourceAttributes, UserAttributes};
pub use evaluator::{Decision, evaluate};
pub use policy::{AbacPolicy, Condition, Effect as PolicyEffect, Rule};

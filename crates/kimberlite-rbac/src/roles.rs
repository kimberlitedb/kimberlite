#![allow(clippy::match_same_arms)]
//! Role definitions for RBAC.
//!
//! Defines 4 roles with escalating privileges:
//! - Auditor: Read-only audit logs (most restrictive)
//! - User: Read/write own data
//! - Analyst: Read across tenants (no write)
//! - Admin: Full access (least restrictive)

use serde::{Deserialize, Serialize};

/// Role in the access control system.
///
/// Roles are ordered from least to most privileged:
/// Auditor < User < Analyst < Admin
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Role {
    /// Read-only access to audit logs.
    ///
    /// **Permissions:**
    /// - Read audit logs
    /// - Cannot access application data
    /// - Cannot write or delete
    ///
    /// **Use Cases:**
    /// - Compliance auditors
    /// - Security team reviewing access patterns
    /// - External audit firms (SOX, HIPAA compliance)
    Auditor,

    /// Standard user with access to own data.
    ///
    /// **Permissions:**
    /// - Read/write own data (tenant-isolated)
    /// - Cannot access other tenants' data
    /// - Cannot access audit logs
    /// - Cannot export data
    ///
    /// **Use Cases:**
    /// - Application end-users
    /// - Service accounts with limited scope
    User,

    /// Analyst with cross-tenant read access.
    ///
    /// **Permissions:**
    /// - Read across all tenants (no tenant isolation)
    /// - Cannot write or delete
    /// - Can export data for analysis
    /// - Cannot access audit logs
    ///
    /// **Use Cases:**
    /// - Business intelligence analysts
    /// - Data scientists
    /// - Reporting systems
    Analyst,

    /// Administrator with full access.
    ///
    /// **Permissions:**
    /// - Full read/write/delete access
    /// - Cross-tenant access
    /// - Access to audit logs
    /// - Can grant/revoke permissions
    /// - Can export data
    ///
    /// **Use Cases:**
    /// - System administrators
    /// - DevOps engineers
    /// - Emergency break-glass access
    Admin,
}

impl Role {
    /// Returns whether this role can read data.
    pub fn can_read(&self) -> bool {
        match self {
            Role::Auditor => true, // Audit logs only
            Role::User => true,
            Role::Analyst => true,
            Role::Admin => true,
        }
    }

    /// Returns whether this role can write data.
    pub fn can_write(&self) -> bool {
        match self {
            Role::Auditor => false,
            Role::User => true, // Own data only
            Role::Analyst => false,
            Role::Admin => true,
        }
    }

    /// Returns whether this role can delete data.
    pub fn can_delete(&self) -> bool {
        match self {
            Role::Auditor => false,
            Role::User => false, // Users cannot delete (compliance)
            Role::Analyst => false,
            Role::Admin => true,
        }
    }

    /// Returns whether this role can export data.
    pub fn can_export(&self) -> bool {
        match self {
            Role::Auditor => false,
            Role::User => false,
            Role::Analyst => true, // For BI/reporting
            Role::Admin => true,
        }
    }

    /// Returns whether this role can access audit logs.
    pub fn can_access_audit_logs(&self) -> bool {
        match self {
            Role::Auditor => true,
            Role::User => false,
            Role::Analyst => false,
            Role::Admin => true,
        }
    }

    /// Returns whether this role has cross-tenant access.
    ///
    /// If false, access is restricted to the user's own tenant.
    pub fn has_cross_tenant_access(&self) -> bool {
        match self {
            Role::Auditor => false, // Audit logs are global but not tenant data
            Role::User => false,
            Role::Analyst => true,
            Role::Admin => true,
        }
    }

    /// Returns the restrictiveness level (0 = least restrictive).
    ///
    /// Used for privilege escalation prevention.
    pub fn restrictiveness(&self) -> u8 {
        match self {
            Role::Admin => 0, // Least restrictive
            Role::Analyst => 1,
            Role::User => 2,
            Role::Auditor => 3, // Most restrictive
        }
    }

    /// Returns whether this role can escalate to the target role.
    ///
    /// **Escalation Rules:**
    /// - Cannot escalate to a less restrictive role
    /// - Can only de-escalate (more restrictive)
    ///
    /// # Examples
    ///
    /// ```
    /// use kimberlite_rbac::roles::Role;
    ///
    /// assert!(!Role::User.can_escalate_to(Role::Admin));  // Cannot escalate
    /// assert!(Role::Admin.can_escalate_to(Role::User));    // Can de-escalate
    /// assert!(Role::User.can_escalate_to(Role::User));     // Same role OK
    /// ```
    pub fn can_escalate_to(&self, target: Role) -> bool {
        // Can only escalate to same or more restrictive role
        self.restrictiveness() <= target.restrictiveness()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_role_ordering() {
        // Enum ordering (derived Ord):
        // Auditor < User < Analyst < Admin
        assert!(Role::Auditor < Role::User);
        assert!(Role::User < Role::Analyst);
        assert!(Role::Analyst < Role::Admin);

        // Restrictiveness ordering (via restrictiveness() method):
        // Admin (0) < Analyst (1) < User (2) < Auditor (3)
        assert!(Role::Admin.restrictiveness() < Role::Analyst.restrictiveness());
        assert!(Role::Analyst.restrictiveness() < Role::User.restrictiveness());
        assert!(Role::User.restrictiveness() < Role::Auditor.restrictiveness());
    }

    #[test]
    fn test_role_permissions() {
        // Auditor: read audit logs only
        assert!(Role::Auditor.can_read());
        assert!(!Role::Auditor.can_write());
        assert!(!Role::Auditor.can_delete());
        assert!(!Role::Auditor.can_export());
        assert!(Role::Auditor.can_access_audit_logs());
        assert!(!Role::Auditor.has_cross_tenant_access());

        // User: read/write own data
        assert!(Role::User.can_read());
        assert!(Role::User.can_write());
        assert!(!Role::User.can_delete());
        assert!(!Role::User.can_export());
        assert!(!Role::User.can_access_audit_logs());
        assert!(!Role::User.has_cross_tenant_access());

        // Analyst: cross-tenant read + export
        assert!(Role::Analyst.can_read());
        assert!(!Role::Analyst.can_write());
        assert!(!Role::Analyst.can_delete());
        assert!(Role::Analyst.can_export());
        assert!(!Role::Analyst.can_access_audit_logs());
        assert!(Role::Analyst.has_cross_tenant_access());

        // Admin: full access
        assert!(Role::Admin.can_read());
        assert!(Role::Admin.can_write());
        assert!(Role::Admin.can_delete());
        assert!(Role::Admin.can_export());
        assert!(Role::Admin.can_access_audit_logs());
        assert!(Role::Admin.has_cross_tenant_access());
    }

    #[test]
    fn test_escalation_prevention() {
        // Cannot escalate to less restrictive role
        assert!(!Role::User.can_escalate_to(Role::Admin));
        assert!(!Role::User.can_escalate_to(Role::Analyst));
        assert!(!Role::Auditor.can_escalate_to(Role::User));

        // Can de-escalate to more restrictive role
        assert!(Role::Admin.can_escalate_to(Role::User));
        assert!(Role::Admin.can_escalate_to(Role::Auditor));
        assert!(Role::Analyst.can_escalate_to(Role::User));

        // Same role is allowed
        assert!(Role::User.can_escalate_to(Role::User));
        assert!(Role::Admin.can_escalate_to(Role::Admin));
    }

    #[test]
    fn test_restrictiveness_ordering() {
        assert!(Role::Admin.restrictiveness() < Role::Analyst.restrictiveness());
        assert!(Role::Analyst.restrictiveness() < Role::User.restrictiveness());
        assert!(Role::User.restrictiveness() < Role::Auditor.restrictiveness());
    }
}

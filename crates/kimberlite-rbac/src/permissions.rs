#![allow(clippy::match_same_arms)]
//! Permission types for access control.
//!
//! Defines fine-grained permissions that can be granted or denied.

use serde::{Deserialize, Serialize};

/// Permission that can be granted to a role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Permission {
    /// Read data from streams.
    Read,

    /// Write (append) data to streams.
    Write,

    /// Delete streams or data.
    Delete,

    /// Export data outside the system.
    ///
    /// **Compliance Impact:**
    /// - GDPR Article 20: Right to data portability
    /// - HIPAA: Designated record set export
    /// - Requires additional audit logging
    Export,

    /// Create new streams.
    CreateStream,

    /// Access audit logs.
    ///
    /// **Compliance Impact:**
    /// - HIPAA § 164.312(b): Audit controls
    /// - SOC 2: Change detection
    /// - Restricted to Admin and Auditor roles
    AccessAuditLogs,

    /// Grant or revoke permissions.
    ///
    /// **Security Impact:**
    /// - High-risk permission (can escalate privileges)
    /// - Restricted to Admin role only
    /// - All grants/revocations are audited
    ManagePermissions,
}

impl Permission {
    /// Returns whether this permission is high-risk.
    ///
    /// High-risk permissions require additional scrutiny and audit logging.
    pub fn is_high_risk(&self) -> bool {
        matches!(
            self,
            Permission::Delete | Permission::Export | Permission::ManagePermissions
        )
    }

    /// Returns whether this permission requires audit logging.
    ///
    /// **Compliance:** HIPAA § 164.312(b), SOC 2 CC7.2
    pub fn requires_audit(&self) -> bool {
        match self {
            Permission::Read => true,
            Permission::Write => true,
            Permission::Delete => true,
            Permission::Export => true, // GDPR Article 20
            Permission::CreateStream => true,
            Permission::AccessAuditLogs => true,
            Permission::ManagePermissions => true,
        }
    }

    /// Returns the compliance frameworks that regulate this permission.
    pub fn applicable_frameworks(&self) -> &'static [&'static str] {
        match self {
            Permission::Read | Permission::Write => {
                &["HIPAA", "GDPR", "SOC2", "PCI_DSS", "ISO27001"]
            }
            Permission::Delete => {
                &["GDPR", "HIPAA", "SOC2", "ISO27001"] // Right to erasure
            }
            Permission::Export => {
                &["GDPR", "HIPAA"] // Data portability, designated record set
            }
            Permission::CreateStream => &["SOC2", "ISO27001"],
            Permission::AccessAuditLogs => &["HIPAA", "SOC2", "PCI_DSS", "ISO27001", "FedRAMP"],
            Permission::ManagePermissions => &["SOC2", "ISO27001", "FedRAMP"],
        }
    }
}

/// Set of permissions granted to a role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionSet {
    permissions: Vec<Permission>,
}

impl PermissionSet {
    /// Creates a new permission set.
    pub fn new(permissions: Vec<Permission>) -> Self {
        Self { permissions }
    }

    /// Creates an empty permission set.
    pub fn empty() -> Self {
        Self {
            permissions: Vec::new(),
        }
    }

    /// Returns whether this set contains the given permission.
    pub fn contains(&self, permission: Permission) -> bool {
        self.permissions.contains(&permission)
    }

    /// Adds a permission to the set.
    pub fn grant(&mut self, permission: Permission) {
        if !self.permissions.contains(&permission) {
            self.permissions.push(permission);
        }
    }

    /// Removes a permission from the set.
    pub fn revoke(&mut self, permission: Permission) {
        self.permissions.retain(|p| *p != permission);
    }

    /// Returns all permissions in the set.
    pub fn iter(&self) -> impl Iterator<Item = &Permission> {
        self.permissions.iter()
    }

    /// Returns whether any permission in the set is high-risk.
    pub fn has_high_risk_permission(&self) -> bool {
        self.permissions.iter().any(Permission::is_high_risk)
    }
}

impl Default for PermissionSet {
    fn default() -> Self {
        Self::empty()
    }
}

impl From<Vec<Permission>> for PermissionSet {
    fn from(permissions: Vec<Permission>) -> Self {
        Self::new(permissions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_high_risk() {
        assert!(!Permission::Read.is_high_risk());
        assert!(!Permission::Write.is_high_risk());
        assert!(Permission::Delete.is_high_risk());
        assert!(Permission::Export.is_high_risk());
        assert!(!Permission::CreateStream.is_high_risk());
        assert!(!Permission::AccessAuditLogs.is_high_risk());
        assert!(Permission::ManagePermissions.is_high_risk());
    }

    #[test]
    fn test_permission_audit_required() {
        // All permissions require audit logging
        assert!(Permission::Read.requires_audit());
        assert!(Permission::Write.requires_audit());
        assert!(Permission::Delete.requires_audit());
        assert!(Permission::Export.requires_audit());
        assert!(Permission::CreateStream.requires_audit());
        assert!(Permission::AccessAuditLogs.requires_audit());
        assert!(Permission::ManagePermissions.requires_audit());
    }

    #[test]
    fn test_permission_set_operations() {
        let mut set = PermissionSet::empty();
        assert!(!set.contains(Permission::Read));

        set.grant(Permission::Read);
        assert!(set.contains(Permission::Read));

        set.grant(Permission::Read); // Duplicate grant is no-op
        assert_eq!(set.permissions.len(), 1);

        set.grant(Permission::Write);
        assert!(set.contains(Permission::Write));
        assert_eq!(set.permissions.len(), 2);

        set.revoke(Permission::Read);
        assert!(!set.contains(Permission::Read));
        assert!(set.contains(Permission::Write));
    }

    #[test]
    fn test_permission_set_high_risk() {
        let mut set = PermissionSet::empty();
        assert!(!set.has_high_risk_permission());

        set.grant(Permission::Read);
        assert!(!set.has_high_risk_permission());

        set.grant(Permission::Delete);
        assert!(set.has_high_risk_permission());
    }

    #[test]
    fn test_permission_set_from_vec() {
        let permissions = vec![Permission::Read, Permission::Write];
        let set = PermissionSet::from(permissions);

        assert!(set.contains(Permission::Read));
        assert!(set.contains(Permission::Write));
        assert!(!set.contains(Permission::Delete));
    }
}

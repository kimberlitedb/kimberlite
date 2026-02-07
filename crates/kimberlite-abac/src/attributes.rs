//! Attribute types for ABAC evaluation.
//!
//! Three attribute categories drive access decisions:
//! - **User attributes**: Role, department, clearance level, device, network
//! - **Resource attributes**: Data classification, owner tenant, stream name
//! - **Environment attributes**: Time, business hours, source country

use chrono::{DateTime, Datelike, Timelike, Utc};
use kimberlite_types::DataClass;
use serde::{Deserialize, Serialize};

// ============================================================================
// Device Type
// ============================================================================

/// The type of device making the access request.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeviceType {
    /// Desktop workstation or laptop.
    Desktop,
    /// Mobile phone or tablet.
    Mobile,
    /// Server or automated system.
    Server,
    /// Unknown or unclassified device.
    Unknown,
}

// ============================================================================
// User Attributes
// ============================================================================

/// Attributes describing the user making the access request.
///
/// These are typically populated from the authentication/identity provider
/// at the start of each request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserAttributes {
    /// The user's role (e.g., "admin", "analyst", "user", "auditor").
    pub role: String,
    /// The user's department (e.g., "engineering", "compliance", "finance").
    pub department: String,
    /// Security clearance level: 0 = public, 1 = confidential, 2 = secret, 3 = top secret.
    pub clearance_level: u8,
    /// IP address of the request origin (String to avoid `IpAddr` serde issues).
    pub ip_address: Option<String>,
    /// The type of device making the request.
    pub device_type: DeviceType,
    /// Tenant the user belongs to, if any.
    pub tenant_id: Option<u64>,
}

impl UserAttributes {
    /// Creates a new `UserAttributes` with required fields and sensible defaults.
    ///
    /// Sets `ip_address` to `None`, `device_type` to `Unknown`, and `tenant_id` to `None`.
    pub fn new(role: &str, department: &str, clearance_level: u8) -> Self {
        assert!(
            clearance_level <= 3,
            "clearance_level must be 0..=3, got {clearance_level}"
        );
        Self {
            role: role.to_string(),
            department: department.to_string(),
            clearance_level,
            ip_address: None,
            device_type: DeviceType::Unknown,
            tenant_id: None,
        }
    }

    /// Sets the IP address.
    pub fn with_ip(mut self, ip: &str) -> Self {
        self.ip_address = Some(ip.to_string());
        self
    }

    /// Sets the device type.
    pub fn with_device(mut self, device: DeviceType) -> Self {
        self.device_type = device;
        self
    }

    /// Sets the tenant ID.
    pub fn with_tenant(mut self, tenant_id: u64) -> Self {
        self.tenant_id = Some(tenant_id);
        self
    }
}

// ============================================================================
// Resource Attributes
// ============================================================================

/// Attributes describing the resource being accessed.
///
/// Populated from stream metadata and the data catalog at query time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceAttributes {
    /// The data classification of the resource.
    pub data_class: DataClass,
    /// The tenant that owns this resource.
    pub owner_tenant: u64,
    /// The name of the stream being accessed.
    pub stream_name: String,
    /// Configured retention period in days (for SOX 7yr, HIPAA 6yr, PCI 1yr checks).
    pub retention_days: Option<u32>,
    /// Whether data correction/amendment is enabled for this resource.
    pub correction_allowed: bool,
    /// Whether this resource is under a legal hold (prevents deletion).
    pub legal_hold_active: bool,
    /// Specific fields being requested (for field-level restriction checks).
    pub requested_fields: Option<Vec<String>>,
}

impl ResourceAttributes {
    /// Creates a new `ResourceAttributes` with sensible defaults for compliance fields.
    ///
    /// Sets `retention_days` and `requested_fields` to `None`,
    /// `correction_allowed` and `legal_hold_active` to `false`.
    pub fn new(data_class: DataClass, owner_tenant: u64, stream_name: &str) -> Self {
        Self {
            data_class,
            owner_tenant,
            stream_name: stream_name.to_string(),
            retention_days: None,
            correction_allowed: false,
            legal_hold_active: false,
            requested_fields: None,
        }
    }
}

// ============================================================================
// Environment Attributes
// ============================================================================

/// Attributes describing the environment/context of the access request.
///
/// These are computed at request time from system state and are not
/// user-controlled, making them harder to forge.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvironmentAttributes {
    /// The timestamp of the access request.
    pub timestamp: DateTime<Utc>,
    /// Whether the request falls within business hours (9:00-17:00 UTC, weekdays).
    pub is_business_hours: bool,
    /// ISO 3166-1 alpha-2 country code of the request source (e.g., "US", "DE").
    pub source_country: String,
}

impl EnvironmentAttributes {
    /// Creates `EnvironmentAttributes` from a timestamp, auto-computing business hours.
    ///
    /// Business hours are defined as 09:00-17:00 UTC on weekdays (Mon-Fri).
    /// This is a simplification; production systems should use per-tenant timezone config.
    pub fn from_timestamp(ts: DateTime<Utc>, country: &str) -> Self {
        let hour = ts.hour();
        let weekday = ts.weekday();
        let is_weekday = matches!(
            weekday,
            chrono::Weekday::Mon
                | chrono::Weekday::Tue
                | chrono::Weekday::Wed
                | chrono::Weekday::Thu
                | chrono::Weekday::Fri
        );
        let is_business_hours = is_weekday && (9..17).contains(&hour);

        Self {
            timestamp: ts,
            is_business_hours,
            source_country: country.to_string(),
        }
    }

    /// Creates `EnvironmentAttributes` with explicit values (no auto-computation).
    pub fn new(timestamp: DateTime<Utc>, is_business_hours: bool, source_country: &str) -> Self {
        Self {
            timestamp,
            is_business_hours,
            source_country: source_country.to_string(),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn test_business_hours_weekday_morning() {
        // Wednesday at 10:00 UTC => business hours
        let ts = Utc.with_ymd_and_hms(2025, 1, 8, 10, 0, 0).unwrap();
        let env = EnvironmentAttributes::from_timestamp(ts, "US");
        assert!(
            env.is_business_hours,
            "10:00 UTC on Wednesday should be business hours"
        );
    }

    #[test]
    fn test_business_hours_weekday_evening() {
        // Wednesday at 18:00 UTC => NOT business hours
        let ts = Utc.with_ymd_and_hms(2025, 1, 8, 18, 0, 0).unwrap();
        let env = EnvironmentAttributes::from_timestamp(ts, "US");
        assert!(
            !env.is_business_hours,
            "18:00 UTC on Wednesday should not be business hours"
        );
    }

    #[test]
    fn test_business_hours_weekend() {
        // Saturday at 10:00 UTC => NOT business hours
        let ts = Utc.with_ymd_and_hms(2025, 1, 11, 10, 0, 0).unwrap();
        let env = EnvironmentAttributes::from_timestamp(ts, "US");
        assert!(
            !env.is_business_hours,
            "10:00 UTC on Saturday should not be business hours"
        );
    }

    #[test]
    fn test_business_hours_boundary_start() {
        // Wednesday at 09:00 UTC => business hours (inclusive start)
        let ts = Utc.with_ymd_and_hms(2025, 1, 8, 9, 0, 0).unwrap();
        let env = EnvironmentAttributes::from_timestamp(ts, "US");
        assert!(
            env.is_business_hours,
            "09:00 UTC on Wednesday should be business hours"
        );
    }

    #[test]
    fn test_business_hours_boundary_end() {
        // Wednesday at 17:00 UTC => NOT business hours (exclusive end)
        let ts = Utc.with_ymd_and_hms(2025, 1, 8, 17, 0, 0).unwrap();
        let env = EnvironmentAttributes::from_timestamp(ts, "US");
        assert!(
            !env.is_business_hours,
            "17:00 UTC on Wednesday should not be business hours (exclusive end)"
        );
    }

    #[test]
    fn test_user_attributes_builder() {
        let user = UserAttributes::new("admin", "engineering", 3)
            .with_ip("192.168.1.1")
            .with_device(DeviceType::Desktop)
            .with_tenant(42);

        assert_eq!(user.role, "admin");
        assert_eq!(user.department, "engineering");
        assert_eq!(user.clearance_level, 3);
        assert_eq!(user.ip_address, Some("192.168.1.1".to_string()));
        assert_eq!(user.device_type, DeviceType::Desktop);
        assert_eq!(user.tenant_id, Some(42));
    }

    #[test]
    #[should_panic(expected = "clearance_level must be 0..=3")]
    fn test_user_attributes_invalid_clearance() {
        UserAttributes::new("admin", "engineering", 4);
    }

    #[test]
    fn test_resource_attributes() {
        let resource = ResourceAttributes::new(DataClass::PHI, 1, "patient_records");
        assert_eq!(resource.data_class, DataClass::PHI);
        assert_eq!(resource.owner_tenant, 1);
        assert_eq!(resource.stream_name, "patient_records");
    }
}

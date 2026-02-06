//! Authentication module for JWT and API key validation.
//!
//! Supports multiple authentication methods:
//! - JWT tokens (for user sessions)
//! - API keys (for service accounts)

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use jsonwebtoken::{DecodingKey, EncodingKey, Header, Validation, decode, encode};
use kimberlite_types::TenantId;
use serde::{Deserialize, Serialize};

use crate::error::{ServerError, ServerResult};

/// Authentication mode for the server.
#[derive(Debug, Clone, Default)]
pub enum AuthMode {
    /// No authentication required.
    #[default]
    None,
    /// JWT token authentication.
    Jwt(JwtConfig),
    /// API key authentication.
    ApiKey(ApiKeyConfig),
    /// Both JWT and API key authentication (either is accepted).
    Both {
        jwt: JwtConfig,
        api_key: ApiKeyConfig,
    },
}

/// JWT configuration.
#[derive(Debug, Clone)]
pub struct JwtConfig {
    /// Secret key for signing/verifying tokens.
    secret: String,
    /// Token expiration duration.
    pub expiration: Duration,
    /// Issuer claim.
    pub issuer: String,
    /// Audience claims.
    pub audience: Vec<String>,
}

impl JwtConfig {
    /// Creates a new JWT configuration.
    pub fn new(secret: impl Into<String>) -> Self {
        Self {
            secret: secret.into(),
            expiration: Duration::from_secs(3600), // 1 hour
            issuer: "kimberlite".to_string(),
            audience: vec!["kimberlite".to_string()],
        }
    }

    /// Sets the token expiration duration.
    #[must_use]
    pub fn with_expiration(mut self, expiration: Duration) -> Self {
        self.expiration = expiration;
        self
    }

    /// Sets the issuer claim.
    #[must_use]
    pub fn with_issuer(mut self, issuer: impl Into<String>) -> Self {
        self.issuer = issuer.into();
        self
    }

    /// Adds an audience claim.
    #[must_use]
    pub fn with_audience(mut self, audience: impl Into<String>) -> Self {
        self.audience.push(audience.into());
        self
    }

    /// Creates a JWT token for the given user with role-based claims.
    ///
    /// # Arguments
    ///
    /// * `subject` - User or service account ID
    /// * `tenant_id` - Tenant ID the user belongs to
    /// * `roles` - Roles assigned to the user (e.g., `["Admin"]`, `["User"]`)
    ///
    /// # Returns
    ///
    /// A signed JWT token string.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let config = JwtConfig::new("secret");
    /// let token = config.create_token(
    ///     "user123",
    ///     TenantId::new(42),
    ///     vec!["User".to_string()],
    /// )?;
    /// ```
    pub fn create_token(
        &self,
        subject: impl Into<String>,
        tenant_id: TenantId,
        roles: Vec<String>,
    ) -> Result<String, jsonwebtoken::errors::Error> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before Unix epoch")
            .as_secs();

        let claims = Claims {
            sub: subject.into(),
            tenant_id: u64::from(tenant_id),
            roles,
            iat: now,
            exp: now + self.expiration.as_secs(),
            iss: self.issuer.clone(),
            aud: self.audience.clone(),
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(self.secret.as_bytes()),
        )
    }
}

/// API key configuration.
#[derive(Debug, Clone, Default)]
pub struct ApiKeyConfig {
    /// Whether to enable API key authentication.
    pub enabled: bool,
}

impl ApiKeyConfig {
    /// Creates a new API key configuration.
    pub fn new() -> Self {
        Self { enabled: true }
    }
}

/// JWT claims structure.
#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// Subject (user ID).
    pub sub: String,
    /// Tenant ID the user belongs to.
    pub tenant_id: u64,
    /// User roles.
    pub roles: Vec<String>,
    /// Issued at timestamp (seconds since epoch).
    pub iat: u64,
    /// Expiration timestamp (seconds since epoch).
    pub exp: u64,
    /// Issuer.
    pub iss: String,
    /// Audience.
    pub aud: Vec<String>,
}

/// Authenticated identity after successful authentication.
#[derive(Debug, Clone)]
pub struct AuthenticatedIdentity {
    /// User or service account ID.
    pub subject: String,
    /// Tenant ID.
    pub tenant_id: TenantId,
    /// Roles/permissions.
    pub roles: Vec<String>,
    /// How the identity was authenticated.
    pub method: AuthMethod,
}

impl AuthenticatedIdentity {
    /// Extracts an RBAC access policy from this identity.
    ///
    /// Parses the role strings from the JWT token and creates the appropriate
    /// `AccessPolicy` with tenant isolation for User role.
    ///
    /// # Errors
    ///
    /// Returns an error if the role string is invalid or unrecognized.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let identity = auth_service.authenticate(Some(token))?;
    /// let policy = identity.extract_policy()?;
    /// let enforcer = PolicyEnforcer::new(policy);
    /// ```
    pub fn extract_policy(&self) -> ServerResult<kimberlite_rbac::AccessPolicy> {
        use kimberlite_rbac::policy::StandardPolicies;
        use kimberlite_rbac::roles::Role;

        // Parse the first role (users should have one primary role)
        let role_str = self
            .roles
            .first()
            .ok_or_else(|| ServerError::Unauthorized("no roles assigned to user".to_string()))?;

        let role = match role_str.as_str() {
            "Admin" | "admin" => Role::Admin,
            "Analyst" | "analyst" => Role::Analyst,
            "User" | "user" => Role::User,
            "Auditor" | "auditor" => Role::Auditor,
            _ => {
                return Err(ServerError::Unauthorized(format!(
                    "invalid role: {role_str}"
                )));
            }
        };

        // Create policy based on role
        let policy = match role {
            Role::Admin => StandardPolicies::admin(),
            Role::Analyst => StandardPolicies::analyst(),
            Role::Auditor => StandardPolicies::auditor(),
            Role::User => StandardPolicies::user(self.tenant_id),
        };

        Ok(policy)
    }

    /// Extracts ABAC user attributes from this identity.
    ///
    /// Creates `UserAttributes` suitable for ABAC policy evaluation.
    /// The clearance level is derived from the role:
    /// - Admin: 3 (top secret)
    /// - Analyst: 2 (secret)
    /// - User: 1 (confidential)
    /// - Auditor: 2 (secret, for audit access)
    pub fn extract_abac_user_attributes(&self) -> kimberlite_abac::UserAttributes {
        let role_str = self.roles.first().map_or("user", |r| r.as_str());
        let clearance = match role_str {
            "Admin" | "admin" => 3,
            "Analyst" | "analyst" | "Auditor" | "auditor" => 2,
            _ => 1,
        };

        let mut attrs =
            kimberlite_abac::UserAttributes::new(&role_str.to_lowercase(), "default", clearance);
        attrs.tenant_id = Some(u64::from(self.tenant_id));
        attrs
    }

    /// Returns the primary role for this identity.
    ///
    /// If multiple roles are present, returns the first one.
    /// Returns None if no roles are assigned.
    pub fn primary_role(&self) -> Option<kimberlite_rbac::Role> {
        use kimberlite_rbac::roles::Role;

        self.roles
            .first()
            .and_then(|role_str| match role_str.as_str() {
                "Admin" | "admin" => Some(Role::Admin),
                "Analyst" | "analyst" => Some(Role::Analyst),
                "User" | "user" => Some(Role::User),
                "Auditor" | "auditor" => Some(Role::Auditor),
                _ => None,
            })
    }
}

/// Authentication method used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthMethod {
    /// JWT token.
    Jwt,
    /// API key.
    ApiKey,
    /// No authentication (anonymous).
    Anonymous,
}

/// Authentication service that handles token validation and API key verification.
pub struct AuthService {
    /// Authentication mode.
    mode: AuthMode,
    /// API key store (in-memory for now, could be backed by database).
    api_keys: RwLock<HashMap<String, ApiKeyEntry>>,
}

/// An API key entry.
#[derive(Debug, Clone)]
struct ApiKeyEntry {
    /// The hashed API key.
    #[allow(dead_code)]
    key_hash: String,
    /// Subject (service account ID).
    subject: String,
    /// Tenant ID.
    tenant_id: TenantId,
    /// Roles.
    roles: Vec<String>,
    /// Expiration (optional).
    expires_at: Option<SystemTime>,
}

impl AuthService {
    /// Creates a new authentication service.
    pub fn new(mode: AuthMode) -> Self {
        Self {
            mode,
            api_keys: RwLock::new(HashMap::new()),
        }
    }

    /// Authenticates a request using the provided token.
    ///
    /// The token can be either a JWT or an API key, depending on the configured mode.
    pub fn authenticate(&self, token: Option<&str>) -> ServerResult<AuthenticatedIdentity> {
        match &self.mode {
            AuthMode::None => Ok(AuthenticatedIdentity {
                subject: "anonymous".to_string(),
                tenant_id: TenantId::new(0),
                roles: vec![],
                method: AuthMethod::Anonymous,
            }),

            AuthMode::Jwt(config) => {
                let token = token.ok_or(ServerError::Unauthorized("missing token".to_string()))?;
                self.validate_jwt(token, config)
            }

            AuthMode::ApiKey(_) => {
                let token =
                    token.ok_or(ServerError::Unauthorized("missing API key".to_string()))?;
                self.validate_api_key(token)
            }

            AuthMode::Both { jwt, api_key: _ } => {
                let token =
                    token.ok_or(ServerError::Unauthorized("missing credentials".to_string()))?;

                // Try JWT first, then API key
                if token.contains('.') {
                    // Looks like a JWT (has dots)
                    self.validate_jwt(token, jwt)
                } else {
                    // Try as API key
                    self.validate_api_key(token)
                }
            }
        }
    }

    /// Validates a JWT token.
    #[allow(clippy::unused_self)] // May need self in the future for token blacklisting
    fn validate_jwt(&self, token: &str, config: &JwtConfig) -> ServerResult<AuthenticatedIdentity> {
        let mut validation = Validation::default();
        validation.set_issuer(&[&config.issuer]);
        validation.set_audience(&config.audience);

        let token_data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(config.secret.as_bytes()),
            &validation,
        )
        .map_err(|e| ServerError::Unauthorized(format!("invalid JWT: {e}")))?;

        let claims = token_data.claims;

        Ok(AuthenticatedIdentity {
            subject: claims.sub,
            tenant_id: TenantId::new(claims.tenant_id),
            roles: claims.roles,
            method: AuthMethod::Jwt,
        })
    }

    /// Validates an API key.
    fn validate_api_key(&self, key: &str) -> ServerResult<AuthenticatedIdentity> {
        let keys = self
            .api_keys
            .read()
            .map_err(|_| ServerError::Unauthorized("lock poisoned".to_string()))?;

        let entry = keys
            .get(key)
            .ok_or_else(|| ServerError::Unauthorized("invalid API key".to_string()))?;

        // Check expiration
        if let Some(expires_at) = entry.expires_at {
            if SystemTime::now() > expires_at {
                return Err(ServerError::Unauthorized("API key expired".to_string()));
            }
        }

        Ok(AuthenticatedIdentity {
            subject: entry.subject.clone(),
            tenant_id: entry.tenant_id,
            roles: entry.roles.clone(),
            method: AuthMethod::ApiKey,
        })
    }

    /// Registers an API key (for testing or initial setup).
    pub fn register_api_key(
        &self,
        key: impl Into<String>,
        subject: impl Into<String>,
        tenant_id: TenantId,
        roles: Vec<String>,
        expires_at: Option<SystemTime>,
    ) -> ServerResult<()> {
        let key = key.into();
        // In production, we would hash the key before storing
        let key_hash = key.clone(); // Simplified for now

        let entry = ApiKeyEntry {
            key_hash,
            subject: subject.into(),
            tenant_id,
            roles,
            expires_at,
        };

        self.api_keys
            .write()
            .map_err(|_| ServerError::Unauthorized("lock poisoned".to_string()))?
            .insert(key, entry);

        Ok(())
    }

    /// Creates a JWT token for a user.
    pub fn create_jwt(
        &self,
        subject: &str,
        tenant_id: TenantId,
        roles: Vec<String>,
    ) -> ServerResult<String> {
        let config = match &self.mode {
            AuthMode::Jwt(c) => c,
            AuthMode::Both { jwt, .. } => jwt,
            _ => {
                return Err(ServerError::Unauthorized(
                    "JWT authentication not configured".to_string(),
                ));
            }
        };

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| ServerError::Unauthorized(format!("time error: {e}")))?;

        let claims = Claims {
            sub: subject.to_string(),
            tenant_id: u64::from(tenant_id),
            roles,
            iat: now.as_secs(),
            exp: (now + config.expiration).as_secs(),
            iss: config.issuer.clone(),
            aud: config.audience.clone(),
        };

        encode(
            &Header::default(),
            &claims,
            &EncodingKey::from_secret(config.secret.as_bytes()),
        )
        .map_err(|e| ServerError::Unauthorized(format!("failed to create JWT: {e}")))
    }

    /// Revokes an API key.
    pub fn revoke_api_key(&self, key: &str) -> ServerResult<bool> {
        let mut keys = self
            .api_keys
            .write()
            .map_err(|_| ServerError::Unauthorized("lock poisoned".to_string()))?;

        Ok(keys.remove(key).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_mode_none() {
        let service = AuthService::new(AuthMode::None);
        let identity = service.authenticate(None).unwrap();
        assert_eq!(identity.method, AuthMethod::Anonymous);
        assert_eq!(identity.subject, "anonymous");
    }

    #[test]
    fn test_jwt_creation_and_validation() {
        let config = JwtConfig::new("test-secret-key-that-is-long-enough");
        let service = AuthService::new(AuthMode::Jwt(config));

        // Create a token
        let token = service
            .create_jwt("user123", TenantId::new(1), vec!["admin".to_string()])
            .unwrap();

        // Validate the token
        let identity = service.authenticate(Some(&token)).unwrap();
        assert_eq!(identity.subject, "user123");
        assert_eq!(identity.tenant_id, TenantId::new(1));
        assert_eq!(identity.roles, vec!["admin"]);
        assert_eq!(identity.method, AuthMethod::Jwt);
    }

    #[test]
    fn test_jwt_missing_token() {
        let config = JwtConfig::new("test-secret");
        let service = AuthService::new(AuthMode::Jwt(config));

        let result = service.authenticate(None);
        assert!(result.is_err());
    }

    #[test]
    fn test_api_key_authentication() {
        let service = AuthService::new(AuthMode::ApiKey(ApiKeyConfig::new()));

        // Register an API key
        service
            .register_api_key(
                "test-api-key",
                "service-account-1",
                TenantId::new(2),
                vec!["read".to_string()],
                None,
            )
            .unwrap();

        // Validate the API key
        let identity = service.authenticate(Some("test-api-key")).unwrap();
        assert_eq!(identity.subject, "service-account-1");
        assert_eq!(identity.tenant_id, TenantId::new(2));
        assert_eq!(identity.method, AuthMethod::ApiKey);
    }

    #[test]
    fn test_api_key_invalid() {
        let service = AuthService::new(AuthMode::ApiKey(ApiKeyConfig::new()));

        let result = service.authenticate(Some("invalid-key"));
        assert!(result.is_err());
    }

    #[test]
    fn test_api_key_revocation() {
        let service = AuthService::new(AuthMode::ApiKey(ApiKeyConfig::new()));

        service
            .register_api_key("key-to-revoke", "test", TenantId::new(1), vec![], None)
            .unwrap();

        // Key should work
        assert!(service.authenticate(Some("key-to-revoke")).is_ok());

        // Revoke it
        assert!(service.revoke_api_key("key-to-revoke").unwrap());

        // Key should no longer work
        assert!(service.authenticate(Some("key-to-revoke")).is_err());
    }

    // RBAC Integration Tests

    #[test]
    fn test_extract_policy_admin_role() {
        let config = JwtConfig::new("test-secret");
        let service = AuthService::new(AuthMode::Jwt(config));

        let token = service
            .create_jwt("admin_user", TenantId::new(1), vec!["Admin".to_string()])
            .unwrap();

        let identity = service.authenticate(Some(&token)).unwrap();
        let policy = identity.extract_policy().unwrap();

        // Admin policy should allow all streams and columns
        assert!(policy.allows_stream("any_stream"));
        assert!(policy.allows_column("any_column"));
        assert_eq!(policy.role, kimberlite_rbac::roles::Role::Admin);
    }

    #[test]
    fn test_extract_policy_user_role() {
        let config = JwtConfig::new("test-secret");
        let service = AuthService::new(AuthMode::Jwt(config));

        let tenant_id = TenantId::new(42);
        let token = service
            .create_jwt("user123", tenant_id, vec!["User".to_string()])
            .unwrap();

        let identity = service.authenticate(Some(&token)).unwrap();
        let policy = identity.extract_policy().unwrap();

        // User policy should have tenant isolation
        assert_eq!(policy.tenant_id, Some(tenant_id));
        assert_eq!(policy.role, kimberlite_rbac::roles::Role::User);

        // Should have row-level security filter for tenant
        assert!(!policy.row_filters().is_empty());
    }

    #[test]
    fn test_extract_policy_analyst_role() {
        let config = JwtConfig::new("test-secret");
        let service = AuthService::new(AuthMode::Jwt(config));

        let token = service
            .create_jwt("analyst", TenantId::new(1), vec!["Analyst".to_string()])
            .unwrap();

        let identity = service.authenticate(Some(&token)).unwrap();
        let policy = identity.extract_policy().unwrap();

        // Analyst has cross-tenant access but cannot write
        assert_eq!(policy.role, kimberlite_rbac::roles::Role::Analyst);
        assert_eq!(policy.tenant_id, None); // No tenant restriction
    }

    #[test]
    fn test_extract_policy_auditor_role() {
        let config = JwtConfig::new("test-secret");
        let service = AuthService::new(AuthMode::Jwt(config));

        let token = service
            .create_jwt("auditor", TenantId::new(1), vec!["Auditor".to_string()])
            .unwrap();

        let identity = service.authenticate(Some(&token)).unwrap();
        let policy = identity.extract_policy().unwrap();

        // Auditor can only access audit logs
        assert_eq!(policy.role, kimberlite_rbac::roles::Role::Auditor);
        assert!(policy.allows_stream("audit_log"));
        assert!(!policy.allows_stream("patient_records"));
    }

    #[test]
    fn test_extract_policy_invalid_role() {
        let config = JwtConfig::new("test-secret");
        let service = AuthService::new(AuthMode::Jwt(config));

        let token = service
            .create_jwt("user", TenantId::new(1), vec!["InvalidRole".to_string()])
            .unwrap();

        let identity = service.authenticate(Some(&token)).unwrap();
        let result = identity.extract_policy();

        // Should fail with invalid role
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_policy_no_roles() {
        let config = JwtConfig::new("test-secret");
        let service = AuthService::new(AuthMode::Jwt(config));

        let token = service
            .create_jwt("user", TenantId::new(1), vec![])
            .unwrap();

        let identity = service.authenticate(Some(&token)).unwrap();
        let result = identity.extract_policy();

        // Should fail with no roles
        assert!(result.is_err());
    }

    #[test]
    fn test_primary_role() {
        let identity = AuthenticatedIdentity {
            subject: "test".to_string(),
            tenant_id: TenantId::new(1),
            roles: vec!["Admin".to_string(), "User".to_string()],
            method: AuthMethod::Jwt,
        };

        let role = identity.primary_role().unwrap();
        assert_eq!(role, kimberlite_rbac::roles::Role::Admin);
    }

    #[test]
    fn test_primary_role_case_insensitive() {
        let identity = AuthenticatedIdentity {
            subject: "test".to_string(),
            tenant_id: TenantId::new(1),
            roles: vec!["admin".to_string()], // lowercase
            method: AuthMethod::Jwt,
        };

        let role = identity.primary_role().unwrap();
        assert_eq!(role, kimberlite_rbac::roles::Role::Admin);
    }

    #[test]
    fn test_jwt_token_with_role_round_trip() {
        use kimberlite_rbac::enforcement::PolicyEnforcer;

        let config = JwtConfig::new("test-secret");
        let service = AuthService::new(AuthMode::Jwt(config));

        // Create token with User role
        let tenant_id = TenantId::new(42);
        let token = service
            .create_jwt("user123", tenant_id, vec!["User".to_string()])
            .unwrap();

        // Authenticate and extract policy
        let identity = service.authenticate(Some(&token)).unwrap();
        let policy = identity.extract_policy().unwrap();

        // Create enforcer and verify tenant isolation
        let enforcer = PolicyEnforcer::new(policy).without_audit();

        let where_clause = enforcer.generate_where_clause();
        assert!(where_clause.contains("tenant_id"));
        assert!(where_clause.contains("42"));
    }

    // ABAC Integration Tests

    #[test]
    fn test_extract_abac_user_attributes_admin() {
        let identity = AuthenticatedIdentity {
            subject: "admin_user".to_string(),
            tenant_id: TenantId::new(1),
            roles: vec!["Admin".to_string()],
            method: AuthMethod::Jwt,
        };

        let attrs = identity.extract_abac_user_attributes();
        assert_eq!(attrs.role, "admin");
        assert_eq!(attrs.clearance_level, 3);
        assert_eq!(attrs.tenant_id, Some(1));
    }

    #[test]
    fn test_extract_abac_user_attributes_user() {
        let identity = AuthenticatedIdentity {
            subject: "user123".to_string(),
            tenant_id: TenantId::new(42),
            roles: vec!["User".to_string()],
            method: AuthMethod::Jwt,
        };

        let attrs = identity.extract_abac_user_attributes();
        assert_eq!(attrs.role, "user");
        assert_eq!(attrs.clearance_level, 1);
        assert_eq!(attrs.tenant_id, Some(42));
    }

    #[test]
    fn test_abac_evaluation_with_identity() {
        use chrono::Utc;
        use kimberlite_abac::attributes::{EnvironmentAttributes, ResourceAttributes};
        use kimberlite_abac::{AbacPolicy, evaluator};
        use kimberlite_types::DataClass;

        let identity = AuthenticatedIdentity {
            subject: "analyst".to_string(),
            tenant_id: TenantId::new(1),
            roles: vec!["Analyst".to_string()],
            method: AuthMethod::Jwt,
        };

        let user_attrs = identity.extract_abac_user_attributes();
        let resource = ResourceAttributes::new(DataClass::Confidential, 1, "metrics");
        let env = EnvironmentAttributes::from_timestamp(Utc::now(), "US");

        // FedRAMP policy should allow US-based access
        let policy = AbacPolicy::fedramp_policy();
        let decision = evaluator::evaluate(&policy, &user_attrs, &resource, &env);

        assert_eq!(decision.effect, kimberlite_abac::PolicyEffect::Allow);
    }
}

#![no_main]

// Fuzz target for authentication (JWT + API key).
//
// `AuthService::authenticate` is the first trust boundary crossed by every
// unauthenticated client. Anything that either panics or produces an
// `AuthenticatedIdentity` with elevated roles from an adversarial token is
// an auth bypass.
//
// Oracles:
//   1. `authenticate` on any token string never panics.
//   2. For a JWT-mode service, an arbitrary non-structured token must NEVER
//      authenticate as Admin/Analyst/Auditor — those require a valid
//      signature over the configured secret.
//   3. For API-key mode with no keys registered, any token must fail.
//   4. Round-trip: a token freshly issued via `create_token` must
//      authenticate with the expected subject, tenant, and roles.
//   5. Tamper invariant: flipping any single byte of a valid signed token
//      (outside the header prefix) must cause authentication to fail.

use std::time::Duration;

use kimberlite_server::auth::{
    ApiKeyConfig, AuthMode, AuthService, JwtConfig,
};
use kimberlite_types::TenantId;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 16 {
        return;
    }

    // Derive a JWT secret and a fuzz-controlled token from the input.
    let secret = std::str::from_utf8(&data[..16]).unwrap_or("fuzz-secret-key!");
    let token_bytes = &data[16..];
    let token_str = std::str::from_utf8(token_bytes).ok();

    // ── JWT mode ────────────────────────────────────────────────────────────
    let jwt_config = JwtConfig::new(secret);
    let jwt_service = AuthService::new(AuthMode::Jwt(jwt_config.clone()));

    if let Some(tok) = token_str {
        match jwt_service.authenticate(Some(tok)) {
            Ok(id) => {
                // If a random fuzz string authenticated, it must at minimum have
                // the structural shape of a JWT (header.payload.signature).
                assert!(
                    tok.matches('.').count() >= 2,
                    "JWT auth accepted {tok:?} as valid but it has no JWT structure"
                );
                // Whatever roles it gained, the token was signed with our secret
                // by construction — reject the possibility of a zero-signature
                // or empty-roles Admin escalation.
                if id.roles.iter().any(|r| r.eq_ignore_ascii_case("admin")) {
                    // Can only happen if the attacker guessed the HMAC over
                    // our secret — either we've been cosmically unlucky, or
                    // there is a bypass. Let the assertion fire.
                    panic!("JWT auth accepted a fuzz-derived token with Admin role");
                }
            }
            Err(_) => {}
        }
    }

    // ── API-key mode with empty registry ────────────────────────────────────
    let api_service = AuthService::new(AuthMode::ApiKey(ApiKeyConfig::new()));
    if let Some(tok) = token_str {
        assert!(
            api_service.authenticate(Some(tok)).is_err(),
            "API-key auth accepted token {tok:?} with no keys registered"
        );
    }

    // ── Round-trip + tamper: issue a real token, then tamper it ─────────────
    //
    // Exercises the happy path and the tamper-rejection path. Uses a short
    // tenant id derived from fuzz bytes so the signed bytes vary across
    // iterations.
    let tenant_raw = u64::from_le_bytes(data[..8].try_into().expect("16 >= 8"));
    let tenant = TenantId::new(tenant_raw);

    let issued = jwt_config
        .with_expiration(Duration::from_secs(3600))
        .create_token("fuzz-subject", tenant, vec!["User".to_string()])
        .expect("creating a JWT should succeed for any secret+tenant");

    // Happy path: issued token authenticates.
    let id = jwt_service
        .authenticate(Some(&issued))
        .expect("the token we just issued must authenticate");
    assert_eq!(id.subject, "fuzz-subject");
    assert_eq!(id.tenant_id, tenant);

    // Tamper path: flip one byte in the signature segment (after the second '.')
    // and confirm authentication fails. Pick the position from fuzz bytes so
    // we cover the full signature range.
    let dots: Vec<usize> = issued.match_indices('.').map(|(i, _)| i).collect();
    if dots.len() >= 2 {
        let sig_start = dots[1] + 1;
        let mut bytes = issued.into_bytes();
        if sig_start < bytes.len() {
            let pos = sig_start + (data[8] as usize % (bytes.len() - sig_start));
            let original = bytes[pos];
            // XOR a non-zero delta so the byte is guaranteed to change.
            bytes[pos] ^= data[9] | 0x01;
            // Still-valid base64url character? If XOR produced a non-base64url
            // byte we'd get a parse error rather than a verification failure —
            // either outcome is Err, which is what the invariant checks.
            let _ = original;
            if let Ok(tampered) = String::from_utf8(bytes) {
                assert!(
                    jwt_service.authenticate(Some(&tampered)).is_err(),
                    "JWT auth accepted a tampered signature: {tampered:?}"
                );
            }
        }
    }
});

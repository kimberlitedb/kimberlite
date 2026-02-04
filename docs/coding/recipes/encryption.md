# Encryption

Encrypt sensitive data in Kimberlite applications.

## Overview

Kimberlite provides multiple layers of encryption:

1. **Encryption at rest** - Per-tenant envelope encryption (automatic)
2. **Encryption in transit** - TLS 1.3 (configure once)
3. **Field-level encryption** - Encrypt specific columns (manual)

This recipe focuses on field-level encryption for sensitive data like SSNs, credit cards, or personal notes.

## When to Use Field-Level Encryption

| Data Type | Encryption Needed? | Why |
|-----------|-------------------|-----|
| Patient name | ❌ No | Already encrypted at rest, needs to be searchable |
| Social Security Number | ✅ Yes | Extra protection, not searchable |
| Credit card number | ✅ Yes | PCI DSS requirement |
| Personal notes | ✅ Yes | Highly sensitive, user-controlled |
| Phone number | ❌ No | Already encrypted at rest, needs to be searchable |
| Date of birth | ❌ No | Already encrypted at rest, needs to be queryable |

**Rule of thumb:** Encrypt fields that are:
- Highly sensitive
- Don't need to be searchable/queryable
- Subject to extra regulations (PCI DSS, etc.)

## Basic Field Encryption

### Setup

```rust
use kimberlite_crypto::SymmetricKey;

// Generate a per-field encryption key
let field_key = SymmetricKey::generate();

// Store key securely (KMS, HSM, or environment variable)
std::env::set_var("FIELD_ENCRYPTION_KEY", field_key.to_base64());
```

### Encrypt Before Insert

```rust
use kimberlite::Client;
use kimberlite_crypto::SymmetricKey;

fn insert_patient_with_encrypted_ssn(
    client: &Client,
    name: &str,
    ssn: &str,  // Plaintext SSN
) -> Result<()> {
    // Load encryption key
    let key = SymmetricKey::from_base64(
        &std::env::var("FIELD_ENCRYPTION_KEY")?
    )?;

    // Encrypt SSN
    let encrypted_ssn = key.encrypt(ssn.as_bytes())?;
    let encoded_ssn = base64::encode(&encrypted_ssn);

    // Insert with encrypted SSN
    client.execute(
        "INSERT INTO patients (name, ssn_encrypted) VALUES (?, ?)",
        &[&name, &encoded_ssn],
    )?;

    Ok(())
}
```

### Decrypt on Read

```rust
fn get_patient_ssn(
    client: &Client,
    patient_id: u64,
) -> Result<String> {
    // Load encryption key
    let key = SymmetricKey::from_base64(
        &std::env::var("FIELD_ENCRYPTION_KEY")?
    )?;

    // Query encrypted SSN
    let row = client.query_one(
        "SELECT ssn_encrypted FROM patients WHERE id = ?",
        &[&patient_id],
    )?;

    let encoded_ssn: String = row.get("ssn_encrypted");
    let encrypted_ssn = base64::decode(&encoded_ssn)?;

    // Decrypt
    let plaintext = key.decrypt(&encrypted_ssn)?;
    Ok(String::from_utf8(plaintext)?)
}
```

## Schema Design for Encrypted Fields

```sql
CREATE TABLE patients (
    id BIGINT PRIMARY KEY,
    name TEXT NOT NULL,                    -- Searchable (not encrypted at app level)
    date_of_birth DATE,                    -- Searchable (not encrypted at app level)
    ssn_encrypted TEXT,                    -- Encrypted at app level (not searchable)
    credit_card_encrypted TEXT,            -- Encrypted at app level (not searchable)
    notes_encrypted TEXT                   -- Encrypted at app level (not searchable)
);

-- No index on encrypted fields (they're not searchable)
```

**Key point:** Encrypted fields cannot be queried. Store a hash if you need to verify without decrypting:

```sql
CREATE TABLE patients (
    id BIGINT PRIMARY KEY,
    ssn_encrypted TEXT,    -- Encrypted SSN
    ssn_hash TEXT          -- SHA-256 hash for verification
);

CREATE INDEX patients_ssn_hash_idx ON patients(ssn_hash);
```

## Searchable Encryption (Hash-Based)

To verify an encrypted field without decrypting:

```rust
use sha2::{Sha256, Digest};

fn insert_patient_with_verifiable_ssn(
    client: &Client,
    name: &str,
    ssn: &str,
) -> Result<()> {
    let key = load_encryption_key()?;

    // Encrypt SSN
    let encrypted_ssn = key.encrypt(ssn.as_bytes())?;
    let encoded_ssn = base64::encode(&encrypted_ssn);

    // Hash SSN for verification
    let mut hasher = Sha256::new();
    hasher.update(ssn.as_bytes());
    let ssn_hash = format!("{:x}", hasher.finalize());

    // Insert both
    client.execute(
        "INSERT INTO patients (name, ssn_encrypted, ssn_hash) VALUES (?, ?, ?)",
        &[&name, &encoded_ssn, &ssn_hash],
    )?;

    Ok(())
}

fn verify_ssn(
    client: &Client,
    patient_id: u64,
    ssn_to_verify: &str,
) -> Result<bool> {
    // Hash the SSN to verify
    let mut hasher = Sha256::new();
    hasher.update(ssn_to_verify.as_bytes());
    let ssn_hash = format!("{:x}", hasher.finalize());

    // Check if hash matches
    let row = client.query_one(
        "SELECT ssn_hash FROM patients WHERE id = ?",
        &[&patient_id],
    )?;

    let stored_hash: String = row.get("ssn_hash");
    Ok(stored_hash == ssn_hash)
}
```

## Key Management

### Option 1: Environment Variables (Development)

```bash
# .env (DO NOT commit to git)
FIELD_ENCRYPTION_KEY=base64_encoded_key_here
```

```rust
use std::env;

fn load_encryption_key() -> Result<SymmetricKey> {
    let key_b64 = env::var("FIELD_ENCRYPTION_KEY")
        .map_err(|_| Error::MissingEncryptionKey)?;
    SymmetricKey::from_base64(&key_b64)
}
```

**⚠️ Only for development. Never commit keys to version control.**

### Option 2: AWS KMS (Production)

```rust
use aws_sdk_kms::Client as KmsClient;

async fn load_encryption_key(kms: &KmsClient) -> Result<SymmetricKey> {
    // Decrypt data key using KMS
    let response = kms
        .decrypt()
        .ciphertext_blob(Blob::new(ENCRYPTED_DATA_KEY))
        .send()
        .await?;

    let plaintext = response.plaintext().unwrap();
    SymmetricKey::from_bytes(plaintext.as_ref())
}
```

### Option 3: HashiCorp Vault (Production)

```rust
use vaultrs::{client::VaultClient, kv2};

async fn load_encryption_key(vault: &VaultClient) -> Result<SymmetricKey> {
    // Read key from Vault
    let secret: HashMap<String, String> = kv2::read(vault, "secret/data", "field_encryption_key").await?;

    let key_b64 = secret.get("key").ok_or(Error::MissingKey)?;
    SymmetricKey::from_base64(key_b64)
}
```

## Key Rotation

Rotate encryption keys periodically:

```rust
fn rotate_field_encryption_key(
    client: &Client,
    table: &str,
    column: &str,
    old_key: &SymmetricKey,
    new_key: &SymmetricKey,
) -> Result<()> {
    // Fetch all encrypted values
    let rows = client.query(
        &format!("SELECT id, {} FROM {}", column, table),
        &[],
    )?;

    for row in rows {
        let id: u64 = row.get("id");
        let encrypted_old: String = row.get(column);

        // Decrypt with old key
        let encrypted_bytes = base64::decode(&encrypted_old)?;
        let plaintext = old_key.decrypt(&encrypted_bytes)?;

        // Re-encrypt with new key
        let encrypted_new = new_key.encrypt(&plaintext)?;
        let encoded_new = base64::encode(&encrypted_new);

        // Update
        client.execute(
            &format!("UPDATE {} SET {} = ? WHERE id = ?", table, column),
            &[&encoded_new, &id],
        )?;
    }

    Ok(())
}
```

**⚠️ Run during maintenance window. Large tables may take hours.**

## Helper: Encryption Wrapper

Encapsulate encryption logic:

```rust
use kimberlite_crypto::SymmetricKey;
use base64;

pub struct EncryptedField {
    key: SymmetricKey,
}

impl EncryptedField {
    pub fn new(key: SymmetricKey) -> Self {
        Self { key }
    }

    /// Encrypt plaintext, return base64-encoded ciphertext
    pub fn encrypt(&self, plaintext: &str) -> Result<String> {
        let ciphertext = self.key.encrypt(plaintext.as_bytes())?;
        Ok(base64::encode(&ciphertext))
    }

    /// Decrypt base64-encoded ciphertext, return plaintext
    pub fn decrypt(&self, encoded: &str) -> Result<String> {
        let ciphertext = base64::decode(encoded)?;
        let plaintext = self.key.decrypt(&ciphertext)?;
        Ok(String::from_utf8(plaintext)?)
    }

    /// Hash for verification (can query without decrypting)
    pub fn hash(&self, plaintext: &str) -> String {
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(plaintext.as_bytes());
        format!("{:x}", hasher.finalize())
    }
}
```

**Usage:**

```rust
let field = EncryptedField::new(load_encryption_key()?);

// Insert
let encrypted_ssn = field.encrypt("123-45-6789")?;
client.execute("INSERT INTO patients (ssn_encrypted) VALUES (?)", &[&encrypted_ssn])?;

// Query
let row = client.query_one("SELECT ssn_encrypted FROM patients WHERE id = 1", &[])?;
let encrypted: String = row.get("ssn_encrypted");
let plaintext_ssn = field.decrypt(&encrypted)?;
```

## Performance Considerations

**Encryption overhead:**
- AES-256-GCM: ~2 GB/s (hardware-accelerated)
- Per-field overhead: <1ms for typical field sizes

**Best practices:**
- Encrypt/decrypt only when needed (not on every query)
- Cache decrypted values in memory (with TTL)
- Use connection pooling to amortize key loading

## Compliance

### HIPAA

- ✅ Encryption at rest (per-tenant, automatic)
- ✅ Encryption in transit (TLS 1.3)
- ✅ Field-level encryption (manual, for extra-sensitive data)

### PCI DSS

Requires encryption for:
- Credit card numbers (PAN)
- CVV codes
- Track data

```rust
// PCI DSS: Encrypt credit card data
let field = EncryptedField::new(load_encryption_key()?);

let encrypted_pan = field.encrypt("4111111111111111")?;
let encrypted_cvv = field.encrypt("123")?;

client.execute(
    "INSERT INTO payment_methods (pan_encrypted, cvv_encrypted) VALUES (?, ?)",
    &[&encrypted_pan, &encrypted_cvv],
)?;
```

**⚠️ Never log decrypted credit card data.**

## Testing

```rust
#[test]
fn test_field_encryption_round_trip() {
    let key = SymmetricKey::generate();
    let field = EncryptedField::new(key);

    let plaintext = "123-45-6789";
    let encrypted = field.encrypt(plaintext).unwrap();
    let decrypted = field.decrypt(&encrypted).unwrap();

    assert_eq!(plaintext, decrypted);
}

#[test]
fn test_hash_verification() {
    let key = SymmetricKey::generate();
    let field = EncryptedField::new(key);

    let ssn = "123-45-6789";
    let hash1 = field.hash(ssn);
    let hash2 = field.hash(ssn);

    // Same input produces same hash
    assert_eq!(hash1, hash2);

    // Different input produces different hash
    let hash3 = field.hash("987-65-4321");
    assert_ne!(hash1, hash3);
}
```

## Related Documentation

- **[Cryptography](../../internals/architecture/crypto.md)** - Encryption algorithms
- **[Compliance](../../concepts/compliance.md)** - Encryption requirements
- **[Multi-tenancy](../../concepts/multitenancy.md)** - Per-tenant encryption

---

**Key Takeaway:** Use field-level encryption for highly sensitive data that doesn't need to be searchable. Kimberlite's per-tenant encryption protects all data at rest, but field-level encryption adds defense-in-depth.

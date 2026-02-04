# Cryptography

Kimberlite's cryptographic primitives and their usage patterns.

## Dual-Hash Strategy

Kimberlite uses two hash algorithms for different purposes:

| Type | Algorithm | Purpose | Performance | FIPS |
|------|-----------|---------|-------------|------|
| `ChainHash` | SHA-256 | Compliance paths, audit trails, exports | 500 MB/s | ✅ FIPS 180-4 |
| `InternalHash` | BLAKE3 | Content addressing, Merkle trees, dedup | 5+ GB/s | ❌ Not approved |

## When to Use Each Hash

```rust
match purpose {
    HashPurpose::Compliance => SHA-256,  // Audit trails, exports, proofs
    HashPurpose::Internal => BLAKE3,     // Dedup, Merkle trees, fingerprints
}
```

**Selection by Use Case:**

### Use SHA-256 (ChainHash)
- Log hash chains (tamper-evident audit trail)
- Checkpoint hashes (compliance exports)
- Digital signatures (signing audit trails)
- External verification (regulators need FIPS-approved)
- Cross-tenant data sharing proofs

### Use BLAKE3 (InternalHash)
- Content addressing (deduplication)
- Merkle tree construction (internal indexes)
- Cache keys (content-addressed storage)
- Internal integrity checks (not externally verified)

## SHA-256 Implementation

```rust
use sha2::{Sha256, Digest};

pub fn chain_hash(data: &[u8]) -> ChainHash {
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    ChainHash::from_bytes(result.into())
}
```

**Properties:**
- **Algorithm:** SHA-256 (FIPS 180-4)
- **Output:** 32 bytes (256 bits)
- **Performance:** ~500 MB/s on single core
- **Security:** Collision-resistant (2^128 operations)
- **Use case:** Compliance-critical paths

## BLAKE3 Implementation

```rust
use blake3;

pub fn internal_hash(data: &[u8]) -> InternalHash {
    let hash = blake3::hash(data);
    InternalHash::from_bytes(hash.as_bytes())
}

// For large data, use parallel hashing
pub fn internal_hash_large(data: &[u8]) -> InternalHash {
    let mut hasher = blake3::Hasher::new();
    hasher.update_rayon(data);  // Parallel hashing
    let hash = hasher.finalize();
    InternalHash::from_bytes(hash.as_bytes())
}
```

**Properties:**
- **Algorithm:** BLAKE3
- **Output:** 32 bytes (256 bits)
- **Performance:** 5-15 GB/s (single core), 15+ GB/s (parallel)
- **Security:** Collision-resistant (2^128 operations)
- **Use case:** Internal hot paths

## Encryption

### Symmetric Encryption (AES-256-GCM)

Used for encrypting data at rest:

```rust
use aes_gcm::{Aes256Gcm, Key, Nonce};
use aes_gcm::aead::{Aead, NewAead};

pub struct SymmetricKey {
    key: Key<Aes256Gcm>,
}

impl SymmetricKey {
    /// Generate a new random key
    pub fn generate() -> Self {
        let mut key_bytes = [0u8; 32];
        getrandom::getrandom(&mut key_bytes).expect("RNG failure");
        Self {
            key: Key::from_slice(&key_bytes).clone(),
        }
    }

    /// Encrypt data with authenticated encryption
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let cipher = Aes256Gcm::new(&self.key);
        let nonce = Nonce::from_slice(&self.generate_nonce());

        let ciphertext = cipher.encrypt(nonce, plaintext)
            .map_err(|_| Error::EncryptionFailed)?;

        // Prepend nonce to ciphertext
        let mut result = nonce.to_vec();
        result.extend_from_slice(&ciphertext);
        Ok(result)
    }

    /// Decrypt data and verify authenticity
    pub fn decrypt(&self, ciphertext: &[u8]) -> Result<Vec<u8>> {
        if ciphertext.len() < 12 {
            return Err(Error::InvalidCiphertext);
        }

        let (nonce, ciphertext) = ciphertext.split_at(12);
        let cipher = Aes256Gcm::new(&self.key);
        let nonce = Nonce::from_slice(nonce);

        cipher.decrypt(nonce, ciphertext)
            .map_err(|_| Error::DecryptionFailed)
    }

    fn generate_nonce(&self) -> [u8; 12] {
        let mut nonce = [0u8; 12];
        getrandom::getrandom(&mut nonce).expect("RNG failure");
        nonce
    }
}
```

**Properties:**
- **Algorithm:** AES-256-GCM
- **Key size:** 256 bits (32 bytes)
- **Nonce size:** 96 bits (12 bytes)
- **Authentication:** Built-in (AEAD)
- **Performance:** Hardware-accelerated (Intel AES-NI)

### Alternative: ChaCha20-Poly1305

For platforms without AES-NI:

```rust
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use chacha20poly1305::aead::{Aead, NewAead};

pub struct ChaChaKey {
    key: Key,
}

impl ChaChaKey {
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<Vec<u8>> {
        let cipher = ChaCha20Poly1305::new(&self.key);
        let nonce = Nonce::from_slice(&self.generate_nonce());

        let ciphertext = cipher.encrypt(nonce, plaintext)
            .map_err(|_| Error::EncryptionFailed)?;

        let mut result = nonce.to_vec();
        result.extend_from_slice(&ciphertext);
        Ok(result)
    }
}
```

**Use ChaCha20-Poly1305 when:**
- No AES-NI hardware acceleration
- ARM or RISC-V platforms
- Mobile devices

## Digital Signatures (Ed25519)

Used for signing audit trails and non-repudiation:

```rust
use ed25519_dalek::{Keypair, PublicKey, Signature, Signer, Verifier};

pub struct SigningKey {
    keypair: Keypair,
}

impl SigningKey {
    /// Generate a new signing key
    pub fn generate() -> Self {
        let mut csprng = rand::thread_rng();
        let keypair = Keypair::generate(&mut csprng);
        Self { keypair }
    }

    /// Sign data
    pub fn sign(&self, message: &[u8]) -> Signature {
        self.keypair.sign(message)
    }

    /// Get public key
    pub fn public_key(&self) -> PublicKey {
        self.keypair.public
    }
}

pub struct VerifyingKey {
    public_key: PublicKey,
}

impl VerifyingKey {
    /// Verify signature
    pub fn verify(&self, message: &[u8], signature: &Signature) -> Result<()> {
        self.public_key.verify(message, signature)
            .map_err(|_| Error::InvalidSignature)
    }
}
```

**Properties:**
- **Algorithm:** Ed25519 (EdDSA)
- **Key size:** 32 bytes (public and private)
- **Signature size:** 64 bytes
- **Performance:** ~50k signs/sec, ~25k verifies/sec
- **Use case:** Audit trail signing, non-repudiation

## Key Hierarchy (Envelope Encryption)

```
┌─────────────┐
│ Master Key  │  Stored in HSM/KMS
└──────┬──────┘
       │
       ▼
┌─────────────┐ ┌─────────────┐ ┌─────────────┐
│  KEK_A      │ │  KEK_B      │ │  KEK_C      │  Per-tenant
│  (wrapped)  │ │  (wrapped)  │ │  (wrapped)  │  Key Encryption Keys
└──────┬──────┘ └──────┬──────┘ └──────┬──────┘
       │               │               │
       ▼               ▼               ▼
┌─────────────┐ ┌─────────────┐ ┌─────────────┐
│  DEK_A      │ │  DEK_B      │ │  DEK_C      │  Per-tenant
│  (encrypts) │ │  (encrypts) │ │  (encrypts) │  Data Encryption Keys
└─────────────┘ └─────────────┘ └─────────────┘
```

### Master Key

Stored in HSM or KMS (AWS KMS, HashiCorp Vault, etc.):

```rust
pub trait KeyManagementService {
    /// Generate a new master key
    fn generate_master_key(&self) -> Result<MasterKeyId>;

    /// Wrap (encrypt) a key with the master key
    fn wrap_key(&self, master_key_id: MasterKeyId, key: &[u8]) -> Result<Vec<u8>>;

    /// Unwrap (decrypt) a wrapped key
    fn unwrap_key(&self, master_key_id: MasterKeyId, wrapped: &[u8]) -> Result<Vec<u8>>;
}
```

### KEK (Key Encryption Key)

Per-tenant key that encrypts the DEK:

```rust
pub struct KeyEncryptionKey {
    wrapped: Vec<u8>,  // Encrypted by master key
    master_key_id: MasterKeyId,
}

impl KeyEncryptionKey {
    /// Generate new KEK for a tenant
    pub fn generate(kms: &dyn KeyManagementService) -> Result<Self> {
        let master_key_id = kms.generate_master_key()?;
        let kek_bytes = SymmetricKey::generate().as_bytes();
        let wrapped = kms.wrap_key(master_key_id, kek_bytes)?;

        Ok(Self {
            wrapped,
            master_key_id,
        })
    }

    /// Unwrap to get the actual KEK
    pub fn unwrap(&self, kms: &dyn KeyManagementService) -> Result<SymmetricKey> {
        let kek_bytes = kms.unwrap_key(self.master_key_id, &self.wrapped)?;
        Ok(SymmetricKey::from_bytes(&kek_bytes))
    }
}
```

### DEK (Data Encryption Key)

Per-tenant key that encrypts actual data:

```rust
pub struct DataEncryptionKey {
    wrapped: Vec<u8>,  // Encrypted by KEK
}

impl DataEncryptionKey {
    /// Generate new DEK for a tenant
    pub fn generate(kek: &SymmetricKey) -> Result<Self> {
        let dek = SymmetricKey::generate();
        let wrapped = kek.encrypt(dek.as_bytes())?;
        Ok(Self { wrapped })
    }

    /// Unwrap to get the actual DEK
    pub fn unwrap(&self, kek: &SymmetricKey) -> Result<SymmetricKey> {
        let dek_bytes = kek.decrypt(&self.wrapped)?;
        Ok(SymmetricKey::from_bytes(&dek_bytes))
    }

    /// Encrypt data
    pub fn encrypt(&self, kek: &SymmetricKey, plaintext: &[u8]) -> Result<Vec<u8>> {
        let dek = self.unwrap(kek)?;
        dek.encrypt(plaintext)
    }

    /// Decrypt data
    pub fn decrypt(&self, kek: &SymmetricKey, ciphertext: &[u8]) -> Result<Vec<u8>> {
        let dek = self.unwrap(kek)?;
        dek.decrypt(ciphertext)
    }
}
```

## Key Rotation

Rotate KEK without re-encrypting all data:

```rust
pub fn rotate_kek(
    tenant_id: TenantId,
    old_kek: &KeyEncryptionKey,
    kms: &dyn KeyManagementService,
) -> Result<KeyEncryptionKey> {
    // 1. Generate new KEK
    let new_kek = KeyEncryptionKey::generate(kms)?;

    // 2. Unwrap old KEK
    let old_kek_key = old_kek.unwrap(kms)?;

    // 3. Unwrap DEK with old KEK
    let dek_wrapped = get_tenant_dek(tenant_id)?;
    let dek = dek_wrapped.unwrap(&old_kek_key)?;

    // 4. Re-wrap DEK with new KEK
    let new_kek_key = new_kek.unwrap(kms)?;
    let new_dek_wrapped = DataEncryptionKey::generate(&new_kek_key)?;

    // 5. Save new wrapped DEK
    store_tenant_dek(tenant_id, new_dek_wrapped)?;

    Ok(new_kek)
}
```

## Zeroization

Keys are zeroized when dropped:

```rust
use zeroize::{Zeroize, ZeroizeOnDrop};

#[derive(Zeroize, ZeroizeOnDrop)]
pub struct SymmetricKey {
    #[zeroize(skip)]
    key: Key<Aes256Gcm>,
}

impl Drop for SymmetricKey {
    fn drop(&mut self) {
        // Zeroize key material
        self.zeroize();
    }
}
```

This prevents keys from leaking through:
- Memory dumps
- Core dumps
- Swap files
- Debuggers

## Constant-Time Operations

Use constant-time comparisons for secrets:

```rust
use subtle::ConstantTimeEq;

pub fn verify_mac(expected: &[u8], actual: &[u8]) -> bool {
    // Constant-time comparison (prevents timing attacks)
    expected.ct_eq(actual).into()
}
```

**Never use `==` for secret comparison** (vulnerable to timing attacks).

## Random Number Generation

Use cryptographically secure RNG:

```rust
use rand::RngCore;

pub fn generate_nonce() -> [u8; 12] {
    let mut nonce = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut nonce);
    nonce
}

// Or use getrandom directly (lower-level)
pub fn generate_key() -> [u8; 32] {
    let mut key = [0u8; 32];
    getrandom::getrandom(&mut key).expect("RNG failure");
    key
}
```

**Never use:**
- `rand::random()` without `thread_rng()` (may not be cryptographically secure)
- Timestamps as nonces (predictable)
- Sequential counters as nonces (predictable)

## Performance Benchmarks

On Intel i9 (single core):

| Operation | Throughput | Latency |
|-----------|-----------|---------|
| SHA-256 hash | 500 MB/s | ~2 µs/KB |
| BLAKE3 hash | 5 GB/s | ~0.2 µs/KB |
| BLAKE3 parallel | 15 GB/s | ~0.07 µs/KB |
| AES-256-GCM encrypt | 2 GB/s | ~0.5 µs/KB |
| AES-256-GCM decrypt | 2 GB/s | ~0.5 µs/KB |
| Ed25519 sign | 50k/sec | ~20 µs |
| Ed25519 verify | 25k/sec | ~40 µs |

## Security Considerations

### All-Zero Detection

Detect all-zero keys (security vulnerability):

```rust
impl SymmetricKey {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        // Production assertion (not debug-only)
        assert!(
            !bytes.iter().all(|&b| b == 0),
            "All-zero keys are not allowed (security violation)"
        );

        Ok(Self {
            key: Key::from_slice(bytes).clone(),
        })
    }
}
```

### Key Stretching (Future)

For password-derived keys, use Argon2:

```rust
// Planned for v0.6.0
pub fn derive_key_from_password(password: &str, salt: &[u8]) -> Result<SymmetricKey> {
    let config = argon2::Config::default();
    let hash = argon2::hash_raw(password.as_bytes(), salt, &config)?;
    SymmetricKey::from_bytes(&hash)
}
```

## Testing

Cryptography is tested exhaustively:

- **Unit tests:** Each primitive (hash, encrypt, sign)
- **Property tests:** Round-trip encryption/decryption
- **Known-answer tests:** NIST test vectors for SHA-256, AES-GCM
- **VOPR scenarios:** Byzantine attacks test cryptographic validation

See [Testing Overview](../testing/overview.md).

## Compliance

Kimberlite's cryptography supports compliance:

| Framework | Requirement | Kimberlite Support |
|-----------|-------------|-------------------|
| HIPAA | Encryption at rest | ✅ AES-256-GCM per-tenant |
| HIPAA | Encryption in transit | ✅ TLS 1.3 |
| FIPS 140-2 | Approved algorithms | ✅ SHA-256, AES-256 |
| SOC 2 | Key management | ✅ Envelope encryption + KMS |
| GDPR | Right to erasure | ✅ Delete KEK → data unrecoverable |

## Related Documentation

- **[Multi-tenancy](../../concepts/multitenancy.md)** - Per-tenant encryption
- **[Compliance](../../concepts/compliance.md)** - Cryptographic guarantees
- **[Data Model](../../concepts/data-model.md)** - Hash chains in the log

---

**Key Takeaway:** Kimberlite uses a dual-hash strategy (SHA-256 for compliance, BLAKE3 for performance) and envelope encryption (master key → KEK → DEK) for per-tenant cryptographic isolation.

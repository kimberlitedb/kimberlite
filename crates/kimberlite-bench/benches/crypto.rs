//! Cryptographic operations benchmarks.
//!
//! Benchmarks encryption, hashing, and signing operations to establish
//! performance baselines for cryptographic primitives.

use std::hint::black_box;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use kimberlite_crypto::encryption::{Nonce, decrypt, encrypt};
use kimberlite_crypto::{
    ChainHash, EncryptionKey, FieldKey, SigningKey, chain_hash, decrypt_field, encrypt_field,
    internal_hash,
};

// ============================================================================
// Hash Benchmarks
// ============================================================================

fn bench_blake3_hash(c: &mut Criterion) {
    let mut group = c.benchmark_group("blake3_hash");

    for size in [64, 256, 1024, 4096, 16384] {
        group.throughput(Throughput::Bytes(size as u64));
        let data = vec![0u8; size];

        group.bench_with_input(BenchmarkId::from_parameter(size), &data, |b, data| {
            b.iter(|| {
                let hash = internal_hash(black_box(data));
                black_box(hash);
            });
        });
    }

    group.finish();
}

fn bench_chain_hash(c: &mut Criterion) {
    let mut group = c.benchmark_group("chain_hash");

    let prev_hash = ChainHash::from_bytes(&[0u8; 32]);
    let data = vec![0u8; 1024];

    group.bench_function("chain_hash_1kb", |b| {
        b.iter(|| {
            let hash = chain_hash(black_box(Some(&prev_hash)), black_box(&data));
            black_box(hash);
        });
    });

    group.finish();
}

// ============================================================================
// Encryption Benchmarks
// ============================================================================

fn bench_aes_gcm_encrypt(c: &mut Criterion) {
    let mut group = c.benchmark_group("aes_gcm_encrypt");

    let key = EncryptionKey::generate();
    let nonce = Nonce::from_position(42);

    for size in [64, 256, 1024, 4096, 16384, 65536] {
        group.throughput(Throughput::Bytes(size as u64));
        let plaintext = vec![0u8; size];

        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &plaintext,
            |b, plaintext| {
                b.iter(|| {
                    let ciphertext =
                        encrypt(black_box(&key), black_box(&nonce), black_box(plaintext));
                    black_box(ciphertext);
                });
            },
        );
    }

    group.finish();
}

fn bench_aes_gcm_decrypt(c: &mut Criterion) {
    let mut group = c.benchmark_group("aes_gcm_decrypt");

    let key = EncryptionKey::generate();
    let nonce = Nonce::from_position(42);

    for size in [64, 256, 1024, 4096, 16384, 65536] {
        group.throughput(Throughput::Bytes(size as u64));
        let plaintext = vec![0u8; size];
        let ciphertext = encrypt(&key, &nonce, &plaintext);

        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &ciphertext,
            |b, ciphertext| {
                b.iter(|| {
                    let plaintext =
                        decrypt(black_box(&key), black_box(&nonce), black_box(ciphertext))
                            .expect("valid ciphertext");
                    black_box(plaintext);
                });
            },
        );
    }

    group.finish();
}

fn bench_field_encryption(c: &mut Criterion) {
    let mut group = c.benchmark_group("field_encryption");

    let parent_key = EncryptionKey::generate();
    let key = FieldKey::derive(&parent_key, "test_field");

    for size in [16, 64, 256, 1024] {
        group.throughput(Throughput::Bytes(size as u64));
        let plaintext = vec![0u8; size];

        group.bench_with_input(
            BenchmarkId::new("encrypt", size),
            &plaintext,
            |b, plaintext| {
                b.iter(|| {
                    let ciphertext = encrypt_field(black_box(&key), black_box(plaintext));
                    black_box(ciphertext);
                });
            },
        );
    }

    // Decrypt benchmarks
    for size in [16, 64, 256, 1024] {
        let plaintext = vec![0u8; size];
        let ciphertext = encrypt_field(&key, &plaintext);

        group.bench_with_input(
            BenchmarkId::new("decrypt", size),
            &ciphertext,
            |b, ciphertext| {
                b.iter(|| {
                    let plaintext =
                        decrypt_field(black_box(&key), black_box(ciphertext)).expect("valid");
                    black_box(plaintext);
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Signing Benchmarks
// ============================================================================

fn bench_ed25519_sign(c: &mut Criterion) {
    let mut group = c.benchmark_group("ed25519_sign");

    let signing_key = SigningKey::generate();

    for size in [64, 256, 1024, 4096] {
        group.throughput(Throughput::Bytes(size as u64));
        let message = vec![0u8; size];

        group.bench_with_input(BenchmarkId::from_parameter(size), &message, |b, message| {
            b.iter(|| {
                let signature = signing_key.sign(black_box(message));
                black_box(signature);
            });
        });
    }

    group.finish();
}

fn bench_ed25519_verify(c: &mut Criterion) {
    let mut group = c.benchmark_group("ed25519_verify");

    let signing_key = SigningKey::generate();
    let verifying_key = signing_key.verifying_key();

    for size in [64, 256, 1024, 4096] {
        let message = vec![0u8; size];
        let signature = signing_key.sign(&message);

        group.bench_with_input(
            BenchmarkId::from_parameter(size),
            &(message, signature),
            |b, (message, signature)| {
                b.iter(|| {
                    let valid = verifying_key.verify(black_box(message), black_box(signature));
                    let _ = black_box(valid);
                });
            },
        );
    }

    group.finish();
}

// ============================================================================
// Key Generation Benchmarks
// ============================================================================

fn bench_key_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("key_generation");

    group.bench_function("encryption_key", |b| {
        b.iter(|| {
            let key = EncryptionKey::generate();
            black_box(key);
        });
    });

    group.bench_function("field_key", |b| {
        b.iter(|| {
            let parent_key = EncryptionKey::generate();
            let key = FieldKey::derive(&parent_key, "bench_field");
            black_box(key);
        });
    });

    group.bench_function("signing_key", |b| {
        b.iter(|| {
            let key = SigningKey::generate();
            black_box(key);
        });
    });

    group.finish();
}

// ============================================================================
// Criterion Configuration
// ============================================================================

criterion_group!(
    crypto_benches,
    bench_blake3_hash,
    bench_chain_hash,
    bench_aes_gcm_encrypt,
    bench_aes_gcm_decrypt,
    bench_field_encryption,
    bench_ed25519_sign,
    bench_ed25519_verify,
    bench_key_generation
);

criterion_main!(crypto_benches);

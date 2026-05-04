---
title: "Verifying releases"
section: "operating"
slug: "verifying-releases"
order: 12
---

# Verifying Kimberlite releases

Every Kimberlite release ships two independent supply-chain
attestations:

1. **GPG-signed git tag** — verifiable from any clone with the
   public release-signing key.
2. **Sigstore cosign attestation on release artifacts** — verifiable
   against the GitHub OIDC issuer without prior key material.

Use both. They have different blast radius: the GPG tag certifies
"the maintainer who signed this is the same one whose key is
pinned"; the cosign attestation certifies "this binary was built
by the project's GitHub Actions runner from the source at the tagged
commit".

## Verifying the git tag (GPG)

The release-signing public key is at the repo root:

```
keys/release-signing.asc
```

It's also mirrored to `keys.openpgp.org`. The fingerprint is
pinned in [`SECURITY.md`](../../SECURITY.md).

```bash
# Import the public key (one-time per machine):
curl -O https://raw.githubusercontent.com/kimberlitedb/kimberlite/main/keys/release-signing.asc
gpg --import keys/release-signing.asc

# Verify the tag:
git tag -v v0.7.0
```

A passing verification looks like:

```
object 0123abcd...
type commit
tag v0.7.0
...
gpg: Signature made <date>
gpg:                using <ALGO> key <FINGERPRINT>
gpg: Good signature from "Kimberlite Release Signing <release@kimberlitedb.dev>"
```

A failing verification (wrong key, tampered tag, or tag re-signed
locally) prints `BAD signature` or `Can't check signature`.

## Verifying release binaries (cosign)

Each release publishes platform binaries with a sibling cosign
attestation file. Verify with:

```bash
cosign verify-blob \
    --certificate-identity-regexp "https://github.com/kimberlitedb/kimberlite" \
    --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
    --signature kimberlite-x86_64-unknown-linux-gnu.tar.gz.sig \
    --certificate kimberlite-x86_64-unknown-linux-gnu.tar.gz.cert \
    kimberlite-x86_64-unknown-linux-gnu.tar.gz
```

Cosign checks that:

- The `.sig` was produced by a GitHub Actions OIDC token
- That token was issued for the `kimberlitedb/kimberlite` repo
- The Rekor transparency log entry matches

Both checks must pass before you trust the artifact.

## What we don't sign

- **The npm `@kimberlitedb/client` package.** The npm registry
  enforces its own signing chain; we publish via OIDC trusted
  publishers. The package's `dist/` is reproducibly built from the
  tagged commit.
- **The PyPI `kimberlite` package.** Same pattern via PyPI Trusted
  Publishers.
- **Docker images.** Use `cosign verify` against the registry-
  embedded signature; container scan tools (Trivy, Grype) consume
  this directly.

## Reporting verification failures

If `git tag -v` or `cosign verify` fails on a release we published,
treat it as a security incident. See [`SECURITY.md`](../../SECURITY.md)
for the disclosure path.

AUDIT-2026-05 T1 — closes the v0.7.0 ROADMAP infrastructure item
"GPG-signed release tags by default".

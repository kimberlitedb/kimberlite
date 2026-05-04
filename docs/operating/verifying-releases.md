---
title: "Verifying releases"
section: "operating"
slug: "verifying-releases"
order: 12
---

# Verifying Kimberlite releases

Every Kimberlite release ships two independent supply-chain
attestations:

1. **Signed git tag** — verifiable from any clone with the public
   release-signing key. v0.7.0 ships SSH signatures (Ed25519);
   the org-managed GPG key path is documented but not yet
   provisioned.
2. **Sigstore cosign attestation on release artifacts** — verifiable
   against the GitHub OIDC issuer without prior key material.

Use both. They have different blast radius: the tag signature
certifies "the maintainer who signed this is the same one whose
key is pinned"; the cosign attestation certifies "this binary was
built by the project's GitHub Actions runner from the source at
the tagged commit".

## Verifying the git tag (SSH — v0.7.0+)

The release-signing SSH public key is at the repo root:

```
keys/release-signing.pub
```

```bash
# One-time setup per clone — register the allowed signer:
mkdir -p .git/allowed_signers
echo "jaredreyespt@gmail.com $(cat keys/release-signing.pub)" \
    > .git/allowed_signers/release
git config --local gpg.ssh.allowedSignersFile .git/allowed_signers/release

# Verify:
git tag -v v0.7.0
```

A passing verification looks like:

```
Good "git" signature for jaredreyespt@gmail.com with ED25519 key
SHA256:ye9lgvE9qPCzlintwH2AxLpnz9RVkhEIBzkfwIEF+a8
```

GitHub's "Verified" badge is awarded automatically once the same
public key is registered as a signing key on the maintainer's
GitHub account (Settings → SSH and GPG keys → New SSH key →
"Signing Key" type).

## Verifying the git tag (GPG — future, org-managed)

Once the org-managed GPG key is provisioned (tracked under v0.8.0
infrastructure):

```bash
curl -O https://raw.githubusercontent.com/kimberlitedb/kimberlite/main/keys/release-signing.asc
gpg --import keys/release-signing.asc
git tag -v v0.8.0
```

The fingerprint will be pinned in [`SECURITY.md`](../../SECURITY.md)
when the key lands.

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

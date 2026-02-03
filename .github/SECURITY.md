# Security Policy

For the complete security policy, including vulnerability disclosure details and architecture security, see:

**[docs/SECURITY.md](../docs/SECURITY.md)**

## Quick Reporting Instructions

If you discover a security vulnerability in Kimberlite, please report it to:

**jared.reyes@kimberlite.dev**

### What to Include

- Description of the vulnerability
- Steps to reproduce
- Potential impact
- Suggested fix (if any)

### Response Timeline

- **Initial response:** Within 48 hours
- **Status update:** Within 7 days
- **Fix timeline:** Depends on severity (critical issues prioritized)

## Scope

Security vulnerabilities in:

- Cryptographic primitives (hash chains, signatures, encryption)
- Consensus protocol (VSR)
- Storage integrity (CRC32 checksums, log corruption)
- Multi-tenant isolation
- Authentication/authorization
- SQL injection or input validation

## Out of Scope

- Issues requiring physical access to the server
- Social engineering attacks
- Denial of service (unless it violates invariants)

---

**Do not open public GitHub issues for security vulnerabilities.** Always report privately via email first.

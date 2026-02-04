# Code Review Guide

**Internal Guide** - For Kimberlite maintainers and contributors

## Review Checklist

Use this checklist when reviewing pull requests.

### General

- [ ] **CI passes** - All checks green
- [ ] **Commit messages follow format** - `type(scope): subject`
- [ ] **PR description clear** - What, why, how
- [ ] **No unrelated changes** - Focused on single concern
- [ ] **Branch up to date** - Rebased on latest main

### Code Quality

- [ ] **No unsafe code** - Workspace lint enforces this
- [ ] **No recursion** - Use bounded loops instead
- [ ] **No unwrap** - Use `expect()` with reason, or `?` for errors
- [ ] **70-line soft limit** - Functions should be concise
- [ ] **Clear variable names** - Avoid abbreviations
- [ ] **No commented-out code** - Remove or explain why kept

### Correctness

- [ ] **Tests added** - New code has tests
- [ ] **Tests pass** - `just test` succeeds
- [ ] **Assertions present** - 2+ assertions per non-trivial function
- [ ] **Error handling** - All errors handled or propagated
- [ ] **No panics** - Unless explicitly intended (with doc comment)
- [ ] **Edge cases covered** - Empty, max size, boundary conditions

### Performance

- [ ] **No unnecessary allocations** - Use `&str` over `String` where possible
- [ ] **No N+1 queries** - Batch operations when possible
- [ ] **No busy loops** - Use proper synchronization
- [ ] **Benchmarks** - Performance-critical code has benchmarks

### Security

- [ ] **Input validation** - All external input validated
- [ ] **No timing attacks** - Crypto code uses constant-time operations
- [ ] **Secrets not logged** - No keys/passwords in logs
- [ ] **SQL injection prevented** - Use parameterized queries
- [ ] **Command injection prevented** - Sanitize shell commands

### Consensus/Storage Code

- [ ] **Invariants documented** - Clear comments on what must hold
- [ ] **Assertions for invariants** - Runtime checks
- [ ] **Property tests** - If testing invariants
- [ ] **VOPR scenario** - If new failure mode introduced
- [ ] **Byzantine attacks considered** - Malicious input handled

### Documentation

- [ ] **Public API documented** - /// doc comments
- [ ] **Examples provided** - For non-trivial APIs
- [ ] **CHANGELOG updated** - If user-facing change
- [ ] **Migration guide** - If breaking change
- [ ] **Design doc** - If significant architectural change

## Review Process

### 1. Initial Review (5-10 minutes)

Quick scan for obvious issues:
- CI status
- Diff size (>500 lines? Consider splitting)
- Commit messages
- PR description

### 2. Deep Review (20-60 minutes)

Thorough line-by-line review:
- Read code carefully
- Check tests
- Verify error handling
- Look for edge cases
- Consider alternatives

### 3. Leave Comments

```markdown
# Blocking (must fix before merge)
❌ **Blocking:** This could cause data loss. Use fsync before returning.

# Suggestions (nice to have)
💡 **Suggestion:** Consider using `Vec::with_capacity()` to reduce allocations.

# Nitpicks (optional)
🔧 **Nit:** Typo in comment: "recieve" → "receive"

# Praise (always good!)
✅ **Nice:** Great test coverage here!
```

### 4. Approve or Request Changes

- **Approve** - Looks good, can merge
- **Request Changes** - Blocking issues, need fixes
- **Comment** - Non-blocking feedback

## Common Issues

### 1. Missing Tests

```rust
// ❌ Bad: No tests
pub fn parse_sql(query: &str) -> Result<Ast> {
    // ...implementation...
}

// ✅ Good: Tests provided
pub fn parse_sql(query: &str) -> Result<Ast> {
    // ...implementation...
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_parse_select() {
        assert!(parse_sql("SELECT * FROM users").is_ok());
    }

    #[test]
    fn test_parse_invalid() {
        assert!(parse_sql("INVALID SQL").is_err());
    }
}
```

### 2. Missing Assertions

```rust
// ❌ Bad: No assertions
fn apply_commit(state: &mut State, cmd: Command) {
    state.commit_number += 1;
    state.apply(cmd);
}

// ✅ Good: Assertions present
fn apply_commit(state: &mut State, cmd: Command) {
    assert_eq!(state.status, Status::Normal, "can only commit in normal status");
    assert!(cmd.view >= state.view, "command view must not decrease");

    state.commit_number += 1;
    state.apply(cmd);

    assert!(state.commit_number <= state.op_number, "commit cannot exceed op");
}
```

### 3. Unwrap in Library Code

```rust
// ❌ Bad: Unwrap can panic
let value = map.get(&key).unwrap();

// ✅ Good: Proper error handling
let value = map.get(&key)
    .ok_or(Error::KeyNotFound)?;

// ✅ Also good: Expect with reason (if truly an invariant)
let value = map.get(&key)
    .expect("key must exist: verified in constructor");
```

### 4. No Documentation

```rust
// ❌ Bad: Public API without docs
pub fn connect(addr: &str) -> Result<Client> {
    // ...
}

// ✅ Good: Clear documentation
/// Connect to a Kimberlite cluster.
///
/// # Arguments
///
/// * `addr` - Server address (e.g., "localhost:7000")
///
/// # Returns
///
/// Connected client on success, error if connection fails.
///
/// # Example
///
/// ```
/// let client = connect("localhost:7000")?;
/// ```
pub fn connect(addr: &str) -> Result<Client> {
    // ...
}
```

### 5. Mutable State Without Synchronization

```rust
// ❌ Bad: Shared mutable state without locking
static mut COUNTER: u64 = 0;

fn increment() {
    unsafe {
        COUNTER += 1;  // Race condition!
    }
}

// ✅ Good: Atomic operations
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn increment() {
    COUNTER.fetch_add(1, Ordering::SeqCst);
}
```

## Giving Feedback

### Be Specific

```markdown
# ❌ Vague
This could be better.

# ✅ Specific
Consider using `HashMap::with_capacity(100)` to pre-allocate and avoid rehashing.
```

### Explain Why

```markdown
# ❌ No explanation
Don't use unwrap here.

# ✅ With explanation
Using `unwrap()` here could panic if the key doesn't exist. Since this is library code, we should return `Result` and let the caller decide how to handle missing keys.
```

### Suggest Alternatives

```markdown
# ❌ Just criticism
This is inefficient.

# ✅ With alternative
This allocates a new Vec on every call. Consider:
\`\`\`rust
// Reuse buffer
fn process(&mut self, data: &[u8]) {
    self.buffer.clear();
    self.buffer.extend_from_slice(data);
    // ...
}
\`\`\`
```

### Praise Good Code

```markdown
✅ Excellent test coverage here! The edge cases for empty input and max size are well handled.

✅ Nice use of the newtype pattern to prevent mixing up TenantId and StreamId.

✅ Clear assertion messages - really helpful for debugging.
```

## Receiving Feedback

### Don't Take It Personally

Code review is about the code, not you. Everyone's code gets reviewed.

### Ask Questions

If feedback is unclear, ask for clarification:
```markdown
> "This could cause issues under high load."

Can you elaborate on what kind of issues? Would adding a lock help, or is there a different approach you'd recommend?
```

### Push Back (Respectfully)

If you disagree, explain your reasoning:
```markdown
I considered that approach, but went with this one because it avoids the N+1 query problem we saw in issue #123. The extra allocation is acceptable here since this is called at most once per connection.
```

### Say Thanks

```markdown
Great catch! Fixed in latest commit.

Good point - I added a test for that case.
```

## Merging

### Requirements

- [ ] All comments addressed or discussed
- [ ] CI passing
- [ ] At least one approval from maintainer
- [ ] No merge conflicts

### Merge Strategy

Use **Squash and Merge** for:
- Feature branches with multiple WIP commits
- Experimental branches

Use **Rebase and Merge** for:
- Clean commit history (each commit builds)
- When preserving individual commits is valuable

Use **Merge Commit** for:
- Release branches
- Long-running feature branches

## Related Documentation

- **[Getting Started](getting-started.md)** - Setup and workflow
- **[Testing Strategy](testing-strategy.md)** - Testing requirements
- **[Release Process](release-process.md)** - Release checklist

---

**Key Takeaway:** Good code review catches bugs, improves code quality, and shares knowledge. Be thorough, specific, and constructive. Always explain your reasoning.

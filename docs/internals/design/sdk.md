# Multi-Language SDK Strategy

## Overview

Kimberlite provides idiomatic SDKs for multiple programming languages, enabling developers across ecosystems to build compliance-first applications with minimal friction. Our SDK architecture balances **protocol consistency** (single source of truth) with **language-native idioms** (Pythonic, idiomatic Go, etc.).

**Core Principle**: Hybrid FFI + Idiomatic Wrappers
- Single Rust FFI core library ensures protocol correctness
- Language-specific wrappers provide native developer experience
- Pre-compiled binaries for zero-configuration installation
- Auto-generated type definitions for IDE support

This approach follows [TigerBeetle's proven SDK architecture](https://github.com/tigerbeetle/tigerbeetle), which prioritizes reliability and consistency over per-language native implementations.

---

## Architecture

### Three-Layer Design

```
┌─────────────────────────────────────────────────────────────┐
│  Language Wrapper (Python/TypeScript/Go/Java/C#/C++)        │
│  - Idiomatic API (async/await, error handling, types)       │
│  - Memory safety (RAII, GC integration)                     │
│  - Zero-copy where possible                                 │
└─────────────────────────────────────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  FFI Core (kimberlite-ffi - C ABI)                          │
│  - C-compatible exports                                     │
│  - Protocol implementation (kimberlite-wire)                       │
│  - Connection pooling, cluster failover                     │
└─────────────────────────────────────────────────────────────┘
                            ▼
┌─────────────────────────────────────────────────────────────┐
│  Rust Core (kimberlite-client)                                     │
│  - Binary protocol (Bincode)                                │
│  - TCP + TLS transport                                      │
│  - Request/response matching                                │
└─────────────────────────────────────────────────────────────┘
```

### FFI Core Responsibilities

The `kimberlite-ffi` crate exposes a stable C ABI that:
- Wraps `kimberlite-client` with C-compatible function signatures
- Manages memory ownership boundaries (caller vs library)
- Provides error codes + human-readable messages
- Auto-generates header files via `cbindgen`
- Cross-compiles for Linux/macOS/Windows (x64 + aarch64)

**Memory Safety Guarantees**:
- All pointers are validated (null checks)
- Strings are UTF-8 validated
- Buffers include explicit lengths (no null-terminated assumptions)
- Result structs are caller-freed with explicit `_free()` functions
- Integration with Valgrind + AddressSanitizer in CI

### Language Wrapper Responsibilities

Each language SDK provides:
- **Native types**: `StreamId`, `DataClass`, `Offset` as language-appropriate types
- **Idiomatic errors**: Exceptions (Python/Java), `Result<T, E>` (Rust), `error` returns (Go)
- **Async patterns**: Promises (TypeScript), async/await (Python), goroutines (Go)
- **Resource cleanup**: Context managers (Python), RAII (C++), `defer` (Go)
- **Type safety**: Type hints (Python), generics (TypeScript/Go/Java)

---

## Supported Languages

### Tier 1: Production-Ready (Months 1-3)

| Language   | Status | Package       | Use Cases                          |
|------------|--------|---------------|------------------------------------|
| Rust       | ✅ Done | `kimberlite-client`  | Native implementation (already exists) |
| Python     | 🚧 Phase 11.2 | `kimberlite`  | Healthcare ML, compliance analytics, Jupyter notebooks |
| TypeScript | 🚧 Phase 11.3 | `@kimberlite/client` | Web dashboards, admin tools, MCP servers |

**Rationale**:
- **Python**: Dominant in healthcare ML/analytics (TensorFlow, PyTorch), compliance reporting (pandas)
- **TypeScript**: Web applications, admin dashboards, SaaS control planes

### Tier 2: Enterprise Focus (Months 4-6)

| Language | Status | Package          | Use Cases                          |
|----------|--------|------------------|------------------------------------|
| Go       | 📋 Phase 11.5 | `github.com/kimberlitedb/kimberlite-go` | Cloud infrastructure, microservices |
| Java     | 📋 Phase 11.6 | `com.kimberlite:kimberlite-client` | Enterprise healthcare (Epic, Cerner), fintech |

**Rationale**:
- **Go**: Matches `kimberlite-server` architecture, cloud-native deployments (Kubernetes operators)
- **Java**: Integration with Epic EHR, Cerner, enterprise compliance systems

### Tier 3: Specialized Domains (Months 7-9)

| Language | Status | Package        | Use Cases                          |
|----------|--------|----------------|------------------------------------|
| C#       | 📋 Phase 11.7 | `Kimberlite.Client` | Windows medical software, Unity medical simulation |
| C++      | 📋 Phase 11.8 | `kimberlite-cpp` | High-performance analytics, embedded medical devices |

**Rationale**:
- **C#**: Windows-based medical software, Unity training simulations
- **C++**: Low-latency analytics, embedded devices (patient monitors)

---

## C API Design

### Lifecycle Management

```c
typedef struct KmbClient KmbClient;
typedef uint32_t KmbError;

// Connection
KmbClient* kmb_client_connect(
    const char* addresses[],     // Array of "host:port" strings
    size_t address_count,
    uint64_t tenant_id,
    const char* auth_token,      // NULL-terminated
    KmbError* error_out          // Output parameter
);

void kmb_client_disconnect(KmbClient* client);

// Error handling
const char* kmb_error_message(KmbError error);  // Static string, do not free
bool kmb_error_is_retryable(KmbError error);
```

### Stream Operations

```c
// Create stream
KmbError kmb_client_create_stream(
    KmbClient* client,
    const char* name,           // NULL-terminated
    uint8_t data_class,         // 0=PHI, 1=NonPHI, 2=Deidentified
    uint64_t* stream_id_out     // Output parameter
);

// Append events (batch) with optimistic concurrency
KmbError kmb_client_append(
    KmbClient* client,
    uint64_t stream_id,
    uint64_t expected_offset,   // Expected stream offset (OCC)
    const uint8_t* events[],    // Array of byte buffers
    size_t event_lengths[],     // Parallel array of lengths
    size_t event_count,
    uint64_t* first_offset_out  // First offset of batch
);

// Read events
typedef struct {
    uint64_t offset;
    const uint8_t* data;
    size_t data_len;
} KmbEvent;

typedef struct {
    KmbEvent* events;
    size_t event_count;
} KmbReadResult;

KmbError kmb_client_read(
    KmbClient* client,
    uint64_t stream_id,
    uint64_t from_offset,
    size_t max_events,
    KmbReadResult** result_out
);

void kmb_read_result_free(KmbReadResult* result);
```

### Query Operations

```c
typedef enum {
    KMB_PARAM_INT64,
    KMB_PARAM_STRING,
    KMB_PARAM_BYTES,
    KMB_PARAM_BOOL,
    KMB_PARAM_NULL
} KmbParamType;

typedef struct {
    KmbParamType type;
    union {
        int64_t i64;
        const char* str;    // NULL-terminated
        struct {
            const uint8_t* data;
            size_t len;
        } bytes;
        bool boolean;
    } value;
} KmbQueryParam;

typedef struct {
    // Opaque handle (language wrappers extract columns)
    void* _internal;
    size_t row_count;
    size_t column_count;
} KmbQueryResult;

KmbError kmb_client_query(
    KmbClient* client,
    const char* sql,            // NULL-terminated
    KmbQueryParam* params,
    size_t param_count,
    KmbQueryResult** result_out
);

void kmb_query_result_free(KmbQueryResult* result);

// Column access
KmbError kmb_query_result_get_i64(
    KmbQueryResult* result,
    size_t row,
    size_t col,
    int64_t* value_out
);

KmbError kmb_query_result_get_string(
    KmbQueryResult* result,
    size_t row,
    size_t col,
    const char** value_out  // Points into result, do not free
);
```

### Error Codes

```c
#define KMB_OK                      0
#define KMB_ERR_NULL_POINTER        1
#define KMB_ERR_INVALID_UTF8        2
#define KMB_ERR_CONNECTION_FAILED   3
#define KMB_ERR_STREAM_NOT_FOUND    4
#define KMB_ERR_PERMISSION_DENIED   5
#define KMB_ERR_INVALID_DATA_CLASS  6
#define KMB_ERR_OFFSET_OUT_OF_RANGE 7
#define KMB_ERR_QUERY_SYNTAX        8
#define KMB_ERR_QUERY_EXECUTION     9
#define KMB_ERR_TENANT_NOT_FOUND   10
#define KMB_ERR_AUTH_FAILED        11
#define KMB_ERR_TIMEOUT            12
#define KMB_ERR_INTERNAL           13
#define KMB_ERR_CLUSTER_UNAVAILABLE 14
#define KMB_ERR_UNKNOWN            15
```

---

## Language-Specific Idioms

### Python (ctypes + Type Hints)

```python
from kimberlite import Client, DataClass, StreamNotFoundError
from typing import List

# Connection (context manager)
with Client.connect(
    addresses=["localhost:5432", "localhost:5433"],
    tenant_id=1,
    auth_token="secret"
) as client:
    # Create stream
    stream_id = client.create_stream("events", DataClass.PHI)

    # Append events
    events = [b"event1", b"event2", b"event3"]
    offset = client.append(stream_id, events)

    # Read events
    results = client.read(stream_id, from_offset=0, max_events=100)
    for event in results:
        print(f"Offset {event.offset}: {event.data}")

    # Query
    rows = client.query(
        "SELECT * FROM events WHERE timestamp > ?",
        params=[1704067200]  # Unix timestamp
    )
    for row in rows:
        print(row["id"], row["data"])
```

**Features**:
- Type hints for IDE autocomplete
- Context managers for resource cleanup
- Exceptions for error handling
- Generator-based iteration for large result sets

### TypeScript (N-API + Promises)

```typescript
import { Client, DataClass, StreamNotFoundError } from '@kimberlite/client';

async function main() {
  const client = await Client.connect({
    addresses: ['localhost:5432', 'localhost:5433'],
    tenantId: 1,
    authToken: 'secret'
  });

  try {
    // Create stream
    const streamId = await client.createStream('events', DataClass.PHI);

    // Append events
    const events = [
      Buffer.from('event1'),
      Buffer.from('event2'),
      Buffer.from('event3')
    ];
    const offset = await client.append(streamId, events);

    // Read events
    const results = await client.read(streamId, { fromOffset: 0, maxEvents: 100 });
    for (const event of results) {
      console.log(`Offset ${event.offset}: ${event.data}`);
    }

    // Query
    const rows = await client.query(
      'SELECT * FROM events WHERE timestamp > ?',
      [1704067200]
    );
    for (const row of rows) {
      console.log(row.id, row.data);
    }
  } finally {
    await client.disconnect();
  }
}
```

**Features**:
- Full TypeScript type inference (no `any`)
- Promise-based async API
- Auto-generated `.d.ts` type definitions
- Works in Node.js 18+ and Bun

### Go (Error Returns + Interfaces)

```go
package main

import (
    "context"
    "fmt"
    "log"

    "github.com/kimberlitedb/kimberlite-go"
)

func main() {
    client, err := kimberlite.Connect(kimberlite.Config{
        Addresses: []string{"localhost:5432", "localhost:5433"},
        TenantID:  1,
        AuthToken: "secret",
    })
    if err != nil {
        log.Fatal(err)
    }
    defer client.Close()

    ctx := context.Background()

    // Create stream
    streamID, err := client.CreateStream(ctx, "events", kimberlite.DataClassPHI)
    if err != nil {
        log.Fatal(err)
    }

    // Append events
    events := [][]byte{
        []byte("event1"),
        []byte("event2"),
        []byte("event3"),
    }
    offset, err := client.Append(ctx, streamID, events)
    if err != nil {
        log.Fatal(err)
    }

    // Read events
    results, err := client.Read(ctx, streamID, kimberlite.ReadOptions{
        FromOffset: 0,
        MaxEvents:  100,
    })
    if err != nil {
        log.Fatal(err)
    }

    for _, event := range results {
        fmt.Printf("Offset %d: %s\n", event.Offset, event.Data)
    }

    // Query
    rows, err := client.Query(ctx, "SELECT * FROM events WHERE timestamp > ?", 1704067200)
    if err != nil {
        log.Fatal(err)
    }
    defer rows.Close()

    for rows.Next() {
        var id int64
        var data []byte
        if err := rows.Scan(&id, &data); err != nil {
            log.Fatal(err)
        }
        fmt.Printf("%d: %s\n", id, data)
    }
}
```

**Features**:
- Context-aware cancellation
- `io.Closer` interface for `defer`
- SQL-style `Rows.Scan()` for queries
- No panics (explicit error returns)

---

## Distribution Strategy

### Pre-Compiled Binaries

Each SDK includes pre-compiled FFI libraries for:

| Platform      | Architecture | Binary Extension | CI Builder |
|---------------|--------------|------------------|------------|
| Linux (glibc) | x86_64       | `.so`            | Ubuntu 20.04 |
| Linux (glibc) | aarch64      | `.so`            | Ubuntu 20.04 (cross) |
| macOS         | x86_64       | `.dylib`         | macOS 12 |
| macOS         | aarch64 (Apple Silicon) | `.dylib` | macOS 14 |
| Windows       | x86_64       | `.dll`           | Windows Server 2022 |

**Packaging**:
- **Python**: Include binaries in wheel via `setup.py` data files
- **TypeScript**: Include in npm package, select at runtime via `process.platform`
- **Go**: Embed via `go:embed` directive
- **Java**: Package in JAR resources, extract to temp directory

### Version Synchronization

All SDKs share the same version number as the Rust core:
- **Format**: `MAJOR.MINOR.PATCH` (e.g., `0.1.0`)
- **Compatibility**: FFI ABI is stable within MAJOR version
- **Release**: All SDKs released simultaneously with changelog

### Package Registries

| Language   | Registry | Package Name              | Install Command |
|------------|----------|---------------------------|-----------------|
| Rust       | crates.io | `kimberlite`             | `cargo add kimberlite` |
| Python     | PyPI      | `kimberlite`             | `pip install kimberlite` |
| TypeScript | npm       | `@kimberlite/client`     | `npm install @kimberlite/client` |
| Go         | pkg.go.dev | `github.com/kimberlitedb/kimberlite-go` | `go get github.com/kimberlitedb/kimberlite-go` |
| Java       | Maven Central | `com.kimberlite:kimberlite-client` | (Maven/Gradle) |
| C#         | NuGet     | `Kimberlite.Client`      | `dotnet add package Kimberlite.Client` |

---

## Testing Strategy

### Three-Tier Approach

**Tier 1: FFI Layer (Rust)**
- Unit tests for each C-exported function
- Memory leak detection (Valgrind on Linux)
- Integration tests calling FFI from C program
- Fuzz testing for protocol parsing
- CI runs on Linux/macOS/Windows

**Tier 2: Language Wrappers**
- Type marshaling tests (correct FFI boundary crossing)
- Error propagation tests (FFI errors → language exceptions)
- Memory safety tests (no leaks in wrapper)
- Type inference tests (TypeScript, Python type checking)

**Tier 3: Cross-Language Integration**
- End-to-end tests against real `kimberlite-server`
- Multi-client consistency tests
- Cluster failover tests
- Performance benchmarks vs Rust baseline (< 10ms overhead target)

### Shared Test Fixture

All SDKs use identical test scenarios:

```yaml
# test-fixtures/scenarios.yaml
scenarios:
  - name: basic_append_read
    setup:
      - create_stream: {name: "test", data_class: "NonPHI"}
    steps:
      - append: {stream: "test", events: ["event1", "event2"]}
      - read: {stream: "test", from_offset: 0, expected_count: 2}

  - name: query_with_params
    setup:
      - create_stream: {name: "events", data_class: "PHI"}
      - append: {stream: "events", events: ["a", "b", "c"]}
    steps:
      - query: {sql: "SELECT COUNT(*) FROM events", expected_rows: 1}
```

Each SDK implements a test harness that executes these scenarios, ensuring behavioral consistency.

---

## Performance Targets

| Metric | Target | Measurement |
|--------|--------|-------------|
| FFI overhead | < 10ms p99 | Benchmark vs direct Rust client |
| Memory overhead | < 5% | RSS comparison (Valgrind) |
| Throughput degradation | < 10% | Events/second vs Rust baseline |
| Binary size | < 10 MB | Stripped release binary |
| Cold start time | < 100ms | Time to first request |

### Optimization Techniques

- **Zero-copy reads**: Language wrappers return views into FFI-owned memory
- **Connection pooling**: FFI core maintains pool, wrappers multiplex
- **Batch operations**: All SDKs support batch append/read
- **Async I/O**: TypeScript/Python use event loop integration

---

## Documentation Requirements

Each SDK must include:

1. **README.md**
   - Installation instructions
   - Quickstart example
   - Link to API reference
   - Supported platforms

2. **API Reference** (auto-generated)
   - Python: Sphinx
   - TypeScript: TypeDoc
   - Go: godoc
   - Java: Javadoc

3. **Guides** (`docs/guides/`)
   - `quickstart-{language}.md`
   - `connection-pooling.md`
   - `compliance-patterns.md` (audit trails, point-in-time queries)
   - `cluster-deployment.md`

4. **Examples** (`examples/{language}/`)
   - Basic CRUD operations
   - Compliance audit trail
   - Real-time event subscription
   - Multi-tenant isolation

---

## Implementation Phases

### Phase 11.1: FFI Core Infrastructure (Weeks 1-4)

**Deliverables**:
- `crates/kimberlite-ffi/` crate with C exports
- Auto-generated `kimberlite-ffi.h` header
- Cross-compilation in CI (`.github/workflows/build-ffi.yml`)
- Memory safety tests (Valgrind, AddressSanitizer)

**Acceptance Criteria**:
- ✅ FFI library builds for all platforms
- ✅ All 7 operations accessible via C API
- ✅ Zero memory leaks under Valgrind
- ✅ Integration test calling FFI from C program

### Phase 11.2: Python SDK (Weeks 5-7)

**Deliverables**:
- `sdks/python/kimberlite/` package
- `client.py`, `types.py`, `errors.py`
- Type stubs (`.pyi`) for IDE support
- Wheel distribution with bundled `.so`/`.dylib`/`.dll`

**Acceptance Criteria**:
- ✅ `pip install kimberlite` works on all platforms
- ✅ 90%+ code coverage
- ✅ Passes `mypy --strict`
- ✅ < 10ms FFI overhead

### Phase 11.3: TypeScript SDK (Weeks 8-10)

**Deliverables**:
- `sdks/typescript/src/` package
- `client.ts`, `types.ts`, `errors.ts`
- N-API bindings with Promise-based API
- npm package with pre-built binaries

**Acceptance Criteria**:
- ✅ `npm install @kimberlite/client` works
- ✅ Full TypeScript type inference (no `any`)
- ✅ < 10ms latency overhead
- ✅ Works in Node.js 18+

### Phase 11.4: Documentation & Protocol Spec (Weeks 11-12)

**Deliverables**:
- `docs/SDK.md` (this document)
- `docs/PROTOCOL.md` - Wire protocol specification
- `docs/guides/` - Language-specific quickstarts
- GitHub Pages setup

**Acceptance Criteria**:
- ✅ Protocol spec is implementable by third parties
- ✅ Each SDK has quickstart + API reference
- ✅ Guides include code examples for all supported languages

---

## Risk Mitigation

| Risk | Impact | Mitigation |
|------|--------|------------|
| FFI complexity leads to crashes | High | Extensive memory safety tests (Valgrind, ASAN), fuzzing |
| Language-specific bugs diverge | Medium | Shared integration test suite, CI enforcement |
| Version skew across SDKs | Medium | Synchronized releases, protocol version enforcement |
| Performance degradation | Medium | Benchmark suite in CI, < 10ms overhead budget |
| Community fragmentation | Low | Single FFI core, shared docs, unified release notes |

---

## Success Metrics

**Phase 11.1 (FFI Core)**:
- ✅ FFI library compiles for all platforms
- ✅ Zero memory leaks (Valgrind)
- ✅ All operations accessible via C API

**Phase 11.2 (Python)**:
- ✅ Published to PyPI
- ✅ 90%+ code coverage
- ✅ Passes `mypy --strict`
- ✅ 10+ beta testers

**Phase 11.3 (TypeScript)**:
- ✅ Published to npm
- ✅ Full TypeScript inference
- ✅ < 10ms latency overhead
- ✅ Works in Node.js 18+

**Phase 11.4 (Documentation)**:
- ✅ Protocol spec is complete
- ✅ Each SDK has quickstart guide
- ✅ GitHub Pages live

**Note**: Future SDK implementations (Go, Java, C#, C++, WebAssembly) are documented in [ROADMAP.md](../../../ROADMAP.md#language-sdks).

---

## Contributing

### Adding a New Language SDK

1. **Read `docs/PROTOCOL.md`** - Understand wire protocol
2. **Create `sdks/{language}/`** - Follow directory structure
3. **Wrap FFI core** - Use `kimberlite-ffi.h`
4. **Add tests** - Implement shared test scenarios
5. **Document** - README + API reference + quickstart guide
6. **Submit PR** - Include CI configuration for new SDK

### SDK Quality Checklist

- [ ] Idiomatic error handling (exceptions vs Result vs error returns)
- [ ] Type safety (type hints, generics, enums)
- [ ] Resource cleanup (RAII, context managers, defer)
- [ ] Async patterns (Promises, async/await, goroutines)
- [ ] Memory safety (no leaks, proper FFI boundary)
- [ ] Documentation (README, API reference, examples)
- [ ] Tests (unit, integration, type checking)
- [ ] CI (build, test, lint for new language)

---

## References

- **Protocol Specification**: [docs/PROTOCOL.md](../../reference/protocol.md)
- **FFI Header**: `crates/kimberlite-ffi/kimberlite-ffi.h`
- **Rust Client**: `crates/kimberlite-client/`
- **Test Fixtures**: `test-fixtures/scenarios.yaml`
- **TigerBeetle SDK Approach**: https://github.com/tigerbeetle/tigerbeetle/tree/main/src/clients

---

## FAQ

**Q: Why FFI + wrappers instead of pure language implementations?**
A: Single source of truth for protocol correctness, lower maintenance burden, shared optimizations.

**Q: What about performance overhead?**
A: Target < 10ms p99 FFI overhead. Most workloads are network-bound, not FFI-bound.

**Q: Can I use Kimberlite from a language not listed here?**
A: Yes! Read `docs/PROTOCOL.md` and implement a wrapper around `kimberlite-ffi.h`. We accept third-party SDK contributions.

**Q: How do I handle breaking changes?**
A: FFI ABI is stable within MAJOR version. Language wrappers can evolve independently (patch versions).

**Q: What about WebAssembly?**
A: Planned for Phase 11.9. Requires Rust core compiled to WASM + JavaScript wrapper.

# Kimberlite SDKs

Multi-language client libraries for Kimberlite database.

## Architecture

All SDKs share a common **FFI core** (`kimberlite-ffi`) that wraps the Rust client library. This ensures protocol consistency and simplifies maintenance while providing idiomatic APIs for each language.

```
┌─────────────────────────────────────────────┐
│   Language-Specific SDKs (idiomatic APIs)   │
│   Python | TypeScript | Go | Java | C# | C++│
├─────────────────────────────────────────────┤
│   FFI Core (kimberlite-ffi - C ABI)         │
├─────────────────────────────────────────────┤
│   Rust Client (kimberlite-client)                  │
└─────────────────────────────────────────────┘
```

## Available SDKs

### Tier 1: Production-Ready (Beta)

| Language   | Status | Package | Documentation |
|------------|--------|---------|---------------|
| **Rust**   | ✅ Native | `kimberlite` | [Quickstart](../docs/guides/quickstart-rust.md) |
| **Python** | 🧪 Beta | `pip install kimberlite` | [Quickstart](../docs/guides/quickstart-python.md) |
| **TypeScript** | 🧪 Beta | `npm install @kimberlite/client` | [Quickstart](../docs/guides/quickstart-typescript.md) |

### Tier 2: Planned

| Language | Status | Target | Documentation |
|----------|--------|--------|---------------|
| **Go**   | 📋 Planned | Phase 11.5 | [Quickstart](../docs/guides/quickstart-go.md) |
| **Java** | 📋 Planned | Phase 11.6 | TBD |

### Tier 3: Future

| Language | Status | Target |
|----------|--------|--------|
| **C#**   | 📋 Planned | Phase 11.7 |
| **C++**  | 📋 Planned | Phase 11.8 |

## Quick Start

### Rust

```rust
use kimberlite::{Kimberlite, DataClass};
use kmb_types::TenantId;

let db = Kimberlite::open("./data")?;
let tenant = db.tenant(TenantId::new(1));

let stream_id = tenant.create_stream("events", DataClass::PHI)?;
let events = vec![b"event1".to_vec(), b"event2".to_vec()];
let offset = tenant.append(stream_id, events)?;

println!("Appended at offset: {}", offset);
```

### Python

```python
from kimberlite import Client, DataClass

client = Client.connect(
    addresses=["localhost:5432"],
    tenant_id=1
)

stream_id = client.create_stream("events", DataClass.PHI)
offset = client.append(stream_id, [b"event1", b"event2"])

print(f"Appended at offset: {offset}")
client.disconnect()
```

### TypeScript

```typescript
import { Client, DataClass } from '@kimberlite/client';

const client = await Client.connect({
  addresses: ['localhost:5432'],
  tenantId: 1n
});

const streamId = await client.createStream('events', DataClass.PHI);
const offset = await client.append(streamId, [
  Buffer.from('event1'),
  Buffer.from('event2')
]);

console.log(`Appended at offset: ${offset}`);
await client.disconnect();
```

## SDK Features

### Core Operations

All SDKs support the following operations:

- **Lifecycle**: `connect()`, `disconnect()`
- **Streams**: `create_stream(name, data_class)`
- **Write**: `append(stream_id, events)`
- **Read**: `read(stream_id, from_offset, max_bytes)`
- **Query**: SQL-based queries (future)
- **Admin**: Tenant management, checkpoints, exports

### Common Patterns

#### Connection Pooling

Create a single client instance and reuse it across operations:

```python
# Python - using Flask
from flask import Flask, request
from kimberlite import Client

app = Flask(__name__)
client = None

@app.before_first_request
def init_client():
    global client
    client = Client.connect(addresses=["localhost:5432"], tenant_id=1)

@app.route('/events', methods=['POST'])
def post_event():
    offset = client.append(stream_id, [request.data])
    return {'offset': int(offset)}
```

See [Connection Pooling Guide](../docs/guides/connection-pooling.md) for framework-specific examples.

#### Error Handling

All SDKs provide language-idiomatic error handling:

```typescript
// TypeScript - using try/catch
try {
  const streamId = await client.createStream('events', DataClass.PHI);
} catch (error) {
  if (error instanceof PermissionDeniedError) {
    console.error('No permission for PHI data');
  } else if (error instanceof ConnectionError) {
    console.error('Failed to connect to cluster');
  }
}
```

```python
# Python - using exception handling
from kimberlite import StreamNotFoundError, PermissionDeniedError

try:
    stream_id = client.create_stream("events", DataClass.PHI)
except PermissionDeniedError:
    print("No permission for PHI data")
except StreamNotFoundError:
    print("Stream not found")
```

## Development

### Building the FFI Core

```bash
# Build FFI library for current platform
just build-ffi

# Run FFI tests
just test-ffi

# Run memory safety tests (Linux only)
just test-ffi-valgrind
```

### Testing SDKs

```bash
# Test all SDKs
just test-sdks

# Test individual SDKs
just test-python
just test-typescript
```

### Building Packages

```bash
# Build Python wheel
just build-python-wheel

# Build TypeScript package
just build-typescript
```

## CI/CD

All SDKs are automatically tested in CI:

- **Cross-compilation**: FFI library built for Linux (x64, aarch64), macOS (x64, Apple Silicon), Windows (x64)
- **Memory safety**: Valgrind and AddressSanitizer on Linux
- **SDK tests**: Python 3.8-3.12, Node.js 18-21 on all platforms
- **Type checking**: mypy (Python), tsc (TypeScript)
- **Coverage**: 70%+ coverage requirement

See [.github/workflows/build-ffi.yml](../.github/workflows/build-ffi.yml) for details.

## Performance

All SDKs share the same high-performance Rust core:

- **Throughput**: 100K+ ops/sec (single-threaded append)
- **Latency**: < 10ms FFI overhead vs native Rust
- **Memory**: Zero-copy reads, pooled connections
- **Concurrency**: Thread-safe clients, connection pooling

## Security

- **Encryption**: AES-256-GCM for data at rest
- **Authentication**: Token-based auth with tenant isolation
- **TLS**: Support for encrypted connections (future)
- **Memory safety**: Rust core prevents buffer overflows, use-after-free

## Contributing

### Adding a New SDK

1. **Choose your language** from the planned tier
2. **Study existing SDKs** (Python and TypeScript are good references)
3. **Implement FFI bindings** for your language's FFI system
4. **Wrap with idiomatic API** following language conventions
5. **Add tests** with 70%+ coverage
6. **Write quickstart guide** in `docs/guides/quickstart-<lang>.md`
7. **Add CI job** to `.github/workflows/build-ffi.yml`

See [docs/SDK.md](../docs/SDK.md) for the complete SDK strategy.

### SDK Guidelines

- **Idiomatic**: Follow language conventions (naming, error handling, async patterns)
- **Type-safe**: Use strong typing (Python type hints, TypeScript strict mode)
- **Well-tested**: Unit tests, integration tests, type checking
- **Well-documented**: Quickstart guide, API reference, examples
- **Cross-platform**: Linux, macOS, Windows support

## Resources

- **[SDK Strategy](../docs/SDK.md)** - Architecture and design decisions
- **[Protocol Specification](../docs/PROTOCOL.md)** - Wire protocol for implementers
- **[Connection Pooling Guide](../docs/guides/connection-pooling.md)** - Framework integrations
- **[API Documentation](https://kimberlitedb.github.io/kimberlite/)** - Auto-generated docs

## Support

- **GitHub Issues**: [kimberlitedb/kimberlite/issues](https://github.com/kimberlitedb/kimberlite/issues)
- **Documentation**: [GitHub Pages](https://kimberlitedb.github.io/kimberlite/)
- **Protocol Questions**: See [docs/PROTOCOL.md](../docs/PROTOCOL.md)

## License

All SDKs are licensed under Apache-2.0, same as the core Kimberlite project.

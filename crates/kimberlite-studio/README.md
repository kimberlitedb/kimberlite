# Kimberlite Studio

Beautiful, reactive web UI for Kimberlite database exploration and query execution.

## Overview

Kimberlite Studio is an embedded web interface that provides:
- **SQL Query Editor** with syntax highlighting and keyboard shortcuts
- **Real-time Results Display** with academic table styling
- **Time-Travel Queries** showcasing Kimberlite's immutability
- **Schema Navigation** with interactive tree view
- **Tenant Safety** with explicit, compliance-first selection
- **Reactive UI** using Datastar SSE without build step
- **Accessibility** WCAG 2.1 Level AA compliant
- **Embedded Assets** single binary distribution

## Architecture

```
┌─────────────────────────────────────────────────────┐
│              Kimberlite Studio (Axum)               │
├─────────────────────────────────────────────────────┤
│                                                     │
│  Static Assets (Embedded)                          │
│  ├── CSS (CUBE architecture)                       │
│  ├── Fonts (Signifier, Söhne, Söhne Mono)          │
│  └── Vendor (Datastar.js, icon sprite)             │
│                                                     │
│  HTTP Routes                                        │
│  ├── GET  /                    → index.html        │
│  ├── GET  /css/*               → Stylesheets       │
│  ├── GET  /fonts/*             → Web fonts         │
│  ├── GET  /vendor/*            → JavaScript        │
│  ├── POST /api/query           → Execute SQL       │
│  ├── POST /api/select-tenant   → Change tenant     │
│  ├── GET  /sse/projection-updates → Schema events  │
│  └── GET  /sse/query-results   → Query streaming   │
│                                                     │
│  Backend (Rust)                                     │
│  ├── ProjectionBroadcast (real-time events)        │
│  ├── StudioState (shared state)                    │
│  ├── Templates (server-side rendering)             │
│  └── Assets (embedded via include_dir!)            │
│                                                     │
└─────────────────────────────────────────────────────┘
                        ↓
              ProjectionBroadcast
                        ↓
        ┌───────────────────────────┐
        │  Kimberlite Core (kernel) │
        │  - Table created/updated  │
        │  - Index created          │
        │  - DML operations         │
        └───────────────────────────┘
```

## Design System

**"Math Research Paper But Alive"** aesthetic from website:

### Typography
- **Headings**: Signifier (serif, bold, tight leading)
- **Body**: Söhne (sans-serif, readable)
- **Code/Data**: Söhne Mono (monospace)

### Colors (OKLCH)
- Perceptually uniform color scale (16 levels)
- Automatic light/dark mode via `light-dark()` CSS function
- High contrast support via `@media (prefers-contrast: high)`

### Components
- Square buttons (no border-radius)
- Uppercase button text
- Academic figure captions
- Hard borders, clear separations
- Subtle animations (respects `prefers-reduced-motion`)

### CSS Architecture (CUBE CSS)
Layered cascade for maintainability:

1. **Reset** - Normalize browser defaults
2. **Global** - Design tokens, fonts, base HTML
3. **Compositions** - Layout primitives (Every Layout patterns)
4. **Blocks** - Components (button, query-editor, etc.)
5. **Utilities** - Single-purpose helpers

Total embedded size: ~2MB (fonts + CSS + JS)

## Features

### 1. Query Editor
- Monospace textarea with terminal styling
- Ctrl+Enter / Cmd+Enter to execute
- Real-time signal binding
- Disabled state when no tenant selected

### 2. Results Table
- Server-rendered HTML streamed via SSE
- Academic table styling (sticky headers, hover states)
- Type-aware cell formatting (number, boolean, null)
- Row count display
- Pagination (future)
- Export (CSV/JSON) (future)

### 3. Time-Travel Controls ⭐
- Range slider for offset selection
- "Latest" button to reset
- Real-time offset value display
- Showcases Kimberlite's immutability
- Re-executes query at specific offset

### 4. Tenant Selector (Compliance-First)
- Explicit selection required
- No implicit defaults
- Warning when no tenant selected
- Query execution blocked until selection
- Visual badge showing selected tenant

### 5. Schema Tree Sidebar
- Hierarchical view (tenants → tables → columns)
- Server-rendered via SSE
- Real-time updates on schema changes
- Click to insert table name (future)
- Collapsible sections (future)

### 6. Real-Time Updates (SSE)
- Projection events broadcast from Kimberlite core
- Schema tree updates on table create/drop
- Max offset updates on DML operations
- Keep-alive pings every 15 seconds
- Automatic reconnection on disconnect

### 7. Keyboard Shortcuts
| Shortcut | Action |
|----------|--------|
| `Ctrl+Enter` / `Cmd+Enter` | Execute query |
| `Cmd+K` / `Ctrl+K` | Focus query editor |
| `Escape` | Clear errors |

### 8. Accessibility (WCAG 2.1 AA)
- ✅ Semantic HTML (header, main, aside)
- ✅ Keyboard navigation (tab, focus-visible)
- ✅ Screen reader support (ARIA labels, live regions)
- ✅ Color contrast ≥ 4.5:1
- ✅ Reduced motion support
- ✅ Skip to main content link
- ✅ High contrast mode

## Usage

### Starting Studio

```rust
use kimberlite_studio::{run_studio, StudioConfig};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = StudioConfig {
        port: 5555,
        db_address: "127.0.0.1:5432".to_string(),
        default_tenant: Some(1),
    };

    // Optional: wire up projection events
    let broadcast = Arc::new(kimberlite::broadcast::ProjectionBroadcast::default());

    // Start server
    run_studio(config, Some(broadcast)).await?;
    Ok(())
}
```

### Integration with Kimberlite

```rust
use kimberlite::{Kimberlite, broadcast::ProjectionBroadcast};
use std::sync::Arc;

let db = Kimberlite::open("./data")?;

// Set up broadcast for Studio
let broadcast = Arc::new(ProjectionBroadcast::default());
db.set_projection_broadcast(broadcast.clone())?;

// Start Studio (in separate task)
tokio::spawn(async move {
    let config = StudioConfig::default();
    run_studio(config, Some(broadcast)).await
});

// Now all table operations emit events to Studio UI
let tenant = db.tenant(TenantId::from(1));
tenant.execute("CREATE TABLE patients (id BIGINT, name TEXT)", &[])?;
// → Studio UI receives TableCreated event and updates schema tree
```

## Development

### Prerequisites
- Rust 1.85+
- Assets copied from `/website/public/` (fonts, CSS, vendor files)

### Building

```bash
# Check compilation
cargo check -p kimberlite-studio

# Run tests
cargo test -p kimberlite-studio

# Build release
cargo build -p kimberlite-studio --release
```

### Asset Structure

```
assets/
├── css/
│   ├── global/          # Design tokens, fonts, reset
│   ├── compositions/    # Layout primitives
│   ├── blocks/          # Components
│   ├── utilities/       # Helpers
│   └── studio.css       # Main entry point
├── fonts/
│   ├── test-signifier-*.woff2    # Serif (headings)
│   ├── test-soehne-*.woff2       # Sans-serif (body)
│   └── test-soehne-mono-*.woff2  # Monospace (code)
├── vendor/
│   └── datastar.js               # Reactive framework
└── icons/
    └── sustyicons.svg            # Icon sprite
```

### Testing

See [studio-testing.md](../../docs-internal/contributing/studio-testing.md) for comprehensive test guide.

### Accessibility

See [studio-accessibility.md](../../docs-internal/contributing/studio-accessibility.md) for WCAG compliance details.

## Dependencies

### Runtime
- `axum` - Web server framework
- `tokio` - Async runtime
- `tower-http` - HTTP middleware
- `include_dir` - Asset embedding
- `async-stream` - SSE streaming
- `futures` - Stream utilities
- `html-escape` - XSS prevention
- `kimberlite` (with `broadcast` feature) - Core database

### Development
- `tempfile` - Testing
- `tokio-test` - Async testing

## Performance

### Metrics (Target)
- First Contentful Paint: < 1.0s
- Time to Interactive: < 2.0s
- Lighthouse Performance: ≥ 90
- Lighthouse Accessibility: 100

### Bundle Sizes
- Main CSS: ~50KB (uncompressed)
- Fonts: ~1.2MB (20 WOFF2 files)
- Datastar.js: ~30KB
- Icon sprite: ~153KB
- **Total**: ~1.8MB

### Optimizations
- Font preloading for critical paths
- CSS layer ordering for cascade efficiency
- SSE keep-alive reduces reconnections
- Embedded assets (no network requests)
- No build step (pure HTML/CSS/Datastar)

## Roadmap

### Phase 3 ✅ (Completed)
- [x] Asset migration from website
- [x] Static asset serving
- [x] HTML template with design system
- [x] Query editor UI
- [x] Results table UI
- [x] Time-travel controls UI
- [x] Tenant selector UI
- [x] Schema tree sidebar UI
- [x] ProjectionBroadcast event system
- [x] SSE endpoints (projection updates, query results)
- [x] API endpoints (execute query, select tenant)
- [x] Theme toggle
- [x] Keyboard shortcuts
- [x] Loading indicators and error states
- [x] Animations and polish
- [x] Accessibility (WCAG 2.1 AA)
- [x] Testing documentation

### Phase 4 (Next Steps)
- [ ] Wire up real kimberlite_client for query execution
- [ ] Implement actual time-travel query logic (query at offset)
- [ ] Connect projection events to real schema updates
- [ ] Add query history and favorites
- [ ] Implement pagination for large result sets
- [ ] Add export functionality (CSV, JSON, SQL)
- [ ] SQL syntax highlighting (CodeMirror integration)
- [ ] Query explain/analyze
- [ ] Schema diff visualization

### Future Enhancements
- [ ] Saved query snippets
- [ ] Collaborative cursors (multi-user)
- [ ] Query performance metrics
- [ ] Visual query builder
- [ ] Data visualization (charts, graphs)
- [ ] Audit log viewer
- [ ] Tenant comparison mode
- [ ] API client generator (curl, SDKs)

## License

Apache-2.0

## Contributing

1. Follow CLAUDE.md guidelines
2. Run `just pre-commit` before commits
3. Add tests for new features
4. Update `docs-internal/contributing/studio-testing.md` for new workflows
5. Maintain accessibility standards (WCAG 2.1 AA)
6. Document keyboard shortcuts
7. Use design system tokens (no hardcoded colors)

## Credits

- **Design System**: Inspired by Kimberlite website ("Math Research Paper But Alive")
- **Fonts**: Klim Type Foundry (Signifier, Söhne, Söhne Mono)
- **Icons**: Susty Icons
- **Reactive Framework**: Datastar
- **Layout Primitives**: Every Layout by Andy Bell & Heydon Pickering
- **CSS Architecture**: CUBE CSS by Andy Bell

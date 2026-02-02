//! Asset management and embedding.

use include_dir::{Dir, include_dir};

// Embed all assets at compile time
#[allow(dead_code)]
static ASSETS: Dir = include_dir!("$CARGO_MANIFEST_DIR/assets");

/// Get index.html
pub const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="en" data-theme="light">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Kimberlite Studio</title>

    <!-- Preload critical fonts for performance -->
    <link rel="preload" href="/fonts/test-signifier-medium.woff2" as="font" type="font/woff2" crossorigin>
    <link rel="preload" href="/fonts/test-soehne-buch.woff2" as="font" type="font/woff2" crossorigin>
    <link rel="preload" href="/fonts/test-soehne-mono-buch.woff2" as="font" type="font/woff2" crossorigin>

    <!-- Main stylesheet (CUBE CSS architecture) -->
    <link rel="stylesheet" href="/css/studio.css">

    <!-- Datastar reactive framework -->
    <script type="module" src="/vendor/datastar.js"></script>

    <!-- Theme detection and initialization -->
    <script>
        (function() {
            const stored = localStorage.getItem('kimberlite-theme');
            const prefersDark = window.matchMedia('(prefers-color-scheme: dark)').matches;
            const theme = stored || (prefersDark ? 'dark' : 'light');
            document.documentElement.setAttribute('data-theme', theme);
            document.documentElement.style.colorScheme = theme;
        })();
    </script>
</head>
<body>
    <!-- Main layout grid -->
    <div class="studio-layout" data-signals='{
        "tenant_id": null,
        "query": "",
        "offset": null,
        "max_offset": 0,
        "loading": false,
        "error": null,
        "results": null,
        "show_sidebar": true,
        "theme": "light"
    }'>
        <!-- Header -->
        <header class="studio-header">
            <div class="repel" style="padding: var(--space-s) var(--space-m); align-items: center;">
                <div class="cluster" style="gap: var(--space-m); align-items: center;">
                    <h1 style="font-size: 20px; margin: 0; font-weight: var(--font-bold);">Kimberlite Studio</h1>

                    <!-- Tenant Selector -->
                    <div class="tenant-selector" data-bind-data-selected="$tenant_id === null ? 'false' : 'true'">
                        <label class="tenant-selector__label">Tenant</label>
                        <select class="tenant-selector__select"
                                data-model="tenant_id"
                                data-on-change="console.log('Tenant changed:', $tenant_id)">
                            <option value="">Select tenant...</option>
                            <option value="1">dev-fixtures (ID: 1)</option>
                        </select>
                        <div class="tenant-selector__warning" data-show="$tenant_id === null">
                            ⚠️ Select a tenant to execute queries
                        </div>
                    </div>
                </div>

                <div class="cluster" style="gap: var(--space-xs);">
                    <!-- Theme Toggle -->
                    <button type="button" class="button" data-variant="ghost-light"
                            data-on-click="
                                const next = document.documentElement.getAttribute('data-theme') === 'light' ? 'dark' : 'light';
                                document.documentElement.setAttribute('data-theme', next);
                                document.documentElement.style.colorScheme = next;
                                localStorage.setItem('kimberlite-theme', next);
                                $theme = next;
                            ">
                        <span data-show="$theme === 'light'">🌙</span>
                        <span data-show="$theme === 'dark'">☀️</span>
                    </button>
                </div>
            </div>
        </header>

        <!-- Sidebar (Schema Tree) -->
        <aside class="studio-sidebar" data-bind-data-mobile-open="$show_sidebar">
            <div class="flow" data-space="s" style="padding: var(--space-m);">
                <h2 style="font-size: 14px; text-transform: uppercase; letter-spacing: 0.05em; margin: 0;">
                    Schema
                </h2>
                <div id="schema-tree">
                    <div class="schema-tree">
                        <div class="schema-tree__item" data-level="0" data-type="info" style="color: var(--text-tertiary); font-style: italic;">
                            Select a tenant to view schema
                        </div>
                    </div>
                </div>
            </div>
        </aside>

        <!-- Main Content Area -->
        <main class="studio-main">
            <div class="wrapper" data-width="wide">
                <div class="flow" data-space="l" style="padding: var(--space-l) 0;">

                    <!-- Query Editor -->
                    <section class="query-editor">
                        <div class="query-editor__container">
                            <div class="query-editor__header">
                                <h2 class="query-editor__title">SQL Query</h2>
                            </div>
                            <textarea
                                class="query-editor__textarea"
                                data-model="query"
                                placeholder="SELECT * FROM patients LIMIT 10"
                                data-on-keydown="
                                    if ((evt.ctrlKey || evt.metaKey) && evt.key === 'Enter') {
                                        evt.preventDefault();
                                        $el.closest('section').querySelector('[data-execute-query]').click();
                                    }
                                "></textarea>
                            <div class="query-editor__footer">
                                <span class="query-editor__hint">
                                    <kbd>Ctrl</kbd>+<kbd>Enter</kbd> to execute
                                </span>
                                <button type="button"
                                        class="button"
                                        data-variant="primary"
                                        data-execute-query
                                        data-bind-disabled="$tenant_id === null || $loading"
                                        data-on-click="
                                            if ($tenant_id === null) {
                                                $error = 'Please select a tenant first';
                                                return;
                                            }
                                            $loading = true;
                                            $error = null;
                                            console.log('TODO: Execute query via SSE');
                                            setTimeout(() => {
                                                $loading = false;
                                                $results = {
                                                    columns: ['id', 'name', 'created_at'],
                                                    rows: [
                                                        ['1', 'Alice', '2024-01-01'],
                                                        ['2', 'Bob', '2024-01-02']
                                                    ]
                                                };
                                            }, 500);
                                        ">
                                    <span data-show="!$loading">Execute Query</span>
                                    <span data-show="$loading">
                                        <span class="loading-spinner"></span> Running...
                                    </span>
                                </button>
                            </div>
                        </div>
                    </section>

                    <!-- Time-Travel Controls -->
                    <section class="time-travel" data-show="$max_offset > 0">
                        <div class="time-travel__header">
                            <span class="time-travel__label">Query at offset</span>
                            <span class="time-travel__value" data-bind-data-latest="$offset === null">
                                <span data-text="$offset !== null ? $offset : 'latest'"></span>
                            </span>
                        </div>
                        <div class="time-travel__slider">
                            <input type="range"
                                   data-model="offset"
                                   min="0"
                                   data-bind-max="$max_offset"
                                   data-on-change="console.log('TODO: Re-execute at offset', $offset)">
                        </div>
                        <div class="time-travel__controls">
                            <button type="button" class="button" data-variant="ghost"
                                    data-on-click="$offset = null">
                                ← Latest
                            </button>
                        </div>
                        <div class="time-travel__info">
                            <strong>Time Travel:</strong> Query the database at any point in its history using offsets.
                        </div>
                    </section>

                    <!-- Error Banner -->
                    <div class="error-banner" data-show="$error !== null">
                        <div class="error-banner__title">Error</div>
                        <div class="error-banner__message" data-text="$error"></div>
                    </div>

                    <!-- Results Table -->
                    <section id="results-container" data-show="$results !== null">
                        <div class="results-table">
                            <div class="results-table__empty">
                                Execute a query to see results
                            </div>
                        </div>
                    </section>

                </div>
            </div>
        </main>
    </div>

    <!-- Keyboard shortcuts handler -->
    <script>
        document.addEventListener('keydown', (e) => {
            // Cmd/Ctrl + K: Focus query editor
            if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
                e.preventDefault();
                document.querySelector('.query-editor__textarea')?.focus();
            }

            // Escape: Clear errors
            if (e.key === 'Escape') {
                const signals = window.datastar?.store;
                if (signals) {
                    signals.error = null;
                }
            }
        });

        console.log('Kimberlite Studio initialized');
        console.log('Keyboard shortcuts:');
        console.log('  Ctrl+Enter: Execute query');
        console.log('  Cmd/Ctrl+K: Focus query editor');
        console.log('  Escape: Clear errors');
    </script>
</body>
</html>
"#;

/// Get a CSS file by path.
pub fn get_css(path: &str) -> Option<&'static [u8]> {
    let full_path = format!("css/{path}");
    ASSETS.get_file(&full_path).map(|f| f.contents())
}

/// Get a font file by path.
pub fn get_font(path: &str) -> Option<&'static [u8]> {
    let full_path = format!("fonts/{path}");
    ASSETS.get_file(&full_path).map(|f| f.contents())
}

/// Get a vendor file by path.
pub fn get_vendor(path: &str) -> Option<&'static [u8]> {
    let full_path = format!("vendor/{path}");
    ASSETS.get_file(&full_path).map(|f| f.contents())
}

/// Get the icon sprite SVG.
pub fn get_icons() -> Option<&'static [u8]> {
    ASSETS
        .get_file("icons/sustyicons.svg")
        .map(|f| f.contents())
}

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
        "theme": "light",
        "active_table": null,
        "browse_page": 0,
        "browse_page_size": 50,
        "sort_column": null,
        "sort_dir": "ASC",
        "total_rows": 0,
        "execution_time_ms": 0,
        "row_count": 0,
        "schema_filter": "",
        "show_history": true,
        "active_view": "query",
        "audit_action": "",
        "audit_actor": "",
        "filters": []
    }' data-on-load="@post('/studio/init')">
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
                                data-on-change="@post('/studio/select-tenant')">
                            <option value="">Select tenant...</option>
                        </select>
                        <div class="tenant-selector__warning" data-show="$tenant_id === null">
                            ⚠️ Select a tenant to execute queries
                        </div>
                    </div>
                </div>

                <div class="cluster" style="gap: var(--space-xs); align-items: center;">
                    <!-- View Navigation -->
                    <nav class="studio-nav" style="display: flex; gap: 2px; margin-right: var(--space-s);">
                        <button type="button" class="studio-nav__btn" data-bind-data-active="$active_view === 'query'"
                                data-on-click="$active_view = 'query'">Query</button>
                        <button type="button" class="studio-nav__btn" data-bind-data-active="$active_view === 'audit'"
                                data-on-click="$active_view = 'audit'; @post('/studio/audit')">Audit</button>
                        <button type="button" class="studio-nav__btn" data-bind-data-active="$active_view === 'compliance'"
                                data-on-click="$active_view = 'compliance'; @post('/studio/compliance')">Compliance</button>
                    </nav>
                    <a href="/playground" style="color: var(--text-secondary); text-decoration: none; font-size: 13px; opacity: 0.7;">Playground</a>
                    <!-- Theme Toggle -->
                    <button type="button" class="button" data-variant="ghost-light"
                            data-on-click="
                                const next = document.documentElement.getAttribute('data-theme') === 'light' ? 'dark' : 'light';
                                document.documentElement.setAttribute('data-theme', next);
                                document.documentElement.style.colorScheme = next;
                                localStorage.setItem('kimberlite-theme', next);
                                $theme = next;
                            ">
                        <span data-show="$theme === 'light'">&#127769;</span>
                        <span data-show="$theme === 'dark'">&#9728;&#65039;</span>
                    </button>
                </div>
            </div>
        </header>

        <!-- Sidebar (Schema Tree + History) -->
        <aside class="studio-sidebar" data-bind-data-mobile-open="$show_sidebar">
            <div class="flow" data-space="s" style="padding: var(--space-m);">
                <h2 style="font-size: 14px; text-transform: uppercase; letter-spacing: 0.05em; margin: 0;">
                    Schema
                </h2>
                <!-- Schema Search -->
                <input type="text"
                       class="schema-search"
                       data-model="schema_filter"
                       placeholder="Filter tables..."
                       data-on-input="window._filterSchema($schema_filter)">
                <div id="schema-tree">
                    <div class="schema-tree">
                        <div class="schema-tree__item" data-level="0" data-type="info" style="color: var(--text-tertiary); font-style: italic;">
                            Select a tenant to view schema
                        </div>
                    </div>
                </div>

                <!-- Query History -->
                <div class="schema-tree__divider"></div>
                <h2 style="font-size: 14px; text-transform: uppercase; letter-spacing: 0.05em; margin: 0; cursor: pointer; user-select: none;"
                    data-on-click="$show_history = !$show_history">
                    History <span data-text="$show_history ? '&#9660;' : '&#9654;'"></span>
                </h2>
                <div id="query-history" data-show="$show_history">
                    <div class="schema-tree">
                        <div class="schema-tree__item" data-level="0" data-type="info" style="color: var(--text-tertiary); font-style: italic;">
                            No queries yet
                        </div>
                    </div>
                </div>
            </div>
        </aside>

        <!-- Main Content Area -->
        <main class="studio-main">
            <div class="wrapper" data-width="wide">
                <div class="flow" data-space="l" style="padding: var(--space-l) 0;">

                <!-- ═══ QUERY VIEW ═══ -->
                <div data-show="$active_view === 'query'">

                    <!-- Query Editor with Tabs -->
                    <section class="query-editor">
                        <!-- Tab Bar -->
                        <div class="query-tabs" id="query-tabs">
                            <div class="query-tabs__list" id="tab-list">
                                <button type="button" class="query-tabs__tab query-tabs__tab--active" data-tab-id="1">
                                    <span class="query-tabs__tab-name">Query 1</span>
                                </button>
                            </div>
                            <button type="button" class="query-tabs__add" id="add-tab-btn" title="New tab (Ctrl+T)">+</button>
                        </div>
                        <div class="query-editor__container" style="position: relative;">
                            <!-- Syntax highlighting overlay -->
                            <div class="code-editor">
                                <pre class="code-editor__highlight" id="studio-highlight" aria-hidden="true"><code></code></pre>
                                <textarea
                                    class="code-editor__textarea query-editor__textarea"
                                    id="studio-textarea"
                                    data-model="query"
                                    placeholder="SELECT * FROM patients LIMIT 10"
                                    spellcheck="false"
                                    data-on-keydown="
                                        if ((evt.ctrlKey || evt.metaKey) && evt.key === 'Enter') {
                                            evt.preventDefault();
                                            $el.closest('section').querySelector('[data-execute-query]').click();
                                        }
                                        if (evt.key === 'Tab') {
                                            evt.preventDefault();
                                            window._studioComplete && window._studioComplete(evt.target);
                                        }
                                    "
                                    data-on-input="window._highlightSQL(evt.target)"
                                    data-on-scroll="window._syncScroll(evt.target)"></textarea>
                            </div>
                            <div id="studio-completion" class="sql-completion"></div>
                            <div class="query-editor__footer">
                                <span class="query-editor__hint">
                                    <kbd>Ctrl</kbd>+<kbd>Enter</kbd> to execute &middot; <kbd>Tab</kbd> to complete
                                </span>
                                <button type="button"
                                        class="button"
                                        data-variant="primary"
                                        data-execute-query
                                        data-bind-disabled="$tenant_id === null || $loading"
                                        data-on-click="window._saveQueryHistory($query); @post('/studio/query')">
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
                                   data-on-change="@post('/studio/query')">
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
                    <div class="error-banner" style="display: none;" data-show="$error !== null && $error !== '' && $error !== false">
                        <div class="error-banner__title">Error</div>
                        <div class="error-banner__message" data-text="$error"></div>
                    </div>

                    <!-- Results Table -->
                    <section id="results-container">
                        <div class="results-table">
                            <div class="results-table__empty" style="padding: var(--space-l); text-align: center;">
                                <div style="font-size: 15px; margin-bottom: var(--space-xs);">Execute a query to see results</div>
                                <div style="font-size: 12px; color: var(--text-tertiary);">
                                    Try <code style="background: var(--surface-secondary); padding: 2px 6px; border-radius: 3px;">SHOW TABLES</code>
                                    or click a table in the schema tree to browse data
                                </div>
                            </div>
                        </div>
                    </section>

                    <!-- Export buttons (visible when results exist) -->
                    <div class="export-bar" data-show="$row_count > 0">
                        <button type="button" class="button export-bar__btn" data-variant="ghost"
                                data-on-click="window._exportResults('csv')">
                            Export CSV
                        </button>
                        <button type="button" class="button export-bar__btn" data-variant="ghost"
                                data-on-click="window._exportResults('json')">
                            Export JSON
                        </button>
                    </div>

                    <!-- Data Browser (populated when clicking a table in schema tree) -->
                    <section id="browse-container"></section>

                </div><!-- end query view -->

                <!-- ═══ AUDIT VIEW ═══ -->
                <div data-show="$active_view === 'audit'">
                    <div class="flow" data-space="m">
                        <div class="repel" style="align-items: center;">
                            <h2 style="font-size: 18px; margin: 0; font-weight: 600;">Audit Log</h2>
                            <button type="button" class="button" data-variant="ghost"
                                    data-on-click="@post('/studio/audit')">Refresh</button>
                        </div>

                        <!-- Audit Filters -->
                        <div style="display: flex; gap: var(--space-s); flex-wrap: wrap; align-items: flex-end;">
                            <div>
                                <label style="font-size: 11px; text-transform: uppercase; letter-spacing: 0.05em; color: var(--text-secondary); display: block; margin-bottom: 4px;">Action Type</label>
                                <select data-model="audit_action" style="padding: 6px 10px; border: 1px solid var(--border-default); border-radius: 4px; background: var(--surface-base); color: var(--text-primary); font-size: 13px;">
                                    <option value="">All actions</option>
                                    <option value="SELECT">SELECT</option>
                                    <option value="INSERT">INSERT</option>
                                    <option value="UPDATE">UPDATE</option>
                                    <option value="DELETE">DELETE</option>
                                    <option value="CREATE">CREATE</option>
                                    <option value="DROP">DROP</option>
                                    <option value="CONSENT">CONSENT</option>
                                    <option value="ERASURE">ERASURE</option>
                                    <option value="BREACH">BREACH</option>
                                </select>
                            </div>
                            <div>
                                <label style="font-size: 11px; text-transform: uppercase; letter-spacing: 0.05em; color: var(--text-secondary); display: block; margin-bottom: 4px;">Actor</label>
                                <input type="text" data-model="audit_actor" placeholder="Filter by actor..." style="padding: 6px 10px; border: 1px solid var(--border-default); border-radius: 4px; background: var(--surface-base); color: var(--text-primary); font-size: 13px; font-family: var(--font-mono); width: 200px;">
                            </div>
                            <button type="button" class="button" data-variant="primary" style="font-size: 13px;"
                                    data-on-click="@post('/studio/audit')">Apply Filters</button>
                        </div>

                        <!-- Audit Results -->
                        <section id="audit-container">
                            <div class="results-table">
                                <div class="results-table__empty">Select a tenant and click Refresh to view audit events</div>
                            </div>
                        </section>
                    </div>
                </div><!-- end audit view -->

                <!-- ═══ COMPLIANCE VIEW ═══ -->
                <div data-show="$active_view === 'compliance'">
                    <div class="flow" data-space="m">
                        <div class="repel" style="align-items: center;">
                            <h2 style="font-size: 18px; margin: 0; font-weight: 600;">Compliance Dashboard</h2>
                            <button type="button" class="button" data-variant="ghost"
                                    data-on-click="@post('/studio/compliance')">Refresh</button>
                        </div>

                        <!-- Compliance Content -->
                        <section id="compliance-container">
                            <div class="results-table">
                                <div class="results-table__empty">Select a tenant to view compliance status</div>
                            </div>
                        </section>
                    </div>
                </div><!-- end compliance view -->

                </div>
            </div>
        </main>
    </div>

    <!-- Status Bar -->
    <footer class="studio-status-bar">
        <div class="studio-status-bar__left">
            <span class="studio-status-bar__item" data-show="$tenant_id !== null">
                Tenant: <span data-text="$tenant_id"></span>
            </span>
            <span class="studio-status-bar__item" style="text-transform: capitalize;">
                <span data-text="$active_view"></span>
            </span>
            <span class="studio-status-bar__item" data-show="$offset !== null" style="color: oklch(0.55 0.15 260);">
                @ offset <span data-text="$offset"></span>
            </span>
        </div>
        <div class="studio-status-bar__right">
            <span class="studio-status-bar__item" data-show="$row_count > 0">
                <span data-text="$row_count"></span> rows
            </span>
            <span class="studio-status-bar__item" data-show="$execution_time_ms > 0">
                <span data-text="$execution_time_ms"></span>ms
            </span>
            <span class="studio-status-bar__item" style="color: oklch(0.55 0.12 145);">
                &#9670; Immutable Log
            </span>
        </div>
    </footer>

    <!-- Studio styles -->
    <style>
        /* ─── SQL Completion Dropdown ─────────────────────────── */
        .sql-completion {
            position: absolute;
            background: var(--surface-primary, var(--surface-base));
            border: 1px solid var(--border-primary, var(--border-default));
            border-radius: 4px;
            box-shadow: 0 4px 12px rgba(0,0,0,0.15);
            max-height: 200px;
            overflow-y: auto;
            z-index: 100;
            min-width: 220px;
            display: none;
        }
        .sql-completion.visible { display: block; }
        .sql-completion__item {
            padding: 4px 12px;
            cursor: pointer;
            font-size: 13px;
            font-family: var(--font-mono);
            display: flex;
            align-items: center;
            gap: 8px;
        }
        .sql-completion__item:hover,
        .sql-completion__item.selected {
            background: var(--surface-secondary, rgba(0,0,0,0.05));
        }
        .sql-completion__item[data-type="keyword"] { color: var(--text-secondary); }
        .sql-completion__item[data-type="table"] {
            color: var(--text-brand, var(--text-primary));
            font-weight: var(--font-bold);
        }
        .sql-completion__item[data-type="column"] {
            color: var(--accent-default, #6366f1);
        }
        .sql-completion__badge {
            font-size: 9px;
            padding: 1px 4px;
            border-radius: 3px;
            text-transform: uppercase;
            letter-spacing: 0.05em;
            font-weight: 600;
            opacity: 0.7;
        }
        .sql-completion__badge[data-type="keyword"] { background: var(--gray-3, #e5e5e5); color: var(--gray-9, #404040); }
        .sql-completion__badge[data-type="table"] { background: oklch(0.85 0.1 250); color: oklch(0.35 0.1 250); }
        .sql-completion__badge[data-type="column"] { background: oklch(0.85 0.1 290); color: oklch(0.35 0.1 290); }

        /* ─── Code Editor (Syntax Highlighting Overlay) ───────── */
        .code-editor {
            position: relative;
            min-height: 120px;
        }
        .code-editor__highlight {
            position: absolute;
            top: 0; left: 0; right: 0; bottom: 0;
            margin: 0;
            padding: var(--space-s);
            font-family: var(--font-mono);
            font-size: 14px;
            line-height: 1.5;
            white-space: pre-wrap;
            word-wrap: break-word;
            overflow: auto;
            pointer-events: none;
            border: 1px solid transparent;
            color: transparent;
        }
        .code-editor__highlight code {
            font-family: inherit;
            font-size: inherit;
        }
        .code-editor__textarea {
            position: relative;
            background: transparent !important;
            color: transparent;
            caret-color: var(--text-primary);
            resize: vertical;
        }
        /* Show raw text when highlight not active (fallback) */
        .code-editor__textarea:not(:focus):not(.has-content) {
            color: var(--text-primary);
        }
        /* Syntax token colors */
        .sql-keyword { color: oklch(0.55 0.15 260); font-weight: 600; }
        .sql-string { color: oklch(0.55 0.15 145); }
        .sql-number { color: oklch(0.55 0.15 50); }
        .sql-comment { color: var(--text-tertiary); font-style: italic; }
        .sql-table { color: oklch(0.55 0.15 290); font-weight: 500; }
        .sql-function { color: oklch(0.55 0.12 200); }
        .sql-operator { color: var(--text-secondary); }
        .sql-paren { color: var(--text-tertiary); }
        [data-theme="dark"] .sql-keyword { color: oklch(0.75 0.15 260); }
        [data-theme="dark"] .sql-string { color: oklch(0.75 0.15 145); }
        [data-theme="dark"] .sql-number { color: oklch(0.75 0.15 50); }
        [data-theme="dark"] .sql-table { color: oklch(0.75 0.15 290); }
        [data-theme="dark"] .sql-function { color: oklch(0.75 0.12 200); }

        /* ─── Query Tabs ──────────────────────────────────────── */
        .query-tabs {
            display: flex;
            align-items: stretch;
            border-bottom: 1px solid var(--border-default);
            background: var(--surface-secondary, rgba(0,0,0,0.02));
            border-radius: 6px 6px 0 0;
            overflow-x: auto;
        }
        .query-tabs__list {
            display: flex;
            align-items: stretch;
            gap: 0;
            flex: 1;
            overflow-x: auto;
        }
        .query-tabs__tab {
            display: flex;
            align-items: center;
            gap: 6px;
            padding: 6px 14px;
            font-size: 12px;
            font-family: var(--font-mono);
            border: none;
            background: transparent;
            color: var(--text-secondary);
            cursor: pointer;
            border-bottom: 2px solid transparent;
            white-space: nowrap;
            transition: all 0.15s ease;
        }
        .query-tabs__tab:hover { color: var(--text-primary); background: rgba(0,0,0,0.03); }
        .query-tabs__tab--active {
            color: var(--text-primary);
            border-bottom-color: var(--accent-default, #6366f1);
            background: var(--surface-base, #fff);
        }
        .query-tabs__tab-close {
            font-size: 14px;
            line-height: 1;
            opacity: 0;
            transition: opacity 0.15s;
            padding: 0 2px;
            border-radius: 3px;
        }
        .query-tabs__tab:hover .query-tabs__tab-close { opacity: 0.5; }
        .query-tabs__tab-close:hover { opacity: 1 !important; background: rgba(0,0,0,0.1); }
        .query-tabs__add {
            padding: 6px 12px;
            font-size: 16px;
            border: none;
            background: transparent;
            color: var(--text-tertiary);
            cursor: pointer;
            flex-shrink: 0;
        }
        .query-tabs__add:hover { color: var(--text-primary); }

        /* ─── Schema Search ───────────────────────────────────── */
        .schema-search {
            width: 100%;
            padding: var(--space-2xs) var(--space-xs);
            font-size: 13px;
            border: 1px solid var(--border-default);
            border-radius: 4px;
            background: var(--surface-base);
            color: var(--text-primary);
            font-family: var(--font-mono);
        }
        .schema-search:focus {
            outline: 2px solid var(--accent-default);
            outline-offset: -1px;
        }
        .schema-tree__divider {
            height: 1px;
            background: var(--border-default);
            margin: var(--space-s) 0;
        }

        /* ─── Collapsible Schema Columns ──────────────────────── */
        .schema-tree__column {
            display: none;
        }
        .schema-tree__table.expanded + .schema-tree__column,
        .schema-tree__table.expanded ~ .schema-tree__column {
            /* columns shown via JS toggle */
        }

        /* ─── Query History + Saved Queries ───────────────────── */
        .query-history__item {
            padding: var(--space-2xs) var(--space-xs);
            font-size: 12px;
            font-family: var(--font-mono);
            color: var(--text-secondary);
            cursor: pointer;
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
            border-radius: 3px;
            display: flex;
            align-items: center;
            gap: 4px;
        }
        .query-history__item:hover {
            background: var(--surface-secondary, rgba(0,0,0,0.05));
            color: var(--text-primary);
        }
        .query-history__clear {
            font-size: 11px;
            color: var(--text-tertiary);
            cursor: pointer;
            float: right;
        }
        .query-history__clear:hover { color: var(--text-primary); }
        .query-history__pin {
            cursor: pointer;
            font-size: 11px;
            opacity: 0.3;
            flex-shrink: 0;
        }
        .query-history__pin:hover { opacity: 0.8; }
        .query-history__pin.pinned { opacity: 1; color: oklch(0.65 0.15 80); }
        .query-history__text { overflow: hidden; text-overflow: ellipsis; flex: 1; }

        /* ─── Navigation Tabs ─────────────────────────────────── */
        .studio-nav__btn {
            padding: 4px 12px;
            font-size: 12px;
            font-weight: 500;
            border: none;
            background: transparent;
            color: var(--text-secondary);
            cursor: pointer;
            border-radius: 4px;
            transition: all 0.15s ease;
        }
        .studio-nav__btn:hover { color: var(--text-primary); background: rgba(0,0,0,0.05); }
        .studio-nav__btn[data-active="true"] {
            color: var(--text-primary);
            background: var(--surface-base);
            box-shadow: 0 1px 3px rgba(0,0,0,0.1);
            font-weight: 600;
        }

        /* ─── Export Bar ──────────────────────────────────────── */
        .export-bar {
            display: flex;
            gap: var(--space-xs);
            align-items: center;
        }
        .export-bar__btn { font-size: 13px !important; }

        /* ─── Keyboard Shortcuts Modal ────────────────────────── */
        .shortcuts-modal {
            position: fixed;
            top: 0; left: 0; right: 0; bottom: 0;
            background: rgba(0,0,0,0.5);
            z-index: 1000;
            display: flex;
            align-items: center;
            justify-content: center;
        }
        .shortcuts-modal__content {
            background: var(--surface-base);
            border-radius: 8px;
            padding: var(--space-l);
            max-width: 400px;
            width: 90%;
            box-shadow: 0 16px 48px rgba(0,0,0,0.2);
        }
        .shortcuts-modal__title {
            font-size: 16px;
            font-weight: 600;
            margin: 0 0 var(--space-m);
        }
        .shortcuts-modal__row {
            display: flex;
            justify-content: space-between;
            padding: 4px 0;
            font-size: 13px;
        }
        .shortcuts-modal__keys { font-family: var(--font-mono); color: var(--text-secondary); }
    </style>

    <!-- Keyboard shortcuts modal (hidden by default) -->
    <div id="shortcuts-modal" class="shortcuts-modal" style="display: none;" onclick="if(event.target===this) this.style.display='none'">
        <div class="shortcuts-modal__content">
            <h3 class="shortcuts-modal__title">Keyboard Shortcuts</h3>
            <div class="shortcuts-modal__row"><span>Execute query</span><span class="shortcuts-modal__keys">Ctrl+Enter</span></div>
            <div class="shortcuts-modal__row"><span>SQL completion</span><span class="shortcuts-modal__keys">Tab</span></div>
            <div class="shortcuts-modal__row"><span>Focus query editor</span><span class="shortcuts-modal__keys">Cmd/Ctrl+K</span></div>
            <div class="shortcuts-modal__row"><span>New tab</span><span class="shortcuts-modal__keys">Cmd/Ctrl+T</span></div>
            <div class="shortcuts-modal__row"><span>Close tab</span><span class="shortcuts-modal__keys">Cmd/Ctrl+W</span></div>
            <div class="shortcuts-modal__row"><span>Save/pin query</span><span class="shortcuts-modal__keys">Cmd/Ctrl+S</span></div>
            <div class="shortcuts-modal__row"><span>Clear errors</span><span class="shortcuts-modal__keys">Escape</span></div>
            <div class="shortcuts-modal__row"><span>Show shortcuts</span><span class="shortcuts-modal__keys">F1</span></div>
        </div>
    </div>

    <script>
        // ─── SQL Syntax Highlighting Engine ─────────────────────
        const SQL_KEYWORDS = new Set([
            'SELECT', 'FROM', 'WHERE', 'AND', 'OR', 'NOT', 'IN', 'IS', 'NULL',
            'LIKE', 'BETWEEN', 'EXISTS', 'CASE', 'WHEN', 'THEN', 'ELSE', 'END',
            'AS', 'ON', 'JOIN', 'LEFT', 'RIGHT', 'INNER', 'OUTER', 'CROSS',
            'FULL', 'GROUP', 'BY', 'ORDER', 'ASC', 'DESC', 'HAVING', 'LIMIT',
            'OFFSET', 'UNION', 'ALL', 'DISTINCT', 'TRUE', 'FALSE',
            'INSERT', 'INTO', 'VALUES', 'UPDATE', 'SET', 'DELETE',
            'CREATE', 'TABLE', 'DROP', 'ALTER', 'SHOW', 'TABLES', 'COLUMNS',
            'WITH', 'RECURSIVE', 'EXPLAIN', 'AT'
        ]);
        const SQL_FUNCTIONS = new Set([
            'COUNT', 'SUM', 'AVG', 'MIN', 'MAX', 'COALESCE', 'NULLIF',
            'CAST', 'EXTRACT', 'UPPER', 'LOWER', 'TRIM', 'LENGTH',
            'SUBSTRING', 'REPLACE', 'CONCAT', 'ABS', 'ROUND', 'NOW'
        ]);

        window.STUDIO_TABLES = [];
        window.STUDIO_SCHEMA = {};

        function _escapeHtml(s) {
            return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
        }

        window._highlightSQL = function(textarea) {
            const highlight = document.getElementById('studio-highlight');
            if (!highlight) return;
            const code = highlight.querySelector('code');
            const text = textarea.value;
            if (!text) { code.innerHTML = ''; textarea.classList.remove('has-content'); return; }
            textarea.classList.add('has-content');

            const tableSet = new Set(window.STUDIO_TABLES.map(t => t.toUpperCase()));
            let html = '';
            // Tokenize: strings, comments, words, numbers, operators, whitespace
            const re = /('(?:[^'\\]|\\.)*'|"(?:[^"\\]|\\.)*"|--[^\n]*|\b\d+(?:\.\d+)?\b|\b\w+\b|[(),.;*=<>!]+|\s+)/g;
            let m;
            while ((m = re.exec(text)) !== null) {
                const tok = m[0];
                if (tok.startsWith("'") || tok.startsWith('"')) {
                    html += '<span class="sql-string">' + _escapeHtml(tok) + '</span>';
                } else if (tok.startsWith('--')) {
                    html += '<span class="sql-comment">' + _escapeHtml(tok) + '</span>';
                } else if (/^\d/.test(tok)) {
                    html += '<span class="sql-number">' + _escapeHtml(tok) + '</span>';
                } else if (/^\w+$/.test(tok)) {
                    const upper = tok.toUpperCase();
                    if (SQL_KEYWORDS.has(upper)) {
                        html += '<span class="sql-keyword">' + _escapeHtml(tok) + '</span>';
                    } else if (SQL_FUNCTIONS.has(upper)) {
                        html += '<span class="sql-function">' + _escapeHtml(tok) + '</span>';
                    } else if (tableSet.has(upper)) {
                        html += '<span class="sql-table">' + _escapeHtml(tok) + '</span>';
                    } else {
                        html += _escapeHtml(tok);
                    }
                } else if (/^[(),.;]/.test(tok)) {
                    html += '<span class="sql-paren">' + _escapeHtml(tok) + '</span>';
                } else if (/^[=<>!*]+$/.test(tok)) {
                    html += '<span class="sql-operator">' + _escapeHtml(tok) + '</span>';
                } else {
                    html += _escapeHtml(tok);
                }
            }
            // Ensure trailing newline for proper height sync
            if (text.endsWith('\n')) html += '\n';
            code.innerHTML = html;
        };

        window._syncScroll = function(textarea) {
            const highlight = document.getElementById('studio-highlight');
            if (highlight) {
                highlight.scrollTop = textarea.scrollTop;
                highlight.scrollLeft = textarea.scrollLeft;
            }
        };

        // ─── Query Tabs ────────────────────────────────────────
        const _tabs = [{ id: 1, name: 'Query 1', query: '' }];
        let _activeTabId = 1;
        let _tabCounter = 1;

        function _renderTabs() {
            const list = document.getElementById('tab-list');
            if (!list) return;
            list.innerHTML = '';
            _tabs.forEach(tab => {
                const btn = document.createElement('button');
                btn.type = 'button';
                btn.className = 'query-tabs__tab' + (tab.id === _activeTabId ? ' query-tabs__tab--active' : '');
                btn.setAttribute('data-tab-id', tab.id);
                btn.innerHTML = '<span class="query-tabs__tab-name">' + _escapeHtml(tab.name) + '</span>';
                if (_tabs.length > 1) {
                    const close = document.createElement('span');
                    close.className = 'query-tabs__tab-close';
                    close.textContent = '\u00d7';
                    close.addEventListener('click', (e) => { e.stopPropagation(); _closeTab(tab.id); });
                    btn.appendChild(close);
                }
                btn.addEventListener('click', () => _switchTab(tab.id));
                list.appendChild(btn);
            });
        }

        function _switchTab(id) {
            // Save current query to current tab
            const textarea = document.getElementById('studio-textarea');
            const currentTab = _tabs.find(t => t.id === _activeTabId);
            if (currentTab && textarea) currentTab.query = textarea.value;

            _activeTabId = id;
            const tab = _tabs.find(t => t.id === id);
            if (tab && textarea) {
                textarea.value = tab.query;
                textarea.dispatchEvent(new Event('input', { bubbles: true }));
                window._highlightSQL(textarea);
            }
            _renderTabs();
        }

        function _addTab() {
            const textarea = document.getElementById('studio-textarea');
            const currentTab = _tabs.find(t => t.id === _activeTabId);
            if (currentTab && textarea) currentTab.query = textarea.value;

            _tabCounter++;
            const tab = { id: _tabCounter, name: 'Query ' + _tabCounter, query: '' };
            _tabs.push(tab);
            _activeTabId = tab.id;
            if (textarea) {
                textarea.value = '';
                textarea.dispatchEvent(new Event('input', { bubbles: true }));
                window._highlightSQL(textarea);
                textarea.focus();
            }
            _renderTabs();
        }

        function _closeTab(id) {
            if (_tabs.length <= 1) return;
            const idx = _tabs.findIndex(t => t.id === id);
            if (idx === -1) return;
            _tabs.splice(idx, 1);
            if (_activeTabId === id) {
                _activeTabId = _tabs[Math.min(idx, _tabs.length - 1)].id;
                const tab = _tabs.find(t => t.id === _activeTabId);
                const textarea = document.getElementById('studio-textarea');
                if (tab && textarea) {
                    textarea.value = tab.query;
                    textarea.dispatchEvent(new Event('input', { bubbles: true }));
                    window._highlightSQL(textarea);
                }
            }
            _renderTabs();
        }

        document.getElementById('add-tab-btn')?.addEventListener('click', _addTab);

        // ─── Column-Aware SQL Completion ────────────────────────
        window._studioComplete = function(textarea) {
            const pos = textarea.selectionStart;
            const text = textarea.value.substring(0, pos);

            // Check for dot-notation: tablename.
            const dotMatch = text.match(/(\w+)\.(\w*)$/);
            if (dotMatch) {
                const tableName = dotMatch[1];
                const colPrefix = dotMatch[2].toUpperCase();
                const columns = window.STUDIO_SCHEMA[tableName] || window.STUDIO_SCHEMA[tableName.toLowerCase()] || [];
                const candidates = columns
                    .filter(c => !colPrefix || c.toUpperCase().startsWith(colPrefix))
                    .map(c => ({ name: c, type: 'column' }));
                if (candidates.length > 0) {
                    _showCompletionDropdown(textarea, pos, dotMatch[2], candidates);
                }
                return;
            }

            // Regular word completion
            const wordMatch = text.match(/(\w+)$/);
            if (!wordMatch) return;

            const prefix = wordMatch[1].toUpperCase();
            // Detect context: after FROM/JOIN, prefer tables; otherwise mix all
            const contextMatch = text.match(/(?:FROM|JOIN|INTO|UPDATE|TABLE)\s+\w*$/i);

            let candidates = [];
            if (contextMatch) {
                // Table context — tables first, then keywords
                candidates = [
                    ...window.STUDIO_TABLES.map(t => ({ name: t, type: 'table' })),
                    ...[...SQL_KEYWORDS].map(k => ({ name: k, type: 'keyword' })),
                ];
            } else {
                // Check if any table columns match (from tables in the query)
                const tablesInQuery = _extractTablesFromQuery(text);
                const columnCandidates = [];
                for (const tbl of tablesInQuery) {
                    const cols = window.STUDIO_SCHEMA[tbl] || [];
                    for (const col of cols) {
                        if (col.toUpperCase().startsWith(prefix)) {
                            columnCandidates.push({ name: col, type: 'column' });
                        }
                    }
                }
                candidates = [
                    ...columnCandidates,
                    ...window.STUDIO_TABLES.map(t => ({ name: t, type: 'table' })),
                    ...[...SQL_KEYWORDS].map(k => ({ name: k, type: 'keyword' })),
                    ...[...SQL_FUNCTIONS].map(k => ({ name: k, type: 'keyword' })),
                ];
            }

            candidates = candidates.filter(c =>
                c.name.toUpperCase().startsWith(prefix) && c.name.toUpperCase() !== prefix
            );
            // Deduplicate
            const seen = new Set();
            candidates = candidates.filter(c => {
                const key = c.name.toUpperCase();
                if (seen.has(key)) return false;
                seen.add(key);
                return true;
            });

            if (candidates.length === 0) return;

            if (candidates.length === 1) {
                _applyCompletion(textarea, pos, wordMatch[1], candidates[0].name);
                return;
            }

            _showCompletionDropdown(textarea, pos, wordMatch[1], candidates);
        };

        function _extractTablesFromQuery(sql) {
            const tables = [];
            const re = /(?:FROM|JOIN|INTO|UPDATE)\s+(\w+)/gi;
            let m;
            while ((m = re.exec(sql)) !== null) {
                tables.push(m[1]);
            }
            return tables;
        }

        function _applyCompletion(textarea, pos, prefix, completion) {
            const before = textarea.value.substring(0, pos - prefix.length);
            const after = textarea.value.substring(pos);
            const insert = prefix[0] === prefix[0].toLowerCase() ? completion.toLowerCase() : completion;
            textarea.value = before + insert + ' ' + after;
            textarea.selectionStart = textarea.selectionEnd = before.length + insert.length + 1;
            textarea.dispatchEvent(new Event('input', { bubbles: true }));
            window._highlightSQL(textarea);
        }

        function _showCompletionDropdown(textarea, pos, prefix, candidates) {
            const dropdown = document.getElementById('studio-completion');
            dropdown.innerHTML = '';
            candidates.slice(0, 12).forEach((c, i) => {
                const div = document.createElement('div');
                div.className = 'sql-completion__item' + (i === 0 ? ' selected' : '');
                div.setAttribute('data-type', c.type);
                // Badge + name
                const badge = document.createElement('span');
                badge.className = 'sql-completion__badge';
                badge.setAttribute('data-type', c.type);
                badge.textContent = c.type === 'keyword' ? 'KW' : c.type === 'table' ? 'TBL' : 'COL';
                div.appendChild(badge);
                const nameSpan = document.createElement('span');
                nameSpan.textContent = c.name;
                div.appendChild(nameSpan);

                div.addEventListener('mousedown', (e) => {
                    e.preventDefault();
                    _applyCompletion(textarea, pos, prefix, c.name);
                    dropdown.classList.remove('visible');
                });
                dropdown.appendChild(div);
            });

            dropdown.style.left = '12px';
            dropdown.style.bottom = '50px';
            dropdown.classList.add('visible');

            const hide = () => {
                dropdown.classList.remove('visible');
                textarea.removeEventListener('keydown', hideHandler);
                document.removeEventListener('click', hide);
            };
            const hideHandler = (e) => { if (e.key !== 'Tab') hide(); };
            setTimeout(() => {
                textarea.addEventListener('keydown', hideHandler, { once: true });
                document.addEventListener('click', hide, { once: true });
            }, 0);
        }

        // ─── Insert at Cursor ──────────────────────────────────
        window._insertAtCursor = function(text) {
            const textarea = document.getElementById('studio-textarea');
            if (!textarea) return;
            const pos = textarea.selectionStart;
            const before = textarea.value.substring(0, pos);
            const after = textarea.value.substring(textarea.selectionEnd);
            textarea.value = before + text + after;
            textarea.selectionStart = textarea.selectionEnd = pos + text.length;
            textarea.dispatchEvent(new Event('input', { bubbles: true }));
            window._highlightSQL(textarea);
            textarea.focus();
        };

        // ─── Schema Tree Toggle ────────────────────────────────
        document.addEventListener('click', function(e) {
            const tableItem = e.target.closest('.schema-tree__table');
            if (!tableItem) return;
            // Don't toggle if the click will trigger browse (handled by Datastar)
            // Just toggle the column visibility
            const isExpanded = tableItem.classList.contains('expanded');
            const tableName = tableItem.querySelector('.schema-tree__label')?.textContent;
            if (!tableName) return;
            const tree = tableItem.closest('.schema-tree') || document.getElementById('schema-tree');
            if (!tree) return;
            const columns = tree.querySelectorAll('.schema-tree__column[data-parent="' + tableName + '"]');
            if (isExpanded) {
                tableItem.classList.remove('expanded');
                columns.forEach(col => col.style.display = 'none');
                const toggle = tableItem.querySelector('.schema-tree__toggle');
                if (toggle) toggle.innerHTML = '&#9654;';
            } else {
                tableItem.classList.add('expanded');
                columns.forEach(col => col.style.display = '');
                const toggle = tableItem.querySelector('.schema-tree__toggle');
                if (toggle) toggle.innerHTML = '&#9660;';
            }
        });

        // ─── Schema Filter ──────────────────────────────────────
        window._filterSchema = function(filter) {
            const tree = document.getElementById('schema-tree');
            if (!tree) return;
            const lowerFilter = (filter || '').toLowerCase();
            tree.querySelectorAll('.schema-tree__table').forEach(el => {
                const name = (el.querySelector('.schema-tree__label')?.textContent || '').toLowerCase();
                const match = !lowerFilter || name.includes(lowerFilter);
                el.style.display = match ? '' : 'none';
                const tableName = el.querySelector('.schema-tree__label')?.textContent || '';
                tree.querySelectorAll('.schema-tree__column[data-parent="' + tableName + '"]').forEach(col => {
                    col.style.display = (match && el.classList.contains('expanded')) ? '' : 'none';
                });
            });
        };

        // ─── Query History with Pinned Queries ──────────────────
        const HISTORY_KEY = 'kimberlite-studio-history';
        const PINS_KEY = 'kimberlite-studio-pins';
        const MAX_HISTORY = 50;

        function _loadHistory() {
            try { return JSON.parse(localStorage.getItem(HISTORY_KEY) || '[]'); }
            catch { return []; }
        }
        function _loadPins() {
            try { return JSON.parse(localStorage.getItem(PINS_KEY) || '[]'); }
            catch { return []; }
        }

        function _renderHistory() {
            const container = document.getElementById('query-history');
            if (!container) return;
            const pins = _loadPins();
            const history = _loadHistory();

            if (pins.length === 0 && history.length === 0) {
                container.innerHTML = '<div class="schema-tree"><div class="schema-tree__item" data-level="0" data-type="info" style="color: var(--text-tertiary); font-style: italic;">No queries yet</div></div>';
                return;
            }

            let html = '';
            if (pins.length > 0) {
                html += '<div style="font-size: 11px; color: var(--text-tertiary); text-transform: uppercase; letter-spacing: 0.05em; margin-bottom: 2px;">Saved</div>';
                pins.forEach((q, i) => {
                    const escaped = _escapeHtml(q).replace(/"/g, '&quot;');
                    const truncated = q.length > 55 ? q.substring(0, 55) + '...' : q;
                    html += '<div class="query-history__item" title="' + escaped + '">';
                    html += '<span class="query-history__pin pinned" onclick="event.stopPropagation(); window._unpinQuery(' + i + ')">&#9733;</span>';
                    html += '<span class="query-history__text" onclick="window._loadQuery(\'' + escaped.replace(/'/g, "\\'") + '\')">' + _escapeHtml(truncated) + '</span>';
                    html += '</div>';
                });
                html += '<div style="height: 1px; background: var(--border-default); margin: 4px 0;"></div>';
            }

            if (history.length > 0) {
                html += '<div style="margin-bottom: 2px; display: flex; justify-content: space-between; align-items: center;"><span style="font-size: 11px; color: var(--text-tertiary); text-transform: uppercase; letter-spacing: 0.05em;">Recent</span><span class="query-history__clear" onclick="window._clearHistory()">Clear</span></div>';
                history.forEach((q, i) => {
                    const escaped = _escapeHtml(q).replace(/"/g, '&quot;');
                    const truncated = q.length > 55 ? q.substring(0, 55) + '...' : q;
                    html += '<div class="query-history__item" title="' + escaped + '">';
                    html += '<span class="query-history__pin" onclick="event.stopPropagation(); window._pinQuery(' + i + ')">&#9734;</span>';
                    html += '<span class="query-history__text" onclick="window._loadQuery(\'' + escaped.replace(/'/g, "\\'") + '\')">' + _escapeHtml(truncated) + '</span>';
                    html += '</div>';
                });
            }
            container.innerHTML = html;
        }

        window._saveQueryHistory = function(query) {
            if (!query || !query.trim()) return;
            const history = _loadHistory();
            const idx = history.indexOf(query.trim());
            if (idx !== -1) history.splice(idx, 1);
            history.unshift(query.trim());
            if (history.length > MAX_HISTORY) history.length = MAX_HISTORY;
            localStorage.setItem(HISTORY_KEY, JSON.stringify(history));
            _renderHistory();
        };

        window._loadQuery = function(query) {
            const textarea = document.getElementById('studio-textarea');
            if (textarea) {
                textarea.value = query;
                textarea.dispatchEvent(new Event('input', { bubbles: true }));
                window._highlightSQL(textarea);
                textarea.focus();
            }
        };

        window._loadFromHistory = function(index) {
            const history = _loadHistory();
            if (history[index]) window._loadQuery(history[index]);
        };

        window._pinQuery = function(index) {
            const history = _loadHistory();
            if (!history[index]) return;
            const pins = _loadPins();
            const query = history[index];
            if (!pins.includes(query)) {
                pins.unshift(query);
                localStorage.setItem(PINS_KEY, JSON.stringify(pins));
            }
            _renderHistory();
        };

        window._unpinQuery = function(index) {
            const pins = _loadPins();
            if (index >= 0 && index < pins.length) {
                pins.splice(index, 1);
                localStorage.setItem(PINS_KEY, JSON.stringify(pins));
            }
            _renderHistory();
        };

        window._clearHistory = function() {
            localStorage.removeItem(HISTORY_KEY);
            _renderHistory();
        };

        _renderHistory();

        // ─── Export ─────────────────────────────────────────────
        window._exportResults = function(format) {
            const signals = window.ds?.store || {};
            const tenantId = signals.tenant_id;
            const query = signals.query;
            if (!tenantId || !query) return;
            const params = new URLSearchParams({ tenant_id: tenantId, query: query, format: format });
            window.open('/studio/export?' + params.toString(), '_blank');
        };

        // ─── Keyboard Shortcuts ─────────────────────────────────
        document.addEventListener('keydown', (e) => {
            // Cmd/Ctrl+K: Focus query editor
            if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
                e.preventDefault();
                document.getElementById('studio-textarea')?.focus();
            }
            // Cmd/Ctrl+T: New tab
            if ((e.metaKey || e.ctrlKey) && e.key === 't') {
                e.preventDefault();
                _addTab();
            }
            // Cmd/Ctrl+W: Close tab
            if ((e.metaKey || e.ctrlKey) && e.key === 'w') {
                if (_tabs.length > 1) {
                    e.preventDefault();
                    _closeTab(_activeTabId);
                }
            }
            // Cmd/Ctrl+S: Pin current query
            if ((e.metaKey || e.ctrlKey) && e.key === 's') {
                e.preventDefault();
                const textarea = document.getElementById('studio-textarea');
                if (textarea && textarea.value.trim()) {
                    const pins = _loadPins();
                    const q = textarea.value.trim();
                    if (!pins.includes(q)) {
                        pins.unshift(q);
                        localStorage.setItem(PINS_KEY, JSON.stringify(pins));
                        _renderHistory();
                    }
                }
            }
            // Escape: Clear errors / close dropdowns
            if (e.key === 'Escape') {
                document.getElementById('studio-completion')?.classList.remove('visible');
                document.getElementById('shortcuts-modal').style.display = 'none';
                const signals = window.ds?.store;
                if (signals) signals.error = null;
            }
            // F1: Show shortcuts
            if (e.key === 'F1') {
                e.preventDefault();
                const modal = document.getElementById('shortcuts-modal');
                modal.style.display = modal.style.display === 'none' ? 'flex' : 'none';
            }
        });

        // Initial highlight
        setTimeout(() => {
            const textarea = document.getElementById('studio-textarea');
            if (textarea && textarea.value) window._highlightSQL(textarea);
        }, 100);

        console.log('Kimberlite Studio initialized');
        console.log('Press F1 for keyboard shortcuts');
    </script>
</body>
</html>
"#;

/// Get playground.html
pub const PLAYGROUND_HTML: &str = r#"<!DOCTYPE html>
<html lang="en" data-theme="light">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Kimberlite Playground</title>

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

    <style>
        /* Playground-specific styles */
        .playground-verticals {
            display: flex;
            gap: var(--space-xs);
            flex-wrap: wrap;
        }
        .playground-verticals .button {
            flex: 1;
            min-width: 140px;
            text-align: center;
        }
        .playground-verticals .button[data-active="true"] {
            background: var(--surface-brand);
            color: var(--text-on-brand, #fff);
            border-color: var(--surface-brand);
        }
        .playground-meta {
            display: flex;
            gap: var(--space-m);
            align-items: center;
            font-size: 13px;
            color: var(--text-secondary);
            padding: var(--space-xs) 0;
        }
        .playground-meta__item {
            display: flex;
            align-items: center;
            gap: var(--space-2xs);
        }
        .playground-badge {
            display: inline-block;
            padding: 2px 8px;
            border-radius: 4px;
            font-size: 11px;
            font-weight: var(--font-bold);
            text-transform: uppercase;
            letter-spacing: 0.05em;
        }
        .playground-badge[data-variant="read-only"] {
            background: var(--surface-success, #dcfce7);
            color: var(--text-success, #166534);
        }
        .playground-example {
            font-size: 13px !important;
            text-align: left !important;
            justify-content: flex-start !important;
            white-space: nowrap;
            overflow: hidden;
            text-overflow: ellipsis;
            display: block !important;
            width: 100%;
        }
        #playground-examples {
            max-height: 300px;
            overflow-y: auto;
        }
        .query-editor__textarea {
            min-height: 120px;
        }
        .playground-header-link {
            color: var(--text-secondary);
            text-decoration: none;
            font-size: 14px;
        }
        .playground-header-link:hover {
            color: var(--text-primary);
        }

        /* SQL completion dropdown */
        .sql-completion {
            position: absolute;
            background: var(--surface-primary);
            border: 1px solid var(--border-primary);
            border-radius: 4px;
            box-shadow: 0 4px 12px rgba(0,0,0,0.15);
            max-height: 200px;
            overflow-y: auto;
            z-index: 100;
            min-width: 180px;
            display: none;
        }
        .sql-completion.visible {
            display: block;
        }
        .sql-completion__item {
            padding: 4px 12px;
            cursor: pointer;
            font-size: 13px;
            font-family: var(--font-mono);
        }
        .sql-completion__item:hover,
        .sql-completion__item.selected {
            background: var(--surface-secondary);
        }
        .sql-completion__item[data-type="keyword"] {
            color: var(--text-secondary);
        }
        .sql-completion__item[data-type="table"] {
            color: var(--text-brand, var(--text-primary));
            font-weight: var(--font-bold);
        }
    </style>
</head>
<body>
    <!-- Main layout grid -->
    <div class="studio-layout" data-signals='{
        "vertical": "",
        "query": "",
        "loading": false,
        "error": null,
        "initialized": false,
        "execution_time_ms": 0,
        "row_count": 0,
        "theme": "light"
    }'>
        <!-- Header -->
        <header class="studio-header">
            <div class="repel" style="padding: var(--space-s) var(--space-m); align-items: center;">
                <div class="cluster" style="gap: var(--space-m); align-items: center;">
                    <h1 style="font-size: 20px; margin: 0; font-weight: var(--font-bold);">Kimberlite Playground</h1>
                    <span class="playground-badge" data-variant="read-only">Read-Only</span>
                </div>

                <div class="cluster" style="gap: var(--space-xs);">
                    <a href="/" class="playground-header-link">Studio</a>
                    <!-- Theme Toggle -->
                    <button type="button" class="button" data-variant="ghost-light"
                            data-on-click="
                                const next = document.documentElement.getAttribute('data-theme') === 'light' ? 'dark' : 'light';
                                document.documentElement.setAttribute('data-theme', next);
                                document.documentElement.style.colorScheme = next;
                                localStorage.setItem('kimberlite-theme', next);
                                $theme = next;
                            ">
                        <span data-show="$theme === 'light'">&#127769;</span>
                        <span data-show="$theme === 'dark'">&#9728;&#65039;</span>
                    </button>
                </div>
            </div>
        </header>

        <!-- Sidebar (Schema Tree + Examples) -->
        <aside class="studio-sidebar">
            <div class="flow" data-space="s" style="padding: var(--space-m);">
                <h2 style="font-size: 14px; text-transform: uppercase; letter-spacing: 0.05em; margin: 0;">
                    Schema
                </h2>
                <div id="playground-schema">
                    <div class="schema-tree">
                        <div class="schema-tree__item" data-level="0" data-type="info" style="color: var(--text-tertiary); font-style: italic;">
                            Select a vertical to view schema
                        </div>
                    </div>
                </div>

                <div id="playground-examples">
                    <!-- Example query buttons injected by init_vertical -->
                </div>
            </div>
        </aside>

        <!-- Main Content Area -->
        <main class="studio-main">
            <div class="wrapper" data-width="wide">
                <div class="flow" data-space="l" style="padding: var(--space-l) 0;">

                    <!-- Vertical Selector -->
                    <section>
                        <h2 style="font-size: 14px; text-transform: uppercase; letter-spacing: 0.05em; margin: 0 0 var(--space-s) 0; color: var(--text-secondary);">
                            Choose a compliance vertical
                        </h2>
                        <div class="playground-verticals">
                            <button type="button" class="button" data-variant="outline"
                                    data-bind-data-active="$vertical === 'healthcare'"
                                    data-on-click="$vertical = 'healthcare'; @post('/playground/init')">
                                Healthcare (HIPAA)
                            </button>
                            <button type="button" class="button" data-variant="outline"
                                    data-bind-data-active="$vertical === 'finance'"
                                    data-on-click="$vertical = 'finance'; @post('/playground/init')">
                                Finance (SEC/SOX)
                            </button>
                            <button type="button" class="button" data-variant="outline"
                                    data-bind-data-active="$vertical === 'legal'"
                                    data-on-click="$vertical = 'legal'; @post('/playground/init')">
                                Legal (eDiscovery)
                            </button>
                        </div>
                    </section>

                    <!-- Query Editor -->
                    <section class="query-editor" data-show="$initialized">
                        <div class="query-editor__container" style="position: relative;">
                            <div class="query-editor__header">
                                <h2 class="query-editor__title">SQL Query</h2>
                            </div>
                            <textarea
                                class="query-editor__textarea"
                                id="playground-textarea"
                                data-model="query"
                                data-attr-disabled="!$initialized"
                                placeholder="SELECT * FROM patients LIMIT 10;"
                                data-on-keydown="
                                    if ((evt.ctrlKey || evt.metaKey) && evt.key === 'Enter') {
                                        evt.preventDefault();
                                        if ($initialized && !$loading) {
                                            @post('/playground/query');
                                        }
                                    }
                                    if (evt.key === 'Tab') {
                                        evt.preventDefault();
                                        window._pgComplete && window._pgComplete(evt.target);
                                    }
                                "></textarea>
                            <div id="sql-completion" class="sql-completion"></div>
                            <div class="query-editor__footer">
                                <span class="query-editor__hint">
                                    <kbd>Ctrl</kbd>+<kbd>Enter</kbd> to execute &middot; <kbd>Tab</kbd> to complete
                                </span>
                                <button type="button"
                                        class="button"
                                        data-variant="primary"
                                        data-attr-disabled="$loading || !$initialized"
                                        data-on-click="@post('/playground/query')">
                                    <span data-show="!$loading">Execute (Ctrl+Enter)</span>
                                    <span data-show="$loading">
                                        <span class="loading-spinner"></span> Running...
                                    </span>
                                </button>
                            </div>
                        </div>
                    </section>

                    <!-- Metadata bar -->
                    <div class="playground-meta" data-show="$row_count > 0">
                        <div class="playground-meta__item">
                            <span data-text="$row_count"></span> rows
                        </div>
                        <div class="playground-meta__item">
                            <span data-text="$execution_time_ms"></span>ms
                        </div>
                    </div>

                    <!-- Error Banner -->
                    <div class="error-banner" style="display: none;" data-show="$error !== null && $error !== '' && $error !== false">
                        <div class="error-banner__title">Error</div>
                        <div class="error-banner__message" data-text="$error"></div>
                    </div>

                    <!-- Results (replaced by PatchElements from server) -->
                    <section id="playground-results">
                        <div class="results-table">
                            <div class="results-table__empty">
                                Select a vertical to get started
                            </div>
                        </div>
                    </section>

                </div>
            </div>
        </main>
    </div>

    <!-- SQL completion engine -->
    <script>
        const SQL_KEYWORDS = [
            'SELECT', 'FROM', 'WHERE', 'AND', 'OR', 'NOT', 'IN', 'IS', 'NULL',
            'LIKE', 'BETWEEN', 'EXISTS', 'CASE', 'WHEN', 'THEN', 'ELSE', 'END',
            'AS', 'ON', 'JOIN', 'LEFT', 'RIGHT', 'INNER', 'OUTER', 'CROSS',
            'FULL', 'GROUP', 'BY', 'ORDER', 'ASC', 'DESC', 'HAVING', 'LIMIT',
            'OFFSET', 'UNION', 'ALL', 'DISTINCT', 'COUNT', 'SUM', 'AVG', 'MIN',
            'MAX', 'WITH', 'RECURSIVE', 'EXPLAIN', 'TRUE', 'FALSE', 'COALESCE',
            'NULLIF', 'CAST', 'EXTRACT'
        ];

        window.PLAYGROUND_TABLES = [];

        window._pgComplete = function(textarea) {
            const pos = textarea.selectionStart;
            const text = textarea.value.substring(0, pos);
            const match = text.match(/(\w+)$/);
            if (!match) return;

            const prefix = match[1].toUpperCase();
            const candidates = [
                ...window.PLAYGROUND_TABLES.map(t => ({ name: t, type: 'table' })),
                ...SQL_KEYWORDS.map(k => ({ name: k, type: 'keyword' })),
            ].filter(c => c.name.toUpperCase().startsWith(prefix) && c.name.toUpperCase() !== prefix);

            if (candidates.length === 0) return;

            if (candidates.length === 1) {
                // Direct insert
                const completion = candidates[0].name;
                const before = textarea.value.substring(0, pos - match[1].length);
                const after = textarea.value.substring(pos);
                // Preserve case: if user typed lowercase, insert lowercase for tables
                const insert = match[1][0] === match[1][0].toLowerCase()
                    ? completion.toLowerCase() : completion;
                textarea.value = before + insert + ' ' + after;
                textarea.selectionStart = textarea.selectionEnd = before.length + insert.length + 1;
                textarea.dispatchEvent(new Event('input', { bubbles: true }));
                return;
            }

            // Show dropdown
            const dropdown = document.getElementById('sql-completion');
            dropdown.innerHTML = '';
            const maxShow = 10;
            candidates.slice(0, maxShow).forEach((c, i) => {
                const div = document.createElement('div');
                div.className = 'sql-completion__item' + (i === 0 ? ' selected' : '');
                div.setAttribute('data-type', c.type);
                div.textContent = c.name;
                div.addEventListener('mousedown', (e) => {
                    e.preventDefault();
                    const before = textarea.value.substring(0, pos - match[1].length);
                    const after = textarea.value.substring(pos);
                    const insert = match[1][0] === match[1][0].toLowerCase()
                        ? c.name.toLowerCase() : c.name;
                    textarea.value = before + insert + ' ' + after;
                    textarea.selectionStart = textarea.selectionEnd = before.length + insert.length + 1;
                    textarea.dispatchEvent(new Event('input', { bubbles: true }));
                    dropdown.classList.remove('visible');
                });
                dropdown.appendChild(div);
            });

            // Position dropdown near cursor
            dropdown.style.left = '12px';
            dropdown.style.bottom = '50px';
            dropdown.classList.add('visible');

            // Hide on next keydown or click outside
            const hide = () => {
                dropdown.classList.remove('visible');
                textarea.removeEventListener('keydown', hideHandler);
                document.removeEventListener('click', hide);
            };
            const hideHandler = (e) => {
                if (e.key !== 'Tab') hide();
            };
            setTimeout(() => {
                textarea.addEventListener('keydown', hideHandler, { once: true });
                document.addEventListener('click', hide, { once: true });
            }, 0);
        };

        // Keyboard shortcuts
        document.addEventListener('keydown', (e) => {
            if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
                e.preventDefault();
                document.getElementById('playground-textarea')?.focus();
            }
            if (e.key === 'Escape') {
                document.getElementById('sql-completion')?.classList.remove('visible');
            }
        });

        console.log('Kimberlite Playground initialized');
        console.log('Keyboard shortcuts:');
        console.log('  Ctrl+Enter: Execute query');
        console.log('  Tab: SQL completion');
        console.log('  Cmd/Ctrl+K: Focus query editor');
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

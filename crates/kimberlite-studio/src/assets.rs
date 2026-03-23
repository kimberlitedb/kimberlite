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
        "show_history": true
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

                <div class="cluster" style="gap: var(--space-xs);">
                    <a href="/playground" class="playground-header-link" style="color: var(--text-secondary); text-decoration: none; font-size: 14px;">Playground</a>
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

                    <!-- Query Editor -->
                    <section class="query-editor">
                        <div class="query-editor__container" style="position: relative;">
                            <div class="query-editor__header">
                                <h2 class="query-editor__title">SQL Query</h2>
                            </div>
                            <textarea
                                class="query-editor__textarea"
                                id="studio-textarea"
                                data-model="query"
                                placeholder="SELECT * FROM patients LIMIT 10"
                                data-on-keydown="
                                    if ((evt.ctrlKey || evt.metaKey) && evt.key === 'Enter') {
                                        evt.preventDefault();
                                        $el.closest('section').querySelector('[data-execute-query]').click();
                                    }
                                    if (evt.key === 'Tab') {
                                        evt.preventDefault();
                                        window._studioComplete && window._studioComplete(evt.target);
                                    }
                                "></textarea>
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
                    <div class="error-banner" style="display: none;" data-show="$error !== null">
                        <div class="error-banner__title">Error</div>
                        <div class="error-banner__message" data-text="$error"></div>
                    </div>

                    <!-- Results Table -->
                    <section id="results-container">
                        <div class="results-table">
                            <div class="results-table__empty">
                                Execute a query to see results
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
        </div>
        <div class="studio-status-bar__right">
            <span class="studio-status-bar__item" data-show="$row_count > 0">
                <span data-text="$row_count"></span> rows
            </span>
            <span class="studio-status-bar__item" data-show="$execution_time_ms > 0">
                <span data-text="$execution_time_ms"></span>ms
            </span>
        </div>
    </footer>

    <!-- SQL completion + query history + schema filter + export -->
    <style>
        .sql-completion {
            position: absolute;
            background: var(--surface-primary, var(--surface-base));
            border: 1px solid var(--border-primary, var(--border-default));
            border-radius: 4px;
            box-shadow: 0 4px 12px rgba(0,0,0,0.15);
            max-height: 200px;
            overflow-y: auto;
            z-index: 100;
            min-width: 180px;
            display: none;
        }
        .sql-completion.visible { display: block; }
        .sql-completion__item {
            padding: 4px 12px;
            cursor: pointer;
            font-size: 13px;
            font-family: var(--font-mono);
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
        .export-bar {
            display: flex;
            gap: var(--space-xs);
            align-items: center;
        }
        .export-bar__btn { font-size: 13px !important; }
    </style>
    <script>
        // ─── SQL Completion Engine ───────────────────────────────
        const SQL_KEYWORDS = [
            'SELECT', 'FROM', 'WHERE', 'AND', 'OR', 'NOT', 'IN', 'IS', 'NULL',
            'LIKE', 'BETWEEN', 'EXISTS', 'CASE', 'WHEN', 'THEN', 'ELSE', 'END',
            'AS', 'ON', 'JOIN', 'LEFT', 'RIGHT', 'INNER', 'OUTER', 'CROSS',
            'FULL', 'GROUP', 'BY', 'ORDER', 'ASC', 'DESC', 'HAVING', 'LIMIT',
            'OFFSET', 'UNION', 'ALL', 'DISTINCT', 'COUNT', 'SUM', 'AVG', 'MIN',
            'MAX', 'WITH', 'RECURSIVE', 'EXPLAIN', 'TRUE', 'FALSE', 'COALESCE',
            'NULLIF', 'CAST', 'EXTRACT', 'INSERT', 'INTO', 'VALUES', 'UPDATE',
            'SET', 'DELETE', 'CREATE', 'TABLE', 'DROP', 'ALTER', 'SHOW', 'TABLES',
            'COLUMNS'
        ];

        window.STUDIO_TABLES = [];

        window._studioComplete = function(textarea) {
            const pos = textarea.selectionStart;
            const text = textarea.value.substring(0, pos);
            const match = text.match(/(\w+)$/);
            if (!match) return;

            const prefix = match[1].toUpperCase();
            const candidates = [
                ...window.STUDIO_TABLES.map(t => ({ name: t, type: 'table' })),
                ...SQL_KEYWORDS.map(k => ({ name: k, type: 'keyword' })),
            ].filter(c => c.name.toUpperCase().startsWith(prefix) && c.name.toUpperCase() !== prefix);

            if (candidates.length === 0) return;

            if (candidates.length === 1) {
                const completion = candidates[0].name;
                const before = textarea.value.substring(0, pos - match[1].length);
                const after = textarea.value.substring(pos);
                const insert = match[1][0] === match[1][0].toLowerCase()
                    ? completion.toLowerCase() : completion;
                textarea.value = before + insert + ' ' + after;
                textarea.selectionStart = textarea.selectionEnd = before.length + insert.length + 1;
                textarea.dispatchEvent(new Event('input', { bubbles: true }));
                return;
            }

            const dropdown = document.getElementById('studio-completion');
            dropdown.innerHTML = '';
            candidates.slice(0, 10).forEach((c, i) => {
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
        };

        // ─── Schema Filter ──────────────────────────────────────
        window._filterSchema = function(filter) {
            const tree = document.getElementById('schema-tree');
            if (!tree) return;
            const lowerFilter = (filter || '').toLowerCase();
            tree.querySelectorAll('.schema-tree__table').forEach(el => {
                const name = (el.querySelector('.schema-tree__label')?.textContent || '').toLowerCase();
                const match = !lowerFilter || name.includes(lowerFilter);
                el.style.display = match ? '' : 'none';
                // Toggle child columns
                const tableName = el.querySelector('.schema-tree__label')?.textContent || '';
                tree.querySelectorAll(`.schema-tree__column[data-parent="${tableName}"]`).forEach(col => {
                    col.style.display = match ? '' : 'none';
                });
            });
        };

        // ─── Query History ──────────────────────────────────────
        const HISTORY_KEY = 'kimberlite-studio-history';
        const MAX_HISTORY = 50;

        function _loadHistory() {
            try {
                return JSON.parse(localStorage.getItem(HISTORY_KEY) || '[]');
            } catch { return []; }
        }

        function _renderHistory() {
            const container = document.getElementById('query-history');
            if (!container) return;
            const history = _loadHistory();
            if (history.length === 0) {
                container.innerHTML = '<div class="schema-tree"><div class="schema-tree__item" data-level="0" data-type="info" style="color: var(--text-tertiary); font-style: italic;">No queries yet</div></div>';
                return;
            }
            let html = '<div style="margin-bottom: var(--space-2xs);"><span class="query-history__clear" onclick="window._clearHistory()">Clear all</span></div>';
            history.forEach((q, i) => {
                const escaped = q.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;').replace(/"/g,'&quot;');
                const truncated = q.length > 60 ? q.substring(0, 60) + '...' : q;
                const truncEscaped = truncated.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
                html += '<div class="query-history__item" title="' + escaped + '" onclick="window._loadFromHistory(' + i + ')">' + truncEscaped + '</div>';
            });
            container.innerHTML = html;
        }

        window._saveQueryHistory = function(query) {
            if (!query || !query.trim()) return;
            const history = _loadHistory();
            // Remove duplicates
            const idx = history.indexOf(query.trim());
            if (idx !== -1) history.splice(idx, 1);
            history.unshift(query.trim());
            if (history.length > MAX_HISTORY) history.length = MAX_HISTORY;
            localStorage.setItem(HISTORY_KEY, JSON.stringify(history));
            _renderHistory();
        };

        window._loadFromHistory = function(index) {
            const history = _loadHistory();
            if (history[index]) {
                const textarea = document.getElementById('studio-textarea');
                if (textarea) {
                    textarea.value = history[index];
                    textarea.dispatchEvent(new Event('input', { bubbles: true }));
                    textarea.focus();
                }
            }
        };

        window._clearHistory = function() {
            localStorage.removeItem(HISTORY_KEY);
            _renderHistory();
        };

        // Render history on load
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
            if ((e.metaKey || e.ctrlKey) && e.key === 'k') {
                e.preventDefault();
                document.getElementById('studio-textarea')?.focus();
            }
            if (e.key === 'Escape') {
                document.getElementById('studio-completion')?.classList.remove('visible');
                const signals = window.ds?.store;
                if (signals) signals.error = null;
            }
        });

        console.log('Kimberlite Studio initialized');
        console.log('Keyboard shortcuts:');
        console.log('  Ctrl+Enter: Execute query');
        console.log('  Tab: SQL completion');
        console.log('  Cmd/Ctrl+K: Focus query editor');
        console.log('  Escape: Clear errors / close dropdown');
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
                    <div class="error-banner" style="display: none;" data-show="$error !== null">
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

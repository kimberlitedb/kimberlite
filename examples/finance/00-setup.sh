#!/usr/bin/env bash
#
# One-shot setup for the finance ledger example.
#
# Creates a fresh project in /tmp/kimberlite-finance, starts a dev server in
# the background on 127.0.0.1:5432, and applies the schema (which has seed
# data inlined). Prints the PID so you can kill it when you're done.

set -euo pipefail

PROJECT_DIR="${PROJECT_DIR:-/tmp/kimberlite-finance}"
DATA_SUBDIR="data"                              # relative to $PROJECT_DIR
ADDR="${ADDR:-127.0.0.1:5432}"
TENANT="${TENANT:-1}"
EXAMPLE_DIR="$(cd "$(dirname "$0")" && pwd)"

have() { command -v "$1" >/dev/null 2>&1; }

if ! have kimberlite; then
    echo "❌ kimberlite CLI not found on PATH." >&2
    echo "   Install: curl -fsSL https://kimberlite.dev/install.sh | sh" >&2
    exit 1
fi

echo "▶ Fresh project at $PROJECT_DIR"
rm -rf "$PROJECT_DIR"
mkdir -p "$PROJECT_DIR"
kimberlite init "$PROJECT_DIR/$DATA_SUBDIR" --template default --yes >/dev/null

echo "▶ Starting dev server on $ADDR"
(
    cd "$PROJECT_DIR"
    kimberlite start "$DATA_SUBDIR/.kimberlite/data" \
        --address "$ADDR" --development \
        >"$PROJECT_DIR/server.log" 2>&1 &
    echo $! > "$PROJECT_DIR/server.pid"
)

# Wait for the server to open the port.
for _ in {1..30}; do
    if kimberlite info --server "$ADDR" >/dev/null 2>&1; then break; fi
    sleep 0.2
done

SERVER_PID="$(cat "$PROJECT_DIR/server.pid")"

# `kimberlite query` is single-statement; this helper chunks a .sql file on
# semicolons (ignoring comment-only lines) and executes each statement.
apply_sql() {
    local file="$1"
    awk '
        BEGIN { RS = ";" }
        {
            gsub(/--[^\n]*/, "")
            gsub(/[[:space:]]+/, " ")
            gsub(/^ | $/, "")
            if (length($0) > 0) {
                printf "%s;%c", $0, 0
            }
        }
    ' "$file" | while IFS= read -r -d '' stmt; do
        if ! kimberlite query --server "$ADDR" --tenant "$TENANT" "$stmt" >/dev/null; then
            echo "❌ Failed: $stmt" >&2
            return 1
        fi
    done
}

echo "▶ Applying schema + seed"
apply_sql "$EXAMPLE_DIR/schema.sql"

cat <<EOF

✅ Finance ledger example is up and running.

   Server:     $ADDR
   Project:    $PROJECT_DIR
   Server log: $PROJECT_DIR/server.log
   PID:        $SERVER_PID

Next steps:

   # Poke around in the REPL:
   kimberlite repl --tenant $TENANT --address $ADDR

   # Run the SEC-style audit queries:
   kimberlite query --server $ADDR -f $EXAMPLE_DIR/audit_queries.sql

   # Run the time-travel queries (point-in-time portfolio reconstruction):
   kimberlite query --server $ADDR -f $EXAMPLE_DIR/03-time-travel.sql

   # Run the end-to-end SDK walkthrough:
   #   Python:      python $EXAMPLE_DIR/ledger.py

When you're done:

   kill $SERVER_PID
EOF

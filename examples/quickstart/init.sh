#!/bin/bash
# Kimberlite Quickstart Init Script
#
# This script initializes and starts a Kimberlite server for development.

set -e

# Configuration
DATA_DIR="${DATA_DIR:-./data}"
ADDRESS="${ADDRESS:-127.0.0.1:3000}"
KIMBERLITE="${KIMBERLITE:-kimberlite}"

echo "Kimberlite Quickstart"
echo "====================="
echo ""

# Check if kimberlite binary exists
if ! command -v "$KIMBERLITE" &> /dev/null; then
    echo "Error: kimberlite binary not found."
    echo ""
    echo "Either:"
    echo "  1. Add the kimberlite binary to your PATH"
    echo "  2. Set KIMBERLITE=/path/to/kimberlite"
    echo ""
    exit 1
fi

# Clean up existing data directory
if [ -d "$DATA_DIR" ]; then
    echo "Removing existing data directory..."
    rm -rf "$DATA_DIR"
fi

# Initialize data directory
echo "Initializing data directory at $DATA_DIR..."
$KIMBERLITE init "$DATA_DIR" --development

echo ""
echo "Starting server on $ADDRESS..."
echo "Press Ctrl+C to stop."
echo ""

# Start server (foreground)
exec $KIMBERLITE start --address "$ADDRESS" "$DATA_DIR"

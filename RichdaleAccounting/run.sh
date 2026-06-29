#!/bin/bash

# NexusLedger Run Script
# Author: Mounir Siraji <mounir@richdaleai.com>
# License: Apache-2.0

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "🚀 Starting NexusLedger..."

# Check if binary exists
if [ ! -f target/release/nexus-core ]; then
    echo "❌ Binary not found. Building first..."
    ./build.sh
fi

# Create data directories if they don't exist
mkdir -p data/{invoices,receipts,documents,statements,logs,exports,certs}
mkdir -p config

# Set environment variables
export RUST_LOG=${RUST_LOG:-info}
export SURREALDB_URL=${SURREALDB_URL:-ws://localhost:8000}
export SURREALDB_NS=${SURREALDB_NS:-nexus}
export SURREALDB_DB=${SURREALDB_DB:-accounting}
export OLLAMA_URL=${OLLAMA_URL:-http://localhost:11434}

# Run the application
echo "📊 NexusLedger is starting..."
echo "🌐 API will be available at http://localhost:8080"
echo "📝 Press Ctrl+C to stop"

target/release/nexus-core
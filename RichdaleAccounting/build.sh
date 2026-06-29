#!/bin/bash

# NexusLedger Build Script
# Author: Mounir Siraji <mounir@richdaleai.com>
# License: Apache-2.0

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "🚀 Building NexusLedger..."

# Check if Rust is installed
if ! command -v cargo &> /dev/null; then
    echo "❌ Rust/Cargo not found. Please install Rust from https://rustup.rs"
    exit 1
fi

# Build in release mode
echo "📦 Building release binary..."
cargo build --release --workspace

echo "✅ Build completed successfully!"
echo "📁 Binary location: ./target/release/nexus-core"

# Create data directories if they don't exist
echo "📁 Creating data directories..."
mkdir -p data/{invoices,receipts,documents,statements,logs,exports,certs}
mkdir -p config

# Copy default config if it doesn't exist
if [ ! -f config/server.toml ]; then
    echo "📝 Creating default configuration..."
    cp config/server.toml.example config/server.toml 2>/dev/null || \
    cat > config/server.toml << 'EOF'
[server]
port = 8080
host = "0.0.0.0"

[database]
url = "ws://localhost:8000"
ns = "nexus"
db = "accounting"

[ai]
ollama_url = "http://localhost:11434"
model = "llama3.2"

[logging]
level = "info"
file = "data/logs/nexusledger.log"
EOF
fi

echo "✅ NexusLedger is ready to run!"
echo "🚀 To start: ./run.sh"
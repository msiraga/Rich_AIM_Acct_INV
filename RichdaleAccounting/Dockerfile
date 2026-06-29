# NexusLedger Dockerfile
FROM rust:1.70-slim as builder

WORKDIR /usr/src/nexusledger

# Install dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace files
COPY Cargo.toml ./
COPY nexus-core/Cargo.toml ./nexus-core/
COPY nexus-core/src ./nexus-core/src

# Build the application
RUN cargo build --release

FROM debian:bullseye-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    libssl1.1 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /usr/src/nexusledger

# Copy built binary from builder
COPY --from=builder /usr/src/nexusledger/target/release/nexus-core .

# Copy configuration and data directories
COPY config/ ./config/
VOLUME ["./data"]

EXPOSE 8080
EXPOSE 8081

CMD ["./nexus-core"]
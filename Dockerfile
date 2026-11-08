# ============================================================================
# Stage 1: Rust Builder
# ============================================================================
FROM rust:1.83-bookworm AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libasound2-dev \
    libudev-dev \
    build-essential \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY rust/ ./rust/
COPY static/ ./static/
COPY config.json* ./

WORKDIR /src/rust
RUN cargo build --release --bin scanner-controller

# ============================================================================
# Stage 2: Minimal Runtime Container
# ============================================================================
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    libasound2 \
    libudev1 \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /src/rust/target/release/scanner-controller /usr/local/bin/scanner-controller
COPY static/ /app/static/

RUN mkdir -p /var/scanscribe/inbox /app/recordings

ENV SCANSCRIBE_INBOX_DIR=/var/scanscribe/inbox
ENV SCANNER_A_PORT=/dev/ttyACM0
ENV SCANNER_B_PORT=/dev/ttyACM1
ENV RUST_LOG=info

EXPOSE 8000

CMD ["/usr/local/bin/scanner-controller", "serve", "--host", "0.0.0.0", "--port", "8000", "--no-browser"]

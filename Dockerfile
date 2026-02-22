# ── Stage 1: Build ─────────────────────────────────────────────────────────
FROM rust:1.85-bookworm AS builder

WORKDIR /src
COPY . .

RUN cargo build --release --locked 2>/dev/null || cargo build --release \
    && strip target/release/rustedclaw

# ── Stage 2: Runtime ──────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Non-root user
RUN useradd -m -s /bin/bash rustedclaw
USER rustedclaw
WORKDIR /home/rustedclaw

# Copy binary
COPY --from=builder /src/target/release/rustedclaw /usr/local/bin/rustedclaw

# Config directory (mountable)
RUN mkdir -p /home/rustedclaw/.rustedclaw

EXPOSE 42617

# Health check
HEALTHCHECK --interval=15s --timeout=3s --start-period=5s --retries=3 \
    CMD curl -fs http://127.0.0.1:42617/health || exit 1

ENTRYPOINT ["rustedclaw"]
CMD ["gateway", "--host", "0.0.0.0", "--port", "42617"]

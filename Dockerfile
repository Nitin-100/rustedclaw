# ── Stage 1: Build ─────────────────────────────────────────────────────────
FROM rust:1.85-bookworm AS builder

WORKDIR /src
COPY . .

RUN cargo build --release --locked 2>/dev/null || cargo build --release \
    && strip target/release/rustedclaw

# ── Stage 2: Runtime (distroless — no shell, no package manager, minimal attack surface)
FROM gcr.io/distroless/cc-debian12:nonroot

# Copy binary
COPY --from=builder /src/target/release/rustedclaw /usr/local/bin/rustedclaw

EXPOSE 42617

ENTRYPOINT ["rustedclaw"]
CMD ["gateway", "--host", "0.0.0.0", "--port", "42617"]

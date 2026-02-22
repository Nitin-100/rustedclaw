## What this PR adds

### 🔄 CI Workflow (`ci.yml`)
- **Tests** on Ubuntu, macOS, and Windows (`cargo test --release`)
- **Lint** — `cargo fmt --check` + `cargo clippy -D warnings`
- **Docker** — builds the image, checks size, runs a smoke test (`/health`)
- Uploads per-platform release binaries as artifacts

### 🧪 Benchmark Workflow (`bench.yml`)
Automated benchmark suite that measures real numbers on CI hardware:

| Metric | How |
|---|---|
| **Binary size** | `stat` the release binary |
| **Cold start** | 20-run avg of `rustedclaw version` startup time |
| **Idle RAM** | RSS after gateway boot (Linux `/proc`, macOS `ps`, Windows `WorkingSet64`) |
| **Load RAM** | RSS after 1,000 sequential `/health` requests |
| **Burst RAM** | RSS after 1,000 concurrent requests (10×100) |
| **Chat RAM** | RSS after 20 `/v1/chat/completions` POSTs |
| **Throughput** | Sequential req/s to `/health` |

Runs on **Linux x86_64**, **macOS ARM64**, and **Windows x86_64**. Publishes a combined 3-platform comparison table to the GitHub Actions Summary and saves JSON artifacts for each platform.

### 📝 README Updates
- Added **CI** and **Benchmarks** GitHub Actions status badges to the header

### Triggers
- CI: push to `master`/`main`/`ci/benchmarks`, PRs to `master`/`main`
- Bench: push to `master`/`main`/`ci/benchmarks` (when crates/Cargo/workflows/README change), or manual `workflow_dispatch`

---

> **Merge criteria:** All CI jobs pass ✅ and benchmark numbers look reasonable on all 3 platforms.

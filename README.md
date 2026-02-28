<p align="center">
  <img src="assets/logo.png" width="180" alt="RustedClaw">
</p>

<h1 align="center">RustedClaw</h1>

<p align="center">
  <strong>The only AI agent runtime with built-in local inference.<br>Run models on your hardware — zero API keys, zero internet, zero cost per token.<br>~6.7 MB RAM. 4.21 MB binary. One binary. No dependencies. No sign-ups.</strong>
</p>

<p align="center">
  <a href="https://github.com/Nitin-100/rustedclaw/actions/workflows/ci.yml"><img src="https://github.com/Nitin-100/rustedclaw/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/Nitin-100/rustedclaw/actions/workflows/bench.yml"><img src="https://github.com/Nitin-100/rustedclaw/actions/workflows/bench.yml/badge.svg" alt="Benchmarks"></a>
  <a href="https://github.com/Nitin-100/rustedclaw/actions/workflows/local.yml"><img src="https://github.com/Nitin-100/rustedclaw/actions/workflows/local.yml/badge.svg" alt="Local Inference CI"></a>
  <a href="https://github.com/Nitin-100/rustedclaw/actions/workflows/release.yml"><img src="https://github.com/Nitin-100/rustedclaw/actions/workflows/release.yml/badge.svg" alt="Release Builds"></a>
  <a href="#-quick-start"><img src="https://img.shields.io/badge/get_started-2_min-brightgreen?style=for-the-badge" alt="Get Started"></a>
  <a href="#-benchmarks"><img src="https://img.shields.io/badge/RAM-6.68_MB-critical?style=for-the-badge" alt="RAM"></a>
  <a href="#-benchmarks"><img src="https://img.shields.io/badge/binary-4.21_MB-blueviolet?style=for-the-badge" alt="Binary Size"></a>
  <a href="#-local-inference-zero-api-keys-zero-internet"><img src="https://img.shields.io/badge/local_AI-air--gapped-orange?style=for-the-badge" alt="Local Inference"></a>
  <a href="LICENSE-MIT"><img src="https://img.shields.io/badge/license-MIT-blue?style=for-the-badge" alt="MIT License"></a>
</p>

---

## 🤯 Why RustedClaw?

Every other AI agent runtime **forces you online**. They need API keys. They need cloud accounts. They need the internet. Your data leaves your machine, your costs scale with usage, and you're locked in.

**RustedClaw is the first agent runtime that ships a built-in ML engine.** Run TinyLlama, SmolLM, Phi-2, or Qwen directly on your CPU — air-gapped, offline, with zero cost per token. No GPU required. No Python. No Ollama. Just one Rust binary.

And when you *do* want cloud models? Connect to 11 providers with a single API key swap. Best of both worlds.

```
┌─────────────────────────────────────────────────────────────────┐
│  # Cloud mode — any of 11 providers                             │
│  ./rustedclaw gateway                                           │
│                                                                 │
│  # Local mode — no API key, no internet, no cost                │
│  ./rustedclaw gateway --local --model tinyllama                 │
│                                                                 │
│  Web UI at localhost:42617. Chat, tools, memory, agent loops.   │
│  One binary. No Docker. No npm. No account. Just run it.        │
└─────────────────────────────────────────────────────────────────┘
```

<table>
<tr>
<td width="25%" align="center">

**🧠 Local AI Built In**<br>
Run models on your hardware.<br>
8 model presets. GGUF quantized.<br>
CPU-only. Air-gapped. Zero cost.

</td>
<td width="25%" align="center">

**🪶 Absurdly Light**<br>
6.68 MB idle. 6.9 MB peak under<br>
6,000+ requests. 18 ms cold start.<br>
Your <em>terminal emulator</em> uses more.

</td>
<td width="25%" align="center">

**🔓 Truly Yours**<br>
No account. No telemetry. No vendor.<br>
11 cloud providers + local inference.<br>
MIT licensed — fork it, sell it, ship it.

</td>
<td width="25%" align="center">

**⚡ Production-Ready**<br>
4 agent patterns. 9 tools. Memory.<br>
Web UI. Cron. Contracts. Cost tracking.<br>
475 tests. Not a toy — a runtime.

</td>
</tr>
</table>

---

## 🧪 Benchmarks

All numbers measured locally on i7-12700F, 32 GB RAM, Windows 11, NVMe. Reproduce: `scripts\benchmark_lowend.ps1`.

### Standard Build

| Metric | Native (i7-12700F) | Docker "Raspberry Pi" (1 CPU, 256 MB) | Docker "$5 VPS" (1 CPU, 512 MB) | Docker "$10 VPS" (2 CPU, 1 GB) |
|---|:---:|:---:|:---:|:---:|
| **Idle RAM** | 6.68 MB | 1.11 MiB | 1.10 MiB | 1.11 MiB |
| **Post-load RAM** | 6.90 MB | — | — | — |
| **Throughput (seq)** | 186 req/s | 1,786 req/s | 1,832 req/s | 1,792 req/s |
| **Throughput (bulk)** | 3,774 req/s | — | — | — |
| **Endpoints** | 11/11 | 11/11 | 11/11 | 11/11 |

### Local Inference Build (`--features local`)

| Metric | Native (i7-12700F) | Docker "Raspberry Pi" (1 CPU, 256 MB) | Docker "$5 VPS" (1 CPU, 512 MB) | Docker "$10 VPS" (2 CPU, 1 GB) |
|---|:---:|:---:|:---:|:---:|
| **Idle RAM** | 9.34 MB | 1.36 MiB | 1.33 MiB | 1.36 MiB |
| **Throughput (seq)** | 200 req/s | 1,838 req/s | 1,754 req/s | 1,340 req/s |
| **Throughput (bulk)** | 3,906 req/s | — | — | — |
| **Endpoints** | 11/11 | 11/11 | 11/11 | 11/11 |

<sub> Docker cgroup-constrained RSS — the kernel reclaims pages under memory pressure, so reported RSS is lower than on bare metal. Unconstrained native RSS is ~6.7 MB (standard) / ~9.3 MB (local).</sub>

**Machine-independent metrics:**
- Binary size: **4.21 MB** standard  **7.79 MB** with local inference (release, stripped, `opt-level="z"`, LTO)
- Threads: **6** (Tokio `worker_threads=2` + runtime)
- Cold start: **18 ms** standard  **30 ms** local (i7-12700F + NVMe — expect 30–60 ms on a VPS)
- Model presets: **8/8** tested — tinyllama, smollm, smollm:135m, smollm:360m, smollm:1.7b, phi2, qwen:0.5b, qwen:1.5b

> **475 tests**, 0 failures. 0 clippy warnings. 0 fmt diffs.

---

## � What Sets RustedClaw Apart

Other runtimes make you choose: lightweight *or* featureful, cloud *or* local, simple *or* production-ready. RustedClaw is the only one that delivers **all of it** in a single binary.

| Capability | RustedClaw | Others |
|---|:---:|:---:|
| **Local Inference (no API key)** | ✅ Built-in Candle ML engine | ❌ Need Ollama / external setup |
| **Air-Gapped / Offline** | ✅ Fully offline after first download | ❌ Always need internet |
| **Cloud Providers** | 11 (OpenAI, Anthropic, Groq, etc.) | 1–28 (varies) |
| **Binary Size** | **4.21 MB** (7.79 MB with local) | 8 MB – 300 MB |
| **Idle RAM** | **6.68 MB** | 8 MB – 1.2 GB |
| **Cold Start** | **18 ms** | 20 ms – 4 s |
| **Web UI** | ✅ 11-page embedded SPA | Some have it, some don't |
| **Agent Patterns** | 4 (ReAct, RAG, Multi-agent, Chat) | 0–1 |
| **Memory + Search** | SQLite + FTS5 | In-memory or requires Postgres |
| **Agent Contracts** | ✅ Declarative guardrails | ❌ |
| **Cost Tracking & Budgets** | ✅ Built-in | ❌ |
| **WASM Sandbox** | ✅ | Some |
| **External Dependencies** | **None** | npm / PostgreSQL / Docker |
| **Account Required** | **No** | Some require sign-up |
| **Tests** | **475**, 0 failures | Often unpublished |
| **License** | MIT | Varies |

> **The bottom line:** No other runtime gives you local AI inference + 11 cloud providers + Web UI + agent contracts + cost tracking + memory + 4 agent patterns in a 4 MB binary that runs on 6.68 MB RAM.

---

## 🚀 Quick Start

### Option A — Local AI (No API Key Needed)

```bash
git clone https://github.com/Nitin-100/rustedclaw.git && cd rustedclaw
cargo build --release --features local

./target/release/rustedclaw onboard
./target/release/rustedclaw gateway --local --model tinyllama
```

Open **http://localhost:42617** — that's it. No API key. No account. The model downloads once (~670 MB) and is cached forever.

### Option B — Cloud Providers

```bash
git clone https://github.com/Nitin-100/rustedclaw.git && cd rustedclaw
cargo build --release

./target/release/rustedclaw onboard
```

**Set your API key** (pick ONE method):

```bash
# Environment variable (easiest)
export OPENAI_API_KEY="sk-..."              # OpenAI
# or: export OPENROUTER_API_KEY="sk-or-..."  # OpenRouter (100+ models)
# or: export RUSTEDCLAW_API_KEY="sk-..."     # Generic (works with any provider)

# Or edit ~/.rustedclaw/config.toml directly:
#   api_key = "sk-your-key-here"
```

```bash
./target/release/rustedclaw gateway
```

Open **http://localhost:42617** — done. Requires Rust 1.88+.

### Option C — Docker

```bash
git clone https://github.com/Nitin-100/rustedclaw.git && cd rustedclaw

# Cloud mode — set your API key
echo "OPENAI_API_KEY=sk-..." > .env
docker compose up -d

# Or local mode — no API key needed
docker compose -f docker-compose.yml up -d --build  # uses --features local
```

Open **http://localhost:42617** — done.

---

## 🧠 Local Inference (Zero API Keys, Zero Internet)

> **This is our headline feature.** No other self-hosted agent runtime ships a built-in ML engine.

RustedClaw embeds [Candle](https://github.com/huggingface/candle) — a **Rust-native ML framework from HuggingFace** — directly into the binary. No Python. No Ollama. No GPU required. Run quantized language models on your CPU with:

- **Zero API keys** — nothing to sign up for
- **Zero internet** — fully air-gapped after initial model download
- **Zero cost per token** — your hardware, your electricity, that's it
- **Zero external processes** — no sidecar, no daemon, everything in one binary

```bash
# Build with local inference support (adds ~3.6 MB to binary)
cargo build --release --features local

# Run with a local model (auto-downloads on first use, then cached forever)
./target/release/rustedclaw agent --local --model tinyllama

# Or start the full gateway with local AI — Web UI + REST API + tools
./target/release/rustedclaw gateway --local --model tinyllama
```

**Use cases where local inference shines:**
- 🏢 **Enterprise / regulated environments** — data never leaves your network
- 🛩️ **Air-gapped deployments** — military, submarines, field ops, factory floors
- 💰 **Cost-sensitive workloads** — run millions of inferences at zero marginal cost
- 🌐 **Edge / IoT** — SmolLM 135M runs on a Raspberry Pi
- 🧪 **Development & testing** — iterate on agent logic without burning API credits
- 🔒 **Privacy-first applications** — healthcare, legal, finance — no cloud dependency

### Available Models

| Model | Size | RAM | Best For |
|---|---|---|---|
| `smollm:135m` | ~80 MB | ~200 MB | Fastest, IoT/edge devices |
| `smollm:360m` | ~200 MB | ~400 MB | Fast, basic tasks |
| `qwen:0.5b` | ~350 MB | ~600 MB | Small but capable |
| `tinyllama` | ~670 MB | ~1 GB | Best quality/size ratio |
| `qwen:1.5b` | ~900 MB | ~1.5 GB | Good quality |
| `smollm:1.7b` | ~950 MB | ~1.5 GB | Good quality |
| `phi2` | ~1.6 GB | ~2.5 GB | Strong quality |

You can also point to any local GGUF file:
```bash
./target/release/rustedclaw agent --local --model /path/to/model.gguf
```

### Building with Local Inference

Local inference is behind a Cargo feature flag — the standard build stays lean at **4.21 MB**. Enable it when you need it:

```bash
# Standard build (no local models, 4.21 MB)
cargo build --release

# Local inference build (adds Candle ML engine, 7.79 MB)
cargo build --release --features local

# Run tests (including local provider tests)
cargo test --release --features local

# Verify everything
cargo clippy --all-targets --features local -- -D warnings
cargo fmt --all -- --check
```

The `local` feature adds [Candle](https://github.com/huggingface/candle) (Rust-native ML), [tokenizers](https://github.com/huggingface/tokenizers), and [hf-hub](https://github.com/huggingface/hf-hub) as optional dependencies. Binary grows by ~3.6 MB.

### Configuring Local Models

**CLI flags:**
```bash
# Agent mode with a specific model
./target/release/rustedclaw agent --local --model smollm:135m

# Gateway mode — serves local model via REST API + Web UI
./target/release/rustedclaw gateway --local --model qwen:0.5b

# Custom GGUF file
./target/release/rustedclaw agent --local --model /path/to/model.gguf
```

**Config file** (`~/.rustedclaw/config.toml`):
```toml
# Use local inference as default provider
default_provider = "local"
default_model = "tinyllama"    # Any preset name or path to .gguf file
```

**Environment variables:**
```bash
export RUSTEDCLAW_PROVIDER=local
export RUSTEDCLAW_MODEL=tinyllama

# Custom model cache location (default: ~/.cache/huggingface)
export HF_HOME=/path/to/cache
```

### Testing Model Presets

All 8 presets are verified in CI. You can test them locally:

```bash
# Quick health-check — starts gateway with each preset
for model in tinyllama smollm smollm:135m smollm:360m smollm:1.7b phi2 qwen:0.5b qwen:1.5b; do
  echo "Testing $model..."
  ./target/release/rustedclaw gateway --local --model $model --port 42690 &
  PID=$!
  sleep 3
  curl -sf http://127.0.0.1:42690/health && echo " ✓ $model OK" || echo " ✗ $model FAILED"
  kill $PID 2>/dev/null
  sleep 1
done
```

### Air-Gapped / Offline Deployment

Models are downloaded from HuggingFace Hub on first use, then cached locally forever:

1. **On a machine with internet**, run the model once to cache it:
   ```bash
   ./target/release/rustedclaw agent --local --model tinyllama -m "hello"
   ```
2. **Copy the cache** to your air-gapped machine:
   ```bash
   # Default cache locations:
   # Linux/macOS: ~/.cache/huggingface/
   # Windows:     %USERPROFILE%\.cache\huggingface\
   scp -r ~/.cache/huggingface/ airgapped-host:~/.cache/huggingface/
   ```
3. **Run completely offline** — no network calls, zero cost per token:
   ```bash
   ./target/release/rustedclaw gateway --local --model tinyllama
   ```

### Chat Templates

Each model preset maps to its native chat template format:

| Template | Models | Format |
|---|---|---|
| **TinyLlama** | tinyllama | `<\|user\|>\n{msg}</s>\n<\|assistant\|>` |
| **ChatML** | smollm variants, qwen variants | `<\|im_start\|>user\n{msg}<\|im_end\|>` |
| **Llama2** | phi2 | `[INST] {msg} [/INST]` |
| **Llama3** | (custom GGUF) | `<\|begin_of_text\|><\|start_header_id\|>user<\|end_header_id\|>` |

---

## ✨ What You Get

| Feature | Details |
|---|---|
| **🧠 Local Inference** | **Built-in Candle ML engine** — TinyLlama, SmolLM, Phi-2, Qwen on your CPU. Zero API keys, zero cost, air-gapped capable |
| **☁️ 11 Cloud Providers** | OpenAI, Anthropic, OpenRouter, Ollama, DeepSeek, Groq, Together, Fireworks, Mistral, xAI, Perplexity |
| **🔄 Hybrid Mode** | Switch between local and cloud models with a flag — same agent, same tools, same memory |
| **4 Agent Patterns** | ReAct loop, RAG, Multi-agent Coordinator, Interactive Chat |
| **9 Built-in Tools** | Shell, file read/write, calculator, HTTP, search, knowledge base, JSON transform, code analysis |
| **Memory** | SQLite + FTS5 full-text search with hybrid vector/keyword retrieval |
| **Scheduled Routines** | Cron-based task automation with add/remove/pause/resume |
| **Web UI** | 11-page embedded SPA — Dashboard, Chat, Memory, Tools, Contracts, Usage & Cost, Channels, Routines, Jobs, Logs, Settings |
| **Streaming** | Real SSE for chat, logs, and events |
| **Security** | AES-256-GCM encryption, rate limiting, HMAC-SHA256 webhooks, path sandboxing, command injection prevention, SSRF blocking, auth middleware, CORS, CSP headers, secret redaction |
| **Agent Contracts** | Declarative behavior guardrails — deny, confirm, warn, or allow tool calls via TOML rules |
| **Cost Tracking & Budgets** | Real-time token cost tracking, per-model pricing for 20+ models, daily/monthly/per-request budget limits |
| **Channels** | CLI, HTTP webhook, WebSocket, Telegram, Slack, Discord |
| **Pairing** | Optional device-pairing for secure remote access |
| **Migration** | Import data from OpenClaw with `rustedclaw migrate openclaw` |
| **Shell Completions** | Bash, Zsh, Fish, PowerShell via `rustedclaw completions <shell>` |
| **Emergency Stop** | `rustedclaw estop` — halt all tasks instantly, `--resume` to restart |

---

## 🛠️ CLI Commands

```
rustedclaw onboard              Initialize config & workspace
rustedclaw agent [-m "msg"]     Chat or send a single message
rustedclaw gateway [--port N]   Start HTTP gateway + Web UI
rustedclaw daemon               Full runtime (gateway + channels + cron)
rustedclaw status               Show system status
rustedclaw doctor               Diagnose system health
rustedclaw providers            List all supported LLM providers
rustedclaw config validate      Validate configuration
rustedclaw config show          Show resolved config
rustedclaw config path          Show config file path
rustedclaw routine list         List cron routines
rustedclaw routine add <name> <cron> <prompt>
rustedclaw routine remove <name>
rustedclaw routine pause/resume <name>
rustedclaw memory stats         Show memory statistics
rustedclaw memory search <q>    Search memories
rustedclaw memory export        Export memories to JSON
rustedclaw memory clear         Clear all memories
rustedclaw contract list        List configured contracts
rustedclaw contract validate    Validate contract definitions
rustedclaw contract test <tool> <args>  Test a contract against a tool call
rustedclaw usage show           Show cost/token usage snapshot
rustedclaw usage pricing        List model pricing table (20+ models)
rustedclaw usage budgets        Show configured budgets
rustedclaw usage estimate <model> [-i tokens] [-o tokens]  Estimate cost
rustedclaw migrate openclaw     Import from OpenClaw
rustedclaw estop [--resume]     Emergency stop / resume
rustedclaw completions <shell>  Generate shell completions
rustedclaw version              Detailed version info
```

---

## 🔧 Configuration

`rustedclaw onboard` creates the config file at:

| OS | Path |
|---|---|
| **Linux / macOS** | `~/.rustedclaw/config.toml` |
| **Windows** | `%USERPROFILE%\.rustedclaw\config.toml` |

```toml
# ~/.rustedclaw/config.toml

# ── API Key ──────────────────────────────────────────────
# Put your LLM provider key here. This is the ONLY required field.
api_key = "sk-your-openai-key-here"

# ── Provider & Model ────────────────────────────────────
# Supported: openai | anthropic | openrouter | ollama | deepseek
#            groq | together | fireworks | mistral | xai | perplexity
default_provider = "openai"
default_model = "gpt-4o-mini"
default_max_tokens = 4096

# ── Gateway ─────────────────────────────────────────────
[gateway]
port = 42617                      # Web UI + API port
host = "0.0.0.0"                  # 0.0.0.0 for Docker, 127.0.0.1 for local only
require_pairing = false

# ── Agent Contracts (optional guardrails) ───────────────
[[contracts]]
name = "no-rm-rf"
trigger = "tool:shell"
condition = 'args.command CONTAINS "rm -rf"'
action = "deny"
message = "Blocked: rm -rf is forbidden"

# ── Cost Tracking & Budgets (optional) ──────────────────
[telemetry]
enabled = true

[[telemetry.budgets]]
scope = "daily"            # per_request | per_session | daily | monthly | total
max_usd = 5.00             # max spend in USD
on_exceed = "deny"         # deny | warn

[[telemetry.budgets]]
scope = "per_request"
max_usd = 0.50
on_exceed = "deny"

# Custom pricing overrides (built-in pricing for 20+ models)
# [telemetry.custom_pricing."my-provider/my-model"]
# input_per_m = 1.0
# output_per_m = 3.0
```

**Environment variables** override the config file (no file editing needed):

| Variable | What it does |
|---|---|
| `OPENAI_API_KEY` | Sets API key for OpenAI |
| `OPENROUTER_API_KEY` | Sets API key for OpenRouter (100+ models) |
| `RUSTEDCLAW_API_KEY` | Generic API key (works with any provider) |
| `RUSTEDCLAW_PROVIDER` | Override `default_provider` |
| `RUSTEDCLAW_MODEL` | Override `default_model` |

Priority: `RUSTEDCLAW_API_KEY` > `OPENROUTER_API_KEY` > `OPENAI_API_KEY` > `config.toml`.

---

## 📡 API

Full REST API on `http://localhost:42617`:

```
GET  /health                    Health check
POST /v1/chat                   Send message → JSON response
POST /v1/chat/stream            Send message → SSE stream
GET  /v1/ws                     WebSocket chat
GET  /v1/tools                  List tools + schemas
GET  /v1/conversations          List conversations
POST /v1/routines               Create scheduled routine
GET  /v1/memory?q=search+term   Search memories
POST /v1/memory                 Save memory
GET  /v1/status                 System status
GET  /v1/config                 Runtime config
GET  /v1/contracts              List agent contracts
POST /v1/contracts              Add a contract at runtime
DELETE /v1/contracts/:name      Remove a contract
GET  /v1/usage                  Real-time cost & token snapshot
GET  /v1/traces                 List recent execution traces
GET  /v1/traces/:id             Get detailed trace with spans
GET  /v1/budgets                List configured budgets
POST /v1/budgets                Add a spending budget
DELETE /v1/budgets/:scope       Remove a budget
GET  /v1/jobs                   List background jobs
GET  /v1/logs                   SSE log stream
```

---

## 🛡️ Agent Contracts

Declarative behavior guardrails for your AI agent. Define rules in `config.toml` that intercept tool calls _before_ they execute.

**Condition DSL** supports: `CONTAINS`, `MATCHES` (regex), `STARTS_WITH`, `ENDS_WITH`, `==`, `!=`, `>`, `<`, `>=`, `<=`, `AND`, `OR`, `NOT`, parentheses, and dotted field paths (`args.nested.key`).

```toml
# Block dangerous commands
[[contracts]]
name = "no-rm-rf"
trigger = "tool:shell"
condition = 'args.command CONTAINS "rm -rf"'
action = "deny"
message = "Blocked: rm -rf is forbidden"

# Warn on any file write
[[contracts]]
name = "warn-writes"
trigger = "tool:file_write"
action = "warn"
message = "Agent is writing to a file"

# Block internal network access
[[contracts]]
name = "no-internal-ips"
trigger = "tool:http"
condition = 'args.url MATCHES "https?://(10\\.|192\\.168\\.|172\\.(1[6-9]|2[0-9]|3[01]))"'
action = "deny"
message = "Internal network access is forbidden"
priority = 10

# Require confirmation for expensive operations
[[contracts]]
name = "confirm-purchases"
trigger = "tool:purchase"
condition = "args.amount > 100"
action = "confirm"
message = "Purchase over $100 requires confirmation"
```

Actions: `deny` (block), `confirm` (ask user), `warn` (log + allow), `allow` (explicit pass).

Manage at runtime via CLI or REST API:


```bash
rustedclaw contract list                          # Show all contracts
rustedclaw contract validate                      # Check for errors
rustedclaw contract test shell '{"command":"rm -rf /"}'  # Simulate
```

---

## � Security

RustedClaw is hardened with defense-in-depth across every layer. Security is not an afterthought — it's built into the architecture.

### Encryption & Authentication

| Layer | Implementation |
|---|---|
| **Secrets at rest** | AES-256-GCM authenticated encryption with 100K-round SHA-256 key derivation |
| **API authentication** | Bearer token middleware on all `/v1` routes |
| **Device pairing** | Cryptographic 8-digit codes (CSPRNG) for secure remote access |
| **Webhook validation** | HMAC-SHA256 signature verification (constant-time comparison) |

### Network Security

| Layer | Implementation |
|---|---|
| **CORS** | Restrictive same-origin policy with explicit method/header allowlists |
| **Rate limiting** | 60 req/min per client, sliding window, auto-cleanup at 10K clients |
| **Body size limits** | 1 MB max request body on all endpoints |
| **SSRF prevention** | Blocks `10.x`, `172.16-31.x`, `192.168.x`, `127.x`, `169.254.x`, `[::1]`, IPv6 link-local/ULA, `.local`/`.internal` DNS |
| **CSP headers** | `Content-Security-Policy`, `X-Frame-Options: DENY`, `X-Content-Type-Options: nosniff`, `Referrer-Policy` |

### Tool & Filesystem Sandboxing

| Layer | Implementation |
|---|---|
| **Shell commands** | Injection character rejection (`\| ; & $ \` ( )`), explicit allowlist (20 safe commands), 30s timeout, deny-by-default |
| **File access** | Path canonicalization with symlink resolution, forbidden path defaults (`~/.ssh`, `~/.aws`, `/etc/shadow`, `C:\Windows\System32`, etc.) |
| **Calculator** | Expression length limit (1000 chars) to prevent ReDoS |
| **WASM isolation** | Optional sandboxed tool execution via Wasmtime |

### Data Protection

| Layer | Implementation |
|---|---|
| **Secret redaction** | Custom `Debug` impls on `AppConfig`, `ProviderConfig`, and all channel configs — API keys/bot tokens print as `[REDACTED]` |
| **Log hygiene** | User messages logged by length only (not content), pairing codes omitted from structured fields |
| **SQL injection** | Parameterized queries for tag filtering with wildcard escaping |
| **Contract regex** | Pattern length limit (200 chars) to prevent catastrophic backtracking |

### DoS & Resource Protection

| Layer | Implementation |
|---|---|
| **Conversations** | Capped at 1,000 with LRU eviction |
| **Memories** | Capped at 10,000 with oldest-10% eviction |
| **Documents** | Capped at 5,000 with oldest-10% eviction |
| **Bearer tokens** | Capped at 100 (FIFO eviction) |
| **Audit log** | Bounded at 10,000 entries with auto-drain |
| **Telemetry traces** | Auto-pruned at 5,000 (completed traces first) |
| **Contract engine log** | Bounded at 5,000 entries |

### Configuration

Security defaults are safe out of the box. Key settings in `config.toml`:

```toml
[autonomy]
# Shell command allowlist — only these commands can execute
shell_allowlist = ["ls", "cat", "echo", "git", "cargo"]
# File access controls
file_read_forbidden = ["~/.ssh", "~/.aws", "/etc/shadow"]
file_write_forbidden = ["~/.ssh", "/etc"]
# Autonomy level: "supervised" | "semi" | "autonomous"
level = "supervised"

[gateway]
require_pairing = true    # Require device pairing for API access
```

---

## �💰 Cost Tracking & Budgets

Real-time token cost tracking with built-in pricing for 20+ models and budget enforcement that stops runaway API spend.

**Built-in pricing** for Anthropic (Claude 4 Opus/Sonnet, 3.5 Sonnet/Haiku), OpenAI (GPT-4o, o1, o3-mini), Google (Gemini 2.0/1.5), Meta (Llama 3.1), Mistral, DeepSeek — or add custom pricing in config.

**Budget scopes**: `per_request`, `per_session`, `daily`, `monthly`, `total`

**Budget actions**: `deny` (block the LLM call) or `warn` (log and allow)

```toml
# In ~/.rustedclaw/config.toml
[[telemetry.budgets]]
scope = "daily"
max_usd = 5.00
on_exceed = "deny"     # Block calls when daily spend exceeds $5
```

```bash
rustedclaw usage show                             # Cost snapshot
rustedclaw usage pricing                          # Model pricing table
rustedclaw usage estimate anthropic/claude-sonnet-4 -i 1000 -o 500  # Estimate cost
```

Every LLM call and tool execution is traced as a **span** — grouped into **traces** per conversation turn. Query via REST API:

```
GET /v1/usage    →  { session_cost_usd, daily_cost_usd, budgets: [...] }
GET /v1/traces   →  [{ id, spans, total_cost_usd, total_tokens }]
```

---

## 🔬 Verify It Yourself

```powershell
# Windows — simulates 3 low-end tiers via Docker
.\scripts\benchmark_lowend.ps1
```

```bash
# Linux / macOS
./scripts/benchmark.sh
```

---

## 🏗️ Project Structure

```
rustedclaw/
├── crates/
│   ├── core/        # Types, traits, errors          (29 tests)
│   ├── config/      # TOML config + env overrides     (9 tests)
│   ├── providers/   # LLM providers + local Candle   (42+5 tests)
│   ├── channels/    # Input channels                 (38 tests)
│   ├── memory/      # SQLite + FTS5                  (49 tests)
│   ├── tools/       # 9 built-in tools               (67 tests)
│   ├── agent/       # ReAct, RAG, Coordinator        (62 tests)
│   ├── gateway/     # Axum HTTP + SSE + WS           (32 tests)
│   ├── contracts/   # Agent behavior contracts        (33 tests)
│   ├── telemetry/   # Cost tracking, tracing, budgets (29 tests)
│   ├── workflow/    # Cron engine                    (16 tests)
│   ├── security/    # Encryption, sandboxing, audit    (40 tests)
│   └── cli/         # Binary entry point + commands   (6 + 17 e2e tests)
├── frontend/        # Embedded SPA (HTML/CSS/JS)
├── scripts/         # Benchmark scripts
├── Dockerfile
├── docker-compose.yml
└── 475 tests, 0 failures
```

---

## 📝 License

[MIT](LICENSE-MIT) — do whatever you want.

---

<p align="center">
  <sub>Built with 🦀 Rust — the only AI agent runtime with built-in local inference. No account. No lock-in. No cloud required.</sub>
</p>

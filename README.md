<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://em-content.zobj.net/source/twitter/408/lobster_1f99e.png">
    <img src="https://em-content.zobj.net/source/twitter/408/lobster_1f99e.png" width="100" alt="RustedClaw">
  </picture>
</p>

<h1 align="center">RustedClaw</h1>

<p align="center">
  <strong>A lightweight Rust reimplementation of OpenClaw — self-hosted AI assistant that idles at <7 MB RAM.</strong>
</p>

<p align="center">
  <a href="#-quick-start"><img src="https://img.shields.io/badge/get_started-2_min-brightgreen?style=for-the-badge" alt="Get Started"></a>
  <a href="#-rustedclaw-vs-openclaw-vs-zeroclaw"><img src="https://img.shields.io/badge/RAM-6.5_MB_idle-critical?style=for-the-badge" alt="RAM"></a>
  <a href="LICENSE-MIT"><img src="https://img.shields.io/badge/license-MIT-blue?style=for-the-badge" alt="MIT License"></a>
</p>

---

<!-- Replace with your own GIF/screenshot — record with OBS or LICEcap -->
<p align="center">
  <img src="https://placehold.co/800x450/1a1a2e/e94560?text=🦞+RustedClaw+Web+UI+Demo&font=inter" width="720" alt="RustedClaw Web UI demo">
  <br>
  <sub>Built-in Web UI — chat, memory, tools, routines. No frontend build step.</sub>
</p>

---

## 🦞 RustedClaw vs OpenClaw vs ZeroClaw

The whole point of this project: **same features, 100× less resources.**

| | **RustedClaw** 🦞 | **ZeroClaw** 🦀 | **OpenClaw** 🐙 |
|---|:---:|:---:|:---:|
| **Idle RAM** | **6.5 MB** | ~8–12 MB¹ | ~1.2 GB |
| **Private Memory** | **1.3 MB** | ~4 MB¹ | ~600 MB |
| **After 200-req burst** | **6.9 MB** *(zero growth)* | not published | ~1.8 GB |
| **Cold Start** | **17 ms** | ~20 ms¹ | ~4 s |
| **Binary Size** | **3.8 MB** | 8.8 MB | ~300 MB (node_modules) |
| **Threads (idle)** | **6** | not published | 30+ (Node event loop + workers) |
| **Runtime Deps** | **0** — single static binary | 0 — single binary | Node 18 + Python 3 + npm |
| **TLS** | `rustls` (pure Rust) | `rustls` | OpenSSL via Node.js |
| **Deployment** | Copy 1 file (3.8 MB) | Copy 1 file (8.8 MB) | npm install → pray |

<sub>¹ ZeroClaw self-reported numbers for `--help`/`status` commands (exit immediately). Gateway idle RAM is not published. Binary size from macOS arm64 release build. Source: [zeroclaw-labs/zeroclaw](https://github.com/zeroclaw-labs/zeroclaw) README, Feb 2026.</sub>

### Why RustedClaw is smaller than ZeroClaw

Both projects are Rust. The difference comes from **engineering choices**:

- **`opt-level = "z"`** — we optimize for size, not speed. For an I/O-bound LLM proxy, size wins.
- **2 Tokio worker threads** — not CPU-count default. An AI assistant doesn't need 20 threads idle.
- **`rustls` everywhere** — pure-Rust TLS, no native OpenSSL linkage overhead.
- **`panic = "abort"`** — no unwinding tables in the binary.
- **Feature-gated heavy deps** — `wasmtime` (WASM sandbox) is opt-in, not compiled by default.
- **12 focused crates** — each crate pulls only what it needs. No kitchen-sink binary.

> **Reproduce these numbers yourself:** run `scripts/benchmark.ps1` (Windows) or `scripts/benchmark.sh` (Linux/macOS).

---

## 🚀 Quick Start

### Option A — Docker (recommended, 2 minutes)

```bash
git clone https://github.com/Nitin-100/rustedclaw.git && cd rustedclaw

# Set your API key (pick one)
echo "OPENAI_API_KEY=sk-..." > .env
# or: echo "OPENROUTER_API_KEY=sk-or-v1-..." > .env

docker compose up -d
```

Open **http://localhost:42617** — done. Chat away.

### Option B — Pre-built Binary (no Docker)

```bash
# Download from Releases
curl -LO https://github.com/Nitin-100/rustedclaw/releases/latest/download/rustedclaw-linux-x86_64.tar.gz
tar xzf rustedclaw-linux-x86_64.tar.gz

# First-time setup
./rustedclaw onboard

# Set your key
export OPENAI_API_KEY="sk-..."

# Start the Web UI + API
./rustedclaw gateway
```

### Option C — Build from Source

```bash
git clone https://github.com/Nitin-100/rustedclaw.git && cd rustedclaw
cargo build --release
./target/release/rustedclaw onboard
./target/release/rustedclaw gateway
```

Requires Rust 1.85+. No other dependencies.

---

## ✨ What You Get

| Feature | Details |
|---|---|
| **10+ LLM Providers** | OpenAI, Anthropic, OpenRouter, Ollama, DeepSeek, Groq, Together, Fireworks, vLLM, llama.cpp |
| **4 Agent Patterns** | ReAct loop, RAG, Multi-agent Coordinator, Interactive Chat |
| **9 Built-in Tools** | Shell, file read/write, calculator, HTTP, search, knowledge base, JSON transform, code analysis |
| **Memory** | SQLite + FTS5 full-text search with hybrid vector/keyword retrieval |
| **Scheduled Routines** | Cron-based task automation |
| **Web UI** | 7-page embedded SPA — Chat, Memory, Tools, Routines, Jobs, Logs, Settings |
| **Streaming** | Real SSE for chat, logs, and events |
| **Security** | Path validation, command sandboxing, WASM tool isolation, configurable autonomy levels |
| **Channels** | CLI, HTTP webhook, WebSocket, Telegram, Slack, Discord |
| **Pairing** | Optional device-pairing for secure remote access |

---

## 🔧 Configuration

First run creates `~/.rustedclaw/config.toml`:

```toml
api_key = "sk-..."
default_provider = "openai"       # openai | anthropic | openrouter | ollama | ...
default_model = "gpt-4o-mini"
default_max_tokens = 4096

[gateway]
port = 42617
host = "0.0.0.0"
require_pairing = false
```

Or use environment variables — `OPENAI_API_KEY`, `OPENROUTER_API_KEY`, `RUSTEDCLAW_API_KEY`, `RUSTEDCLAW_PROVIDER`, `RUSTEDCLAW_MODEL`.

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
GET  /v1/jobs                   List background jobs
GET  /v1/logs                   SSE log stream
```

---

## 🧪 Benchmarks

Run the included scripts to verify on your own hardware:

```powershell
# Windows
.\scripts\benchmark.ps1
```

```bash
# Linux / macOS
./scripts/benchmark.sh
```

Measures: binary size, cold start (avg of 10 runs), idle RAM, memory under load (200-request burst), growth, CPU time, throughput (req/sec), endpoint validation.

---

## 🏗️ Project Structure

```
rustedclaw/
├── crates/
│   ├── core/        # Types, traits, errors          (62 tests)
│   ├── config/      # TOML config + env overrides     (9 tests)
│   ├── providers/   # LLM providers                  (29 tests)
│   ├── channels/    # Input channels                 (38 tests)
│   ├── memory/      # SQLite + FTS5                  (49 tests)
│   ├── tools/       # 9 built-in tools               (67 tests)
│   ├── agent/       # ReAct, RAG, Coordinator        (42 tests)
│   ├── gateway/     # Axum HTTP + SSE + WS           (32 tests)
│   ├── workflow/    # Cron engine                    (16 tests)
│   ├── security/    # Sandboxing + WASM              (40 tests)
│   └── cli/         # Binary entry point             (17 tests)
├── frontend/        # Embedded SPA (HTML/CSS/JS)
├── scripts/         # Benchmark scripts
├── Dockerfile
├── docker-compose.yml
└── 401 tests, 0 failures
```

---

## 📝 License

[MIT](LICENSE-MIT) — do whatever you want.

---

<p align="center">
  <sub>Built with 🦀 Rust — because 1.2 GB of RAM for a chat assistant is unacceptable.</sub>
</p>

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://em-content.zobj.net/source/twitter/408/lobster_1f99e.png">
    <img src="https://em-content.zobj.net/source/twitter/408/lobster_1f99e.png" width="100" alt="RustedClaw">
  </picture>
</p>

<h1 align="center">RustedClaw</h1>

<p align="center">
  <strong>A lightweight Rust reimplementation of OpenClaw — self-hosted AI assistant that idles at <10 MB RAM.</strong>
</p>

<p align="center">
  <a href="#-quick-start"><img src="https://img.shields.io/badge/get_started-2_min-brightgreen?style=for-the-badge" alt="Get Started"></a>
  <a href="#-rustedclaw-vs-openclaw"><img src="https://img.shields.io/badge/RAM-12.5_MB_vs_1.2_GB-critical?style=for-the-badge" alt="RAM"></a>
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

## 🦞 RustedClaw vs OpenClaw

The whole point of this project: **same features, 100× less resources.**

| | **RustedClaw** 🦞 | **OpenClaw** 🐙 | **Δ** |
|---|:---:|:---:|:---:|
| **Idle RAM** | **12.5 MB** | ~1.2 GB | **96× less** |
| **Private Memory** | **3.2 MB** | ~600 MB | **187× less** |
| **After 200-req burst** | **12.6 MB** *(no growth)* | ~1.8 GB *(GC pressure)* | **143× less** |
| **Cold Start** | **7 ms** | ~4 s | **571× faster** |
| **Binary / Install Size** | **4.4 MB** | ~300 MB (node_modules) | **68× smaller** |
| **Runtime Dependencies** | **0** — single static binary | Node 18 + Python 3 + npm | **Zero** |
| **CPU at Idle (2 min)** | 0.047 s | ~2 s | **42× less** |
| **Deployment** | `scp` one file | Install runtime → clone → `npm i` → pray | - |

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

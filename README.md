<p align="center">
  <img src="assets/logo.png" width="180" alt="RustedClaw">
</p>

<h1 align="center">RustedClaw</h1>

<p align="center">
  <strong>No account required. No lock-in. Bring your own API key from any provider.<br>Self-hosted AI agent runtime — single binary, ~1 MB container RAM, deploys on a Raspberry Pi.</strong>
</p>

<p align="center">
  <a href="#-quick-start"><img src="https://img.shields.io/badge/get_started-2_min-brightgreen?style=for-the-badge" alt="Get Started"></a>
  <a href="#-the-landscape"><img src="https://img.shields.io/badge/RAM-~1_MB_container-critical?style=for-the-badge" alt="RAM"></a>
  <a href="#-the-landscape"><img src="https://img.shields.io/badge/binary-3.9_MB-blueviolet?style=for-the-badge" alt="Binary Size"></a>
  <a href="LICENSE-MIT"><img src="https://img.shields.io/badge/license-MIT-blue?style=for-the-badge" alt="MIT License"></a>
</p>

---

## 🌍 The Landscape

There are several open-source AI agent runtimes. Here's how they compare:

| | **RustedClaw** <img src="assets/logo.png" width="18"> | **nullclaw** ⚡ | **ZeroClaw** 🦀 | **IronClaw** 🔗 | **OpenClaw** 🐙 |
|---|:---:|:---:|:---:|:---:|:---:|
| **Language** | Rust | Zig | Rust | Rust | Rust + JS |
| **Account Required** | **No** ✅ | **No** ✅ | **No** ✅ | **Yes** ❌ (NEAR AI) | **No** ✅ |
| **External Deps** | **None** | **None** | **None** | PostgreSQL + pgvector | Node 18 + npm |
| **Binary Size** | **3.9 MB** | **678 KB** 👑 | 8.8 MB | ~15 MB + Postgres | ~300 MB (node_modules) |
| **Idle RAM** | **~1 MB** 🤝 | **~1 MB** 🤝 | ~8–12 MB¹ | ~50+ MB² | ~1.2 GB |
| **Peak RAM** | **1.3 MB** (2.5K burst) | — | not published | — | — |
| **Cold Start** | **<10 ms** | **<2 ms** 👑 | ~20 ms¹ | ~2 s² | ~4 s |
| **Tests** | **407** | 2843 | not published | not published | not published |
| **Providers** | 11 | 22+ | 28+ | NEAR AI only | varies |
| **Channels** | 6 | 13 | 17 | HTTP only | HTTP + WS |
| **Web UI** | ✅ Embedded | ❌ | ✅ | ✅ | ✅ |
| **Agent Patterns** | 4 (ReAct, RAG, Multi, Chat) | — | skills | tools | tools |
| **Memory** | SQLite + FTS5 | file-based | SQLite + vector | PostgreSQL + pgvector | in-memory |
| **WASM Sandbox** | ✅ (opt-in) | ✅ | ✅ | ✅ | ❌ |
| **License** | MIT | MIT | MIT | MIT + Apache-2.0 | Apache-2.0 |
| **Deployment** | Copy 1 file | Copy 1 file | Copy 1 file | Docker + PostgreSQL | npm install → pray |

<sub>¹ ZeroClaw self-reported for `--help`/`status` (exit immediately). Gateway idle RAM not published. Binary from macOS arm64 release.<br>
² IronClaw requires PostgreSQL + pgvector running alongside — total system footprint much higher.</sub>

### Why RustedClaw?

- **nullclaw** is smaller (Zig is hard to beat on raw binary size) — but RustedClaw **matches it on RAM (~1 MB)** and has a **built-in Web UI**, **4 agent patterns**, **memory with FTS5 search**, and **more structured architecture** (12 focused crates).
- **ZeroClaw** has more channels and providers — but **2× our binary size** and no published RAM benchmarks under load.
- **IronClaw** requires a **NEAR AI account** and **PostgreSQL + pgvector** — that's vendor lock-in, not self-hosting.
- **OpenClaw** uses 1.2 GB of RAM at idle. That's not a typo.

> **Our niche: lightweight + full-featured + truly independent.** No sign-up. No vendor lock-in. Bring your own API key from *any* provider. Single binary — verified at ~1 MB container RAM under 2,500 concurrent requests on a Raspberry Pi simulation (1 CPU, 256 MB limit, zero failures).

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

Requires Rust 1.88+. No other dependencies.

---

## ✨ What You Get

| Feature | Details |
|---|---|
| **11 LLM Providers** | OpenAI, Anthropic, OpenRouter, Ollama, DeepSeek, Groq, Together, Fireworks, Mistral, xAI, Perplexity |
| **4 Agent Patterns** | ReAct loop, RAG, Multi-agent Coordinator, Interactive Chat |
| **9 Built-in Tools** | Shell, file read/write, calculator, HTTP, search, knowledge base, JSON transform, code analysis |
| **Memory** | SQLite + FTS5 full-text search with hybrid vector/keyword retrieval |
| **Scheduled Routines** | Cron-based task automation with add/remove/pause/resume |
| **Web UI** | 7-page embedded SPA — Chat, Memory, Tools, Routines, Jobs, Logs, Settings |
| **Streaming** | Real SSE for chat, logs, and events |
| **Security** | Path validation, command sandboxing, WASM tool isolation, configurable autonomy levels |
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
rustedclaw migrate openclaw     Import from OpenClaw
rustedclaw estop [--resume]     Emergency stop / resume
rustedclaw completions <shell>  Generate shell completions
rustedclaw version              Detailed version info
```

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

All numbers independently verified on constrained Docker containers simulating low-end hardware.
Test host: i7-12700F, 32 GB RAM, NVMe — numbers below are **not** host numbers.

| Metric | Raspberry Pi (1 CPU, 256 MB) | $5 VPS (1 CPU, 512 MB) | $10 VPS (2 CPU, 1 GB) |
|---|:---:|:---:|:---:|
| **Idle RAM** | 996 KiB | 996 KiB | 1004 KiB |
| **After 500 req** | 1.05 MiB | 1.02 MiB | 1.03 MiB |
| **After 2500 concurrent** | 1.17 MiB | 1.28 MiB | 1.29 MiB |
| **After 50 chat POSTs** | 1.16 MiB | 1.28 MiB | 1.29 MiB |
| **Failure rate** | 0 / 3000+ | 0 / 1500+ | 0 / 1500+ |
| **Sequential throughput** | 169 req/s | 198 req/s | 207 req/s |

**Machine-independent metrics:**
- Binary size: **3.94 MB** (release, stripped, LTO)
- Docker image: **44 MB** (distroless runtime)
- Threads: **6** (Tokio worker_threads=2 + runtime)
- Container cold start: **~350 ms** (includes Docker overhead)
- Native cold start: **5.4 ms** average (i7-12700F + NVMe)

> RAM growth after thousands of requests: **< 0.3 MB**. No leaks detected.

Run the included scripts to verify on your own hardware:

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
│   ├── providers/   # LLM providers                  (42 tests)
│   ├── channels/    # Input channels                 (38 tests)
│   ├── memory/      # SQLite + FTS5                  (49 tests)
│   ├── tools/       # 9 built-in tools               (67 tests)
│   ├── agent/       # ReAct, RAG, Coordinator        (62 tests)
│   ├── gateway/     # Axum HTTP + SSE + WS           (32 tests)
│   ├── workflow/    # Cron engine                    (16 tests)
│   ├── security/    # Sandboxing + WASM              (40 tests)
│   └── cli/         # Binary entry point + commands   (6 + 17 e2e tests)
├── frontend/        # Embedded SPA (HTML/CSS/JS)
├── scripts/         # Benchmark scripts
├── Dockerfile
├── docker-compose.yml
└── 407 tests, 0 failures
```

---

## 📝 License

[MIT](LICENSE-MIT) — do whatever you want.

---

<p align="center">
  <sub>Built with 🦀 Rust — no account required, no lock-in, no BS.</sub>
</p>

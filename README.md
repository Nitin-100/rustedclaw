<p align="center">
  <img src="assets/logo.png" width="180" alt="RustedClaw">
</p>

<h1 align="center">RustedClaw</h1>

<p align="center">
  <strong>The lightest AI agent runtime you can self-host.<br>~6.6 MB RAM. 3.94 MB binary. Zero runtime dependencies. Zero sign-ups. Zero lock-in.</strong>
</p>

<p align="center">
  <a href="https://github.com/Nitin-100/rustedclaw/actions/workflows/ci.yml"><img src="https://github.com/Nitin-100/rustedclaw/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/Nitin-100/rustedclaw/actions/workflows/bench.yml"><img src="https://github.com/Nitin-100/rustedclaw/actions/workflows/bench.yml/badge.svg" alt="Benchmarks"></a>
  <a href="#-quick-start"><img src="https://img.shields.io/badge/get_started-2_min-brightgreen?style=for-the-badge" alt="Get Started"></a>
  <a href="#-benchmarks"><img src="https://img.shields.io/badge/RAM-6.6_MB-critical?style=for-the-badge" alt="RAM"></a>
  <a href="#-benchmarks"><img src="https://img.shields.io/badge/binary-3.94_MB-blueviolet?style=for-the-badge" alt="Binary Size"></a>
  <a href="LICENSE-MIT"><img src="https://img.shields.io/badge/license-MIT-blue?style=for-the-badge" alt="MIT License"></a>
</p>

---

## 🤯 Why RustedClaw?

Most AI agent runtimes want you to sign up, install databases, pull 300 MB of node_modules, or burn 1.2 GB of RAM doing nothing.

**RustedClaw doesn't.**

```
┌─────────────────────────────────────────────────────────────┐
│  git clone https://github.com/Nitin-100/rustedclaw.git      │
│  cd rustedclaw && cargo build --release                     │
│  export OPENAI_API_KEY="sk-..."                             │
│  ./target/release/rustedclaw gateway                        │
│                                                             │
│  That's it. Web UI at localhost:42617. Chat, tools, memory. │
│  No Docker. No Postgres. No npm. No account. Just run it.   │
└─────────────────────────────────────────────────────────────┘
```

<table>
<tr>
<td width="33%" align="center">

**🪶 Absurdly Light**<br>
6.6 MB idle. 6.9 MB peak under<br>
6,000+ requests. 5 ms cold start.<br>
Your <em>terminal emulator</em> uses more.

</td>
<td width="33%" align="center">

**🔓 Truly Yours**<br>
No account. No telemetry. No vendor.<br>
Bring your own key from 11 providers.<br>
MIT licensed — fork it, sell it, we don't care.

</td>
<td width="33%" align="center">

**🧠 Actually Useful**<br>
4 agent patterns. 9 tools. Memory with<br>
full-text search. Web UI. Cron routines.<br>
Not a toy — a runtime.

</td>
</tr>
</table>

---

## 🧪 Benchmarks

All numbers measured locally on i7-12700F, 32 GB RAM, Windows 11, NVMe. Reproduce: `scripts\benchmark_lowend.ps1`.

| Metric | Native (i7-12700F) | Docker “Raspberry Pi” (1 CPU, 256 MB) | Docker “$5 VPS” (1 CPU, 512 MB) | Docker “$10 VPS” (2 CPU, 1 GB) |
|---|:---:|:---:|:---:|:---:|
| **Idle RAM** | 6.57 MB | 1.02 MiB¹ | 1.03 MiB¹ | 1.03 MiB¹ |
| **Post-load RAM** | 6.86 MB | 1.06 MiB¹ | 1.04 MiB¹ | 1.06 MiB¹ |
| **Peak RAM (concurrent)** | — | 1.20 MiB¹ | 1.18 MiB¹ | 1.17 MiB¹ |
| **Throughput (seq)** | 4,098 req/s | 1,767 req/s | 1,645 req/s | 1,916 req/s |
| **Throughput (5× parallel)** | — | 703 req/s | 1,027 req/s | 1,025 req/s |
| **RAM growth after 6K req** | 0.29 MB | — | — | — |

<sub>¹ Docker cgroup-constrained RSS — the kernel reclaims pages under memory pressure, so reported RSS is lower than on bare metal. Unconstrained native RSS is ~6.6 MB.</sub>

**Machine-independent metrics:**
- Binary size: **3.94 MB** (release, stripped, `opt-level="z"`, LTO)
- Threads: **6** (Tokio `worker_threads=2` + runtime)
- Cold start: **5 ms** P50, **11 ms** avg (i7-12700F + NVMe — expect 15–30 ms on a VPS)

> RAM growth after 6,000+ requests: **0.29 MB**. No leaks detected.

---

## 🌍 The Landscape

There are several open-source AI agent runtimes. Here's how they compare:

| | **RustedClaw** <img src="assets/logo.png" width="18"> | **nullclaw** ⚡ | **ZeroClaw** 🦀 | **IronClaw** 🔗 | **OpenClaw** 🐙 |
|---|:---:|:---:|:---:|:---:|:---:|
| **Language** | Rust | Zig | Rust | Rust | Rust + JS |
| **Account Required** | **No** ✅ | **No** ✅ | **No** ✅ | **Yes** ❌ (NEAR AI) | **No** ✅ |
| **External Deps** | **None** | **None** | **None** | PostgreSQL + pgvector | Node 18 + npm |
| **Binary Size** | **3.94 MB** | **678 KB** 👑 | 8.8 MB | ~15 MB + Postgres | ~300 MB (node_modules) |
| **Idle RAM** | **6.6 MB** | **~1 MB** 👑 | ~8–12 MB¹ | ~50+ MB² | ~1.2 GB |
| **Peak RAM** | **6.9 MB** | — | not published | — | — |
| **Cold Start** | **5 ms** | **<2 ms** 👑 | ~20 ms¹ | ~2 s² | ~4 s |
| **Tests** | **407** | 2843 | not published | not published | not published |
| **Providers** | 11 | 22+ | 28+ | NEAR AI only | varies |
| **Channels** | 6 | 13 | 17 | HTTP only | HTTP + WS |
| **Web UI** | ✅ Embedded | ❌ | ✅ | ✅ | ✅ |
| **Agent Patterns** | 4 (ReAct, RAG, Multi, Chat) | — | skills | tools | tools |
| **Memory** | SQLite + FTS5 | file-based | SQLite + vector | PostgreSQL + pgvector | in-memory |
| **WASM Sandbox** | ✅ (opt-in) | ✅ | ✅ | ✅ | ❌ |
| **License** | MIT | MIT | MIT | MIT + Apache-2.0 | Apache-2.0 |
| **Deployment** | `cargo build` or Docker | Copy 1 file | Copy 1 file | Docker + PostgreSQL | npm install → pray |

<sub>¹ ZeroClaw self-reported for `--help`/`status` (exit immediately). Gateway idle RAM not published. Binary from macOS arm64 release.<br>
² IronClaw requires PostgreSQL + pgvector running alongside — total system footprint much higher.</sub>

### vs. the competition

| They require | We don't |
|---|---|
| NEAR AI account (IronClaw) | **No account** — bring any API key |
| PostgreSQL + pgvector (IronClaw) | **No external deps** — single binary |
| 300 MB node_modules (OpenClaw) | **3.94 MB** — smaller than a JPEG |
| 1.2 GB idle RAM (OpenClaw) | **6.6 MB** — less than your shell |
| No Web UI (nullclaw) | **Built-in Web UI** — 7-page SPA |
| No memory/search (nullclaw) | **SQLite + FTS5** — full-text search |

> **nullclaw** is smaller (Zig). **ZeroClaw** has more providers. But nothing else matches 6.6 MB RAM + Web UI + 4 agent patterns + memory + zero runtime deps in a single binary.

---

## 🚀 Quick Start

### Option A — Build from Source

```bash
git clone https://github.com/Nitin-100/rustedclaw.git && cd rustedclaw
cargo build --release

# First-time setup
./target/release/rustedclaw onboard

# Set your key
export OPENAI_API_KEY="sk-..."

# Start the Web UI + API
./target/release/rustedclaw gateway
```

Open **http://localhost:42617** — done. Requires Rust 1.88+.

### Option B — Docker

```bash
git clone https://github.com/Nitin-100/rustedclaw.git && cd rustedclaw

# Set your API key (pick one)
echo "OPENAI_API_KEY=sk-..." > .env
# or: echo "OPENROUTER_API_KEY=sk-or-v1-..." > .env

docker compose up -d
```

Open **http://localhost:42617** — done. Chat away.

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
| **Agent Contracts** | Declarative behavior guardrails — deny, confirm, warn, or allow tool calls via TOML rules |
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

[[contracts]]
name = "no-rm-rf"
trigger = "tool:shell"
condition = 'args.command CONTAINS "rm -rf"'
action = "deny"
message = "Blocked: rm -rf is forbidden"
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
GET  /v1/contracts              List agent contracts
POST /v1/contracts              Add a contract at runtime
DELETE /v1/contracts/:name      Remove a contract
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
│   ├── providers/   # LLM providers                  (42 tests)
│   ├── channels/    # Input channels                 (38 tests)
│   ├── memory/      # SQLite + FTS5                  (49 tests)
│   ├── tools/       # 9 built-in tools               (67 tests)
│   ├── agent/       # ReAct, RAG, Coordinator        (62 tests)
│   ├── gateway/     # Axum HTTP + SSE + WS           (32 tests)
│   ├── contracts/   # Agent behavior contracts        (33 tests)
│   ├── workflow/    # Cron engine                    (16 tests)
│   ├── security/    # Sandboxing + WASM              (40 tests)
│   └── cli/         # Binary entry point + commands   (6 + 17 e2e tests)
├── frontend/        # Embedded SPA (HTML/CSS/JS)
├── scripts/         # Benchmark scripts
├── Dockerfile
├── docker-compose.yml
└── 440 tests, 0 failures
```

---

## 📝 License

[MIT](LICENSE-MIT) — do whatever you want.

---

<p align="center">
  <sub>Built with 🦀 Rust — no account required, no lock-in, no BS.</sub>
</p>

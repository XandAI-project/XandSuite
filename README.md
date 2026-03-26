# XandSuite

A local LLM desktop application built with Rust, Tauri v2, and React. XandSuite is a full-featured llama.cpp wrapper that lets you download, manage, and run GGUF models locally or connect to a remote OpenAI-compatible server — all from a clean, dark-themed desktop UI.

---

## Features

- **Chat** — ChatGPT-style conversation interface with streaming token output, markdown rendering, and code highlighting
- **Model Manager** — Browse and download GGUF models directly from HuggingFace; manage local models; connect to a remote llama.cpp server
- **RAG** — Ingest PDFs, CSVs, JSON, and plain text files; automatic chunking, embedding, and vector search to augment chat responses
- **Agentic Tasks** — ReAct-pattern agent with tools: web search (DuckDuckGo), code execution, file operations, HTTP API calls, and database queries
- **Flow Builder** — Visual drag-and-drop node editor (React Flow) for building automated prompt pipelines
- **Database Connectors** — Query MongoDB, PostgreSQL, and MySQL directly from the app
- **Settings** — Per-session inference config, HuggingFace API token, remote server URL, agent limits

---

## Tech Stack

| Layer | Technology |
|---|---|
| Desktop shell | Tauri v2 |
| Frontend | React 19, TypeScript, Vite 7, Tailwind CSS v4 |
| UI components | shadcn/ui (Radix UI primitives) |
| State management | Zustand |
| Flow editor | React Flow |
| Backend | Rust (Tokio async runtime) |
| LLM inference | llama.cpp via remote server or `llama-cpp-2` crate (optional) |
| Database (app state) | SQLite (rusqlite, bundled) |
| Database connectors | MongoDB (mongodb crate), PostgreSQL/MySQL (sqlx) |
| HTTP client | reqwest 0.12 with native-tls |
| RAG vector store | In-memory + JSON persistence (oasysdb) |
| Testing | Vitest (frontend), cargo test (Rust) |

---

## Prerequisites

### Required

| Tool | Minimum version | Notes |
|---|---|---|
| [Node.js](https://nodejs.org/) | 20 LTS | Includes npm |
| [Rust](https://rustup.rs/) | 1.80+ | Install via `rustup` |
| Tauri CLI | 2.x | Installed automatically via npm |

### Windows-specific

- **WebView2** — comes pre-installed on Windows 11; on Windows 10 install the [WebView2 Runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/)
- **Microsoft Visual Studio Build Tools 2022** (C++ workload) — required by some Rust crates that compile C dependencies (rusqlite bundled, etc.)

### Linux-specific

```bash
# Ubuntu / Debian
sudo apt update && sudo apt install -y \
  libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
  librsvg2-dev patchelf curl build-essential pkg-config libssl-dev
```

### Optional — Local LLM inference via llama-cpp-2

If you want to run inference directly through the `llama-cpp-2` Rust crate (instead of via the bundled llama-server process):

- **cmake** 3.20+
- **clang** (Linux) or **MSVC** (Windows, included with Visual Studio Build Tools)
- CUDA Toolkit 12.x (for GPU acceleration)

---

## Getting Started

### 1. Clone and install dependencies

```bash
git clone https://github.com/xandnet/xandsuite.git
cd xandsuite
npm install
```

### 2. Configure environment (optional)

Copy the example env file and fill in your tokens:

```bash
cp .env.example .env
```

```env
# .env
HF_API_TOKEN=hf_your_token_here        # HuggingFace token (optional, increases rate limits)
REMOTE_LLM_URL=http://localhost:8080   # Pre-configured remote llama.cpp server (optional)
```

### 3. Run in development mode

```bash
npm run tauri dev
```

This starts the Vite dev server on `http://localhost:1420` and compiles the Rust backend. The first compile takes several minutes as it builds all Rust dependencies.

### 4. Run frontend-only (no Tauri window)

```bash
npm run dev
```

Useful for rapid UI iteration. Tauri `invoke` calls will fail gracefully since there is no backend.

---

## Building for Production

```bash
npm run tauri build
```

Output installers are placed in `src-tauri/target/release/bundle/`:

| Platform | Format | Location |
|---|---|---|
| Windows | `.msi` + NSIS `.exe` | `bundle/msi/`, `bundle/nsis/` |
| Linux | `.AppImage` + `.deb` | `bundle/appimage/`, `bundle/deb/` |

### Build with local llama-cpp-2 inference enabled

```bash
# CPU only
npm run tauri build -- -- --features local-llm

# With CUDA support
npm run tauri build -- -- --features local-llm,cuda
```

---

## Running Tests

```bash
# Frontend unit tests (Vitest)
npm test

# Frontend tests in watch mode
npm run test:watch

# Rust unit + integration tests
cd src-tauri
cargo test
```

---

## Project Structure

```
xandsuite/
├── src/                          # React frontend
│   ├── components/
│   │   ├── chat/                 # Chat view, message bubbles, input bar
│   │   ├── models/               # Model browser & downloader
│   │   ├── agents/               # Agent task view
│   │   ├── flow/                 # Flow canvas & custom nodes
│   │   ├── rag/                  # RAG collection manager
│   │   ├── database/             # Database connector UI
│   │   ├── layout/               # Sidebar, settings view
│   │   └── ui/                   # Shared UI primitives (shadcn/ui wrappers)
│   ├── stores/                   # Zustand state stores
│   └── lib/                      # Tauri IPC helpers, utils
├── src-tauri/
│   ├── src/
│   │   ├── agent/                # ReAct runtime + tool implementations
│   │   ├── commands/             # Tauri IPC command handlers
│   │   ├── db/                   # SQLite, MongoDB, SQL connectors
│   │   ├── engine/               # LLM engine (local stub + remote OpenAI-compat)
│   │   ├── flow/                 # Flow executor + node logic
│   │   ├── hf/                   # HuggingFace scraper + model downloader
│   │   ├── jobs/                 # Background jobs (HF catalog sync)
│   │   ├── models/               # Shared data structures
│   │   ├── rag/                  # RAG pipeline (ingest, chunk, embed, retrieve)
│   │   ├── state.rs              # Shared AppState
│   │   └── lib.rs                # Tauri app entry point
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   └── capabilities/             # Tauri permission scopes
└── package.json
```

---

## Using a Remote llama.cpp Server

XandSuite can connect to any OpenAI-compatible endpoint (llama-server, LM Studio, Ollama, etc.):

1. Open **Model Manager** → **Remote Server** tab
2. Enter the server URL, e.g. `http://localhost:8080`
3. Enter an API key if required
4. Click **Connect** — the app will verify the connection and activate the engine

The remote engine sends requests to `/v1/chat/completions` with streaming enabled.

---

## Configuration

All persistent settings are stored in the app data directory:

| Platform | Path |
|---|---|
| Windows | `%APPDATA%\com.xandnet.xandsuite\` |
| Linux | `~/.local/share/com.xandnet.xandsuite/` |

Key files:
- `xandsuite.db` — SQLite database (conversations, flows, RAG collections, settings)
- `models/` — Downloaded GGUF model files
- `cache/hf_models_cache.json` — HuggingFace model catalog cache
- `vectors/` — RAG vector store (JSON)

---

## Versioning

This project uses semantic versioning: `major.minor.commits`

Current version: **0.1.0**

---

## License

Proprietary — XandNet. All rights reserved.

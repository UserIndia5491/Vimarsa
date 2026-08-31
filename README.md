# Vimarśa

Vimarśa ("विमर्श", reflection) is a repository intelligence desktop app. Paste any public GitHub URL or open a local folder — Vimarśa maps the codebase, traces dependencies, and explains it in plain language, optionally with an AI summary grounded in real static-analysis facts.

## What it does

- **Any repository** — works with public GitHub URLs (`https://github.com/owner/repo`, shallow-cloned to a temp workspace) or any local folder on your machine.
- **Deep static analysis** — maps structure, languages, file roles, symbols, imports/exports, dependency edges, module centrality, entry points, config/build/test files, README and more.
- **AI explanation (optional)** — sends only a compact, trimmed JSON facts packet plus the README to Groq and renders a structured, evidence-based explanation. The source code itself never leaves your machine unless you request an explanation.

## Requirements

- Node.js 18+ and npm
- Rust (stable) and the platform prerequisites for [Tauri v2](https://v2.tauri.app/start/prerequisites/)

## Setup

```bash
npm install
```

## Groq setup (optional, for the AI explanation)

**End users — no terminal or `.env` file needed:**

1. Open Vimarśa and go to the **Explanation** tab.
2. Paste your Groq API key in the **Groq API Key** box and click **Save** (get a key at https://console.groq.com/keys).
3. Click **Generate Explanation**. The key is saved locally in the app and never committed anywhere.

**Developers — optional `.env` fallback for `cargo run`:**

```bash
cp .env.example .env   # then set GROQ_API_KEY inside, or export it in your shell
# optionally set GROQ_MODEL to override the default (openai/gpt-oss-120b)
```

The app prefers the key saved in the UI; `.env` / environment is only used as a fallback during local development.

## Development

```bash
npm run tauri dev
```

## Building

```bash
npm run tauri build
```

## Architecture

- `src-tauri/src/analyzer.rs` — the Rust static-analysis engine: walks the repository (skipping generated/dependency directories), detects languages and frameworks, infers file roles, extracts symbols/imports/exports/constants, builds dependency edges and module centrality, and produces a structured `RepoReport`. It is deliberately independent of the UI.
- `src-tauri/src/github.rs` — GitHub URL validation and shallow `git2` cloning into a temporary directory, plus stale-workspace cleanup.
- `src-tauri/src/groq.rs` — trims the report into a compact JSON facts payload (importance-ranked, size-capped) and calls the Groq API with strict evidence-only prompting.
- `src/main.tsx` — the Tauri/React desktop workbench: GitHub URL and local-folder inputs, progress events, and the Explanation / Overview / Files / Dependencies views.
- `src/types.ts` — shared TypeScript types mirroring the Rust `RepoReport`.

The scanner runs fully locally; only the optional AI explanation sends data (the compact facts plus README) to Groq.
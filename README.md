# Vimarśa

Vimarśa is a local-first repository intelligence desktop app. Select the root folder of a project and it recursively analyzes the complete repository on your machine.

V0.2 includes language/framework detection, file-role inference, symbols, imports/exports, local dependency edges, module centrality, architecture insights, basic code metrics, review flags and a Tauri/React UI.

## Run

```bash
npm install
npm run tauri dev
```

Build installers/packages with:

```bash
npm run tauri build
```

The scanner is in `src-tauri/src/analyzer.rs`. It is deliberately independent of the UI so later versions can add better parsers, graph views, CLI support or an optional AI explanation layer without replacing the core.

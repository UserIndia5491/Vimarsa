import React, { useMemo, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import { open } from '@tauri-apps/plugin-dialog';
import {
  Activity,
  AlertTriangle,
  Box,
  ChevronRight,
  Code2,
  FolderOpen,
  GitBranch,
  Layers3,
  Network,
  Search,
  ShieldAlert,
  Sparkles,
  Timer,
  XCircle,
  type LucideIcon,
} from 'lucide-react';
import './styles.css';

type SymbolInfo = {
  name: string;
  kind: string;
  line: number | null;
};

type ImportRef = {
  raw: string;
  target: string | null;
  external: boolean;
};

type FileReport = {
  path: string;
  language: string;
  bytes: number;
  lines: number | null;
  kind: string;
  role: string;
  confidence: number;
  purpose: string;
  symbols: SymbolInfo[];
  imports: ImportRef[];
  exports: string[];
  metrics: {
    functions: number;
    classes: number;
    comments: number;
    todo_count: number;
    max_line_length: number;
    estimated_complexity: number;
  };
  warnings: string[];
};

type Report = {
  name: string;
  root: string;
  readme: string | null;
  summary: {
    files: number;
    directories: number;
    source_files: number;
    total_bytes: number;
    largest_file: string | null;
    detected_frameworks: string[];
    confidence: number;
  };
  languages: {
    language: string;
    files: number;
    bytes: number;
  }[];
  files: FileReport[];
  modules: {
    path: string;
    incoming: number;
    outgoing: number;
    role: string;
    centrality: number;
  }[];
  edges: {
    from: string;
    to: string;
    kind: string;
  }[];
  architecture: {
    title: string;
    detail: string;
    severity: string;
  }[];
  warnings: string[];
};

const bytes = (n: number) =>
  n < 1024
    ? `${n} B`
    : n < 1024 ** 2
      ? `${(n / 1024).toFixed(1)} KB`
      : n < 1024 ** 3
        ? `${(n / 1024 ** 2).toFixed(1)} MB`
        : `${(n / 1024 ** 3).toFixed(1)} GB`;

const base = (p: string) => p.split('/').pop() || p;

type Stat = [name: string, value: string | number, icon: LucideIcon];
type Metric = [label: string, value: string | number];

function App() {
  const [report, setReport] = useState<Report | null>(null);
  const [selected, setSelected] = useState<FileReport | null>(null);
  const [q, setQ] = useState('');
  const [busy, setBusy] = useState(false);
  const [aiBusy, setAiBusy] = useState(false);
  const [error, setError] = useState('');
  const [aiError, setAiError] = useState('');
  const [aiExplanation, setAiExplanation] = useState('');
  const [apiKey, setApiKey] = useState('');
  const [showAi, setShowAi] = useState(false);

  const filtered = useMemo(
    () =>
      report
        ? report.files.filter((f) =>
            `${f.path} ${f.role} ${f.language}`
              .toLowerCase()
              .includes(q.toLowerCase()),
          )
        : [],
    [report, q],
  );

  async function scan() {
    setError('');
    setAiError('');
    setAiExplanation('');

    const p = await open({
      directory: true,
      multiple: false,
      title: 'Select repository',
    });

    if (!p || Array.isArray(p)) return;

    setBusy(true);

    try {
      const r = await invoke<Report>('analyze', { path: p });

      setReport(r);
      setSelected(
        r.files.find((f) => f.kind === 'source') ??
          r.files[0] ??
          null,
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function explainWithGroq() {
    if (!apiKey.trim()) {
      setAiError('Enter your Groq API key first.');
      return;
    }

    setAiBusy(true);
    setAiError('');
    setShowAi(true);

    try {
      const result = await invoke<string>('explain', {
        apiKey: apiKey.trim(),
      });

      setAiExplanation(result);
    } catch (e) {
      setAiError(String(e));
    } finally {
      setAiBusy(false);
    }
  }

  if (!report) {
    return (
      <div className="landing">
        <div className="brand">
          <div className="mark">वि</div>
          <div>
            <b>Vimarśa</b>
            <small>Repository intelligence, locally.</small>
          </div>
        </div>

        <div className="hero">
          <div className="eyebrow">
            <Sparkles size={14} />
            LOCAL · PRIVATE · STATIC ANALYSIS
          </div>

          <h1>
            Understand the codebase you <span>did not write.</span>
          </h1>

          <p>
            Select the root of any project. Vimarśa recursively maps its
            structure, code roles, symbols and relationships without uploading
            your source.
          </p>

          <button className="primary" onClick={scan}>
            <FolderOpen size={18} />
            Select repository
          </button>

          {busy && (
            <div className="status">
              <Activity size={15} />
              Scanning the whole repository locally…
            </div>
          )}

          {error && (
            <div className="error">
              <XCircle size={15} />
              {error}
            </div>
          )}
        </div>

        <div className="features">
          {[
            ['Recursive', 'Scans the complete repository root.'],
            ['Explainable', 'Shows the evidence behind each inference.'],
            ['AI-assisted', 'Optional Groq explanation from compact facts.'],
            ['Fast', 'Rust performs the filesystem analysis.'],
          ].map((x) => (
            <div key={x[0]}>
              <b>{x[0]}</b>
              <span>{x[1]}</span>
            </div>
          ))}
        </div>
      </div>
    );
  }

  const stats: Stat[] = [
    ['Files', report.summary.files, Box],
    ['Source', report.summary.source_files, Layers3],
    ['Edges', report.edges.length, Network],
    ['Size', bytes(report.summary.total_bytes), Timer],
  ];

  const metrics: Metric[] = [
    ['Lines', selected?.lines ?? '—'],
    ['Functions', selected?.metrics.functions ?? 0],
    ['Classes', selected?.metrics.classes ?? 0],
    ['Branch signals', selected?.metrics.estimated_complexity ?? 0],
    ['Work markers', selected?.metrics.todo_count ?? 0],
  ];

  return (
    <div className="app">
      <header>
        <div className="mini">
          <span className="mark sm">वि</span>
          <b>Vimarśa</b>
        </div>

        <div className="path">
          <GitBranch size={14} />
          {report.root}
        </div>

        <button className="ghost" onClick={scan}>
          <FolderOpen size={15} />
          Open another
        </button>
      </header>

      <div className="layout">
        <aside>
          <div className="section">
            <label>REPOSITORY</label>
            <h3>{report.name}</h3>

            {stats.map(([n, v, Icon]) => (
              <div className="stat" key={n}>
                <span>
                  <Icon size={14} />
                  {n}
                </span>
                <b>{v}</b>
              </div>
            ))}
          </div>

          <div className="section">
            <label>LANGUAGES</label>

            {report.languages
              .filter((x) => x.language !== 'Other')
              .slice(0, 10)
              .map((l) => (
                <div className="line" key={l.language}>
                  <span>{l.language}</span>
                  <span>{l.files}</span>
                </div>
              ))}
          </div>

          <div className="section">
            <label>STACK</label>

            {report.summary.detected_frameworks.length ? (
              report.summary.detected_frameworks.map((x) => (
                <span className="chip" key={x}>
                  {x}
                </span>
              ))
            ) : (
              <div className="muted">
                No framework confidently detected.
              </div>
            )}
          </div>

          <div className="section aiBox">
            <label>AI EXPLANATION</label>

            <input
              type="password"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder="Groq API key"
            />

            <button
              className="primary aiButton"
              onClick={explainWithGroq}
              disabled={aiBusy}
            >
              <Sparkles size={15} />
              {aiBusy ? 'Thinking…' : 'Explain with Groq'}
            </button>

            <small>
              The key is used only for this request and is not stored by
              Vimarśa.
            </small>

            {aiError && (
              <div className="error">
                <XCircle size={14} />
                {aiError}
              </div>
            )}
          </div>
        </aside>

        <main>
          <section className="overview">
            <div>
              <div className="eyebrow">
                <Code2 size={14} />
                REPOSITORY OVERVIEW
              </div>

              <h2>{report.name}</h2>

              <p>
                {report.summary.source_files} source files ·{' '}
                {report.summary.directories} directories ·{' '}
                {report.edges.length} local dependency edges
              </p>
            </div>

            <div className="conf">
              <b>{report.summary.confidence}%</b>
              <span>analysis confidence</span>
            </div>
          </section>

          {showAi && (
            <section className="aiExplanation">
              <div className="aiExplanationHead">
                <div>
                  <div className="eyebrow">
                    <Sparkles size={14} />
                    GROQ REPOSITORY EXPLANATION
                  </div>

                  <h2>What this codebase is doing</h2>
                </div>

                {aiExplanation && (
                  <button
                    className="ghost"
                    onClick={() => setAiExplanation('')}
                  >
                    Clear
                  </button>
                )}
              </div>

              {aiBusy ? (
                <div className="status">
                  <Activity size={15} />
                  Groq is interpreting the scanner's facts…
                </div>
              ) : aiExplanation ? (
                <article className="aiContent">
                  {aiExplanation.split('\n').map((line, index) => {
                    if (line.startsWith('## ')) {
                      return <h3 key={index}>{line.slice(3)}</h3>;
                    }

                    if (line.startsWith('- ')) {
                      return <li key={index}>{line.slice(2)}</li>;
                    }

                    if (!line.trim()) {
                      return (
                        <div className="aiSpacer" key={index} />
                      );
                    }

                    return <p key={index}>{line}</p>;
                  })}
                </article>
              ) : null}
            </section>
          )}

          <section className="insights">
            {report.architecture.map((x, i) => (
              <div
                className={`insight ${x.severity}`}
                key={i}
              >
                {x.severity === 'warning' ? (
                  <AlertTriangle size={16} />
                ) : (
                  <Network size={16} />
                )}

                <div>
                  <b>{x.title}</b>
                  <p>{x.detail}</p>
                </div>
              </div>
            ))}
          </section>

          <section className="workspace">
            <div className="files">
              <div className="panelHead">
                <div>
                  <b>Codebase</b>
                  <small>{filtered.length} matching files</small>
                </div>

                <div className="search">
                  <Search size={15} />

                  <input
                    value={q}
                    onChange={(e) => setQ(e.target.value)}
                    placeholder="Search files or roles"
                  />
                </div>
              </div>

              <div className="list">
                {filtered.map((f) => (
                  <button
                    className={`row ${
                      selected?.path === f.path ? 'active' : ''
                    }`}
                    key={f.path}
                    onClick={() => setSelected(f)}
                  >
                    <Code2 size={15} />

                    <div>
                      <b>{base(f.path)}</b>
                      <small>{f.path}</small>
                    </div>

                    <span>{f.role}</span>
                    <em>{f.confidence}%</em>
                    <ChevronRight size={15} />
                  </button>
                ))}
              </div>
            </div>

            <div className="detail">
              {selected ? (
                <>
                  <div className="detailHead">
                    <div>
                      <h3>{base(selected.path)}</h3>
                      <small>{selected.path}</small>
                    </div>

                    <div className="badges">
                      <span>{selected.language}</span>
                      <span>{selected.role}</span>
                    </div>
                  </div>

                  <div className="purpose">
                    <div className="eyebrow">
                      <Sparkles size={13} />
                      WHAT THIS FILE DOES
                    </div>

                    <p>{selected.purpose}</p>
                  </div>

                  <div className="cards">
                    <div>
                      <label>SYMBOLS</label>
                      <strong>{selected.symbols.length}</strong>

                      {selected.symbols.slice(0, 6).map((s) => (
                        <span key={`${s.name}-${s.line ?? 'unknown'}`}>
                          <b>{s.name}</b>

                          <small>
                            {s.kind}
                            {s.line ? ` · L${s.line}` : ''}
                          </small>
                        </span>
                      ))}
                    </div>

                    <div>
                      <label>DEPENDENCIES</label>
                      <strong>{selected.imports.length}</strong>

                      {selected.imports.slice(0, 6).map((s) => (
                        <span key={s.raw}>
                          <b>{s.raw}</b>
                          <small>
                            {s.external ? 'external' : 'local'}
                          </small>
                        </span>
                      ))}
                    </div>

                    <div>
                      <label>EXPORTS</label>
                      <strong>{selected.exports.length}</strong>

                      {selected.exports.slice(0, 6).map((s) => (
                        <span key={s}>{s}</span>
                      ))}
                    </div>
                  </div>

                  <div className="metrics">
                    {metrics.map(([label, value]) => (
                      <div key={label}>
                        <b>{value}</b>
                        <span>{label}</span>
                      </div>
                    ))}
                  </div>

                  {selected.warnings.length > 0 && (
                    <div className="warnings">
                      <div className="eyebrow">
                        <ShieldAlert size={13} />
                        REVIEW FLAGS
                      </div>

                      {selected.warnings.map((w) => (
                        <p key={w}>
                          <AlertTriangle size={14} />
                          {w}
                        </p>
                      ))}
                    </div>
                  )}

                  <div className="barWrap">
                    <div>
                      <span>Inference confidence</span>
                      <b>{selected.confidence}%</b>
                    </div>

                    <i>
                      <u
                        style={{
                          width: `${selected.confidence}%`,
                        }}
                      />
                    </i>
                  </div>
                </>
              ) : (
                <div className="empty">
                  Select a file to inspect it.
                </div>
              )}
            </div>
          </section>
        </main>
      </div>
    </div>
  );
}

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

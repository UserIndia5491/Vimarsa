import React, { useEffect, useMemo, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import { marked } from 'marked';
import {
  Activity,
  AlertTriangle,
  Box,
  ChevronDown,
  ChevronRight,
  Code2,
  Eye,
  EyeOff,
  FileCode2,
  Folder,
  FolderOpen,
  GitBranch,
  KeyRound,
  Layers3,
  Network,
  Search,
  Settings2,
  ShieldAlert,
  Sparkles,
  Trash2,
  XCircle,
  type LucideIcon,
} from 'lucide-react';
import type { FileReport, ProgressEvent, Report, Tab } from './types';
import './styles.css';

const STORAGE_KEY = 'vimarsa_groq_api_key';
const STORAGE_MODEL = 'vimarsa_groq_model';
const DEFAULT_MODEL = 'openai/gpt-oss-120b';

const bytes = (n: number) =>
  n < 1024
    ? `${n} B`
    : n < 1024 ** 2
      ? `${(n / 1024).toFixed(1)} KB`
      : n < 1024 ** 3
        ? `${(n / 1024 ** 2).toFixed(1)} MB`
        : `${(n / 1024 ** 3).toFixed(1)} GB`;

const base = (p: string) => p.split('/').pop() || p;

const STAGE_LABELS: Record<string, string> = {
  cloning: 'Cloning repository',
  scanning: 'Scanning repository structure',
  preparing_ai: 'Preparing AI summary',
  explaining: 'Generating explanation',
  done: 'Analysis complete',
  error: 'Error',
};

const TABS: { id: Tab; label: string; icon: LucideIcon }[] = [
  { id: 'overview', label: 'Overview', icon: Box },
  { id: 'explanation', label: 'Explanation', icon: Sparkles },
  { id: 'files', label: 'Files', icon: Code2 },
  { id: 'deps', label: 'Dependencies', icon: Network },
];

function StatusBar({
  progress,
  busy,
  aiBusy,
  error,
  aiError,
  report,
  tab,
}: {
  progress: ProgressEvent | null;
  busy: boolean;
  aiBusy: boolean;
  error: string;
  aiError: string;
  report: Report | null;
  tab: Tab;
}) {
  const err = error || aiError;
  const active = busy || aiBusy;
  const stage = progress?.stage;
  const label =
    STAGE_LABELS[stage ?? ''] ?? (active ? 'Working…' : 'Ready');
  const tabLabel = TABS.find((t) => t.id === tab)?.label ?? '';

  return (
    <footer className="statusbar">
      <div className="statusLeft">
        {err ? (
          <>
            <XCircle size={12} />
            <span className="err">{err}</span>
          </>
        ) : active || stage === 'done' || stage === 'error' ? (
          <>
            {active && stage !== 'error' ? (
              <Activity size={12} className="spin" />
            ) : stage === 'error' ? (
              <XCircle size={12} />
            ) : (
              <Sparkles size={12} />
            )}
            <b>{label}</b>
            {progress?.message && <em>{progress.message}</em>}
          </>
        ) : (
          <span>Ready</span>
        )}
      </div>

      {report && (
        <div className="statusRight">
          <span>{report.summary.files} files</span>
          <span>{report.summary.source_files} source</span>
          <span>{report.edges.length} edges</span>
          <span>{bytes(report.summary.total_bytes)}</span>
          <span>{tabLabel}</span>
        </div>
      )}
    </footer>
  );
}

type TreeDir = { name: string; children: Map<string, TreeDir | TreeFile> };
type TreeFile = { name: string; file: FileReport };

function buildTree(files: FileReport[]): TreeDir {
  const root: TreeDir = { name: '', children: new Map() };

  for (const f of files) {
    const parts = f.path.split('/');
    let node = root;

    parts.forEach((part, i) => {
      const last = i === parts.length - 1;

      if (last) {
        node.children.set(part, { name: part, file: f });
        return;
      }

      let dir = node.children.get(part);
      if (!dir || !('children' in dir)) {
        dir = { name: part, children: new Map() };
        node.children.set(part, dir);
      }
      node = dir;
    });
  }

  return root;
}

function TreeView({
  node,
  depth,
  prefix,
  collapsed,
  onToggle,
  selected,
  onSelect,
}: {
  node: TreeDir;
  depth: number;
  prefix: string;
  collapsed: Set<string>;
  onToggle: (path: string) => void;
  selected: FileReport | null;
  onSelect: (f: FileReport) => void;
}) {
  const entries = [...node.children.entries()].sort(([a, an], [b, bn]) => {
    const ad = 'children' in an ? 0 : 1;
    const bd = 'children' in bn ? 0 : 1;
    return ad - bd || a.localeCompare(b);
  });

  return (
    <>
      {entries.map(([name, child]) => {
        if ('children' in child) {
          const fullPath = prefix ? `${prefix}/${child.name}` : child.name;
          const open = !collapsed.has(fullPath);

          return (
            <div key={fullPath}>
              <button
                className="treeRow"
                style={{ paddingLeft: 8 + depth * 14 }}
                onClick={() => onToggle(fullPath)}
              >
                {open ? (
                  <ChevronDown size={12} />
                ) : (
                  <ChevronRight size={12} />
                )}
                <Folder size={13} className="treeIcon" />
                <span>{child.name}</span>
              </button>

              {open && (
                <TreeView
                  node={child}
                  depth={depth + 1}
                  prefix={fullPath}
                  collapsed={collapsed}
                  onToggle={onToggle}
                  selected={selected}
                  onSelect={onSelect}
                />
              )}
            </div>
          );
        }

        return (
          <button
            key={child.file.path}
            className={`treeRow file ${
              selected?.path === child.file.path ? 'active' : ''
            }`}
            style={{ paddingLeft: 8 + depth * 14 + 16 }}
            onClick={() => onSelect(child.file)}
          >
            <FileCode2 size={13} className="treeIcon" />
            <span>{name}</span>
          </button>
        );
      })}
    </>
  );
}
function ExplanationView({
  aiBusy,
  aiError,
  aiExplanation,
  onExplain,
  onClear,
  groqKey,
  groqKeyInput,
  onKeyInputChange,
  onSaveKey,
  onClearKey,
  showKey,
  onToggleShowKey,
  groqModelInput,
  onModelInputChange,
  keySaveMsg,
}: {
  aiBusy: boolean;
  aiError: string;
  aiExplanation: string;
  onExplain: () => void;
  onClear: () => void;
  groqKey: string;
  groqKeyInput: string;
  onKeyInputChange: (v: string) => void;
  onSaveKey: () => void;
  onClearKey: () => void;
  showKey: boolean;
  onToggleShowKey: () => void;
  groqModelInput: string;
  onModelInputChange: (v: string) => void;
  keySaveMsg: string;
}) {
  const hasKey = groqKey.trim().length > 0;
  const inputHasKey = groqKeyInput.trim().length > 0;

  return (
    <section className="tabView explainView">
      <div className="apiKeyCard">
        <div className="apiKeyHead">
          <span className="apiKeyIcon">
            <KeyRound size={14} />
          </span>
          <div className="apiKeyTitle">
            <b>Groq API Key</b>
            <span>
              No .env file needed — paste your key here. It stays in this app.
              <a href="https://console.groq.com/keys" target="_blank" rel="noreferrer">
                Get a key at console.groq.com
              </a>
            </span>
          </div>
          <span className={`keyStatus ${hasKey ? 'ok' : 'missing'}`}>
            <i className="dot" />
            {hasKey ? 'Key saved' : 'No key'}
          </span>
        </div>

        <div className="apiKeyRow">
          <div className="apiKeyInputWrap">
            <input
              type={showKey ? 'text' : 'password'}
              value={groqKeyInput}
              onChange={(e) => onKeyInputChange(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') onSaveKey();
              }}
              placeholder="gsk_..."
              spellCheck={false}
              autoComplete="off"
            />
            <button
              type="button"
              className="iconBtn"
              title={showKey ? 'Hide key' : 'Show key'}
              onClick={onToggleShowKey}
            >
              {showKey ? <EyeOff size={14} /> : <Eye size={14} />}
            </button>
          </div>
          <button className="btn primary" onClick={onSaveKey} disabled={!inputHasKey}>
            Save
          </button>
          <button className="btn" onClick={onClearKey} disabled={!hasKey && !inputHasKey} title="Remove saved key">
            <Trash2 size={13} />
            Clear
          </button>
        </div>

        <div className="apiKeyMeta">
          <div className="apiKeyModelRow">
            <Settings2 size={11} />
            <span>Model</span>
            <input
              value={groqModelInput}
              onChange={(e) => onModelInputChange(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === 'Enter') onSaveKey();
              }}
              placeholder={DEFAULT_MODEL}
              spellCheck={false}
            />
            <span className="faint">default: {DEFAULT_MODEL}</span>
          </div>
          {keySaveMsg && <span className="keySaveMsg">{keySaveMsg}</span>}
          <span className="apiKeyHint">
            Key is stored locally with <code>localStorage</code> and sent only to <code>api.groq.com</code> when you click Generate. Never committed to your repo.
          </span>
        </div>

        {!hasKey && (
          <div className="apiKeyWarn">
            <AlertTriangle size={12} />
            Add your Groq key above to enable AI explanations. Basic users don't need to create any <code>.env</code> file.
          </div>
        )}
      </div>

      <div className="explainHeader">
        <div className="explainTitle">
          <span className="explainIcon">
            <Sparkles size={16} />
          </span>
          <div>
            <h2>AI Explanation</h2>
            <p>Grounded in static analysis — no hallucinations</p>
          </div>
        </div>
        <div className="explainActions">
          {aiExplanation && !aiBusy && (
            <button className="btn ghost" onClick={onClear}>
              Clear
            </button>
          )}
          <button
            className="btn primary"
            onClick={onExplain}
            disabled={aiBusy || !hasKey}
            title={!hasKey ? 'Save your Groq API key first' : undefined}
          >
            <Sparkles size={13} />
            {aiBusy ? 'Thinking…' : 'Generate Explanation'}
          </button>
        </div>
      </div>

      {aiError && (
        <div className="explainError">
          <XCircle size={14} />
          <span>{aiError}</span>
        </div>
      )}

      {aiBusy && (
        <div className="explainLoading">
          <div className="loadingBar" style={{ width: '68%' }} />
          <div className="loadingBar" style={{ width: '92%' }} />
          <div className="loadingBar" style={{ width: '78%' }} />
          <div className="loadingBar short" style={{ width: '45%' }} />
          <div className="loadingPulse">
            <Activity size={12} className="spin" />
            Generating explanation… this can take 10–30 seconds
          </div>
        </div>
      )}

      {!aiBusy && aiExplanation ? (
        <article
          className="aiContent explainContent"
          dangerouslySetInnerHTML={{
            __html: marked.parse(aiExplanation) as string,
          }}
        />
      ) : (
        !aiBusy &&
        !aiError && (
          <div className="explainEmpty">
            <div className="explainEmptyIcon">
              <Sparkles size={22} />
            </div>
            <h3>Get a plain-language walkthrough</h3>
            <p>
              Vimarśa sends a compact, size-capped facts packet to the AI service — not
              your source code. You’ll get 11 structured sections covering
              purpose, architecture, data flow, key files, and what the scanner
              couldn’t determine.
            </p>
            <div className="explainChips">
              <span>What it is</span>
              <span>How parts connect</span>
              <span>Key files</span>
              <span>Data flow</span>
              <span>How to run</span>
              <span>Tech stack</span>
            </div>
            <span className="explainHint">
              Only a trimmed JSON summary plus README is sent — your code stays
              on your machine. Retries automatically at smaller sizes if the
              service rejects the payload.
            </span>
          </div>
        )
      )}
    </section>
  );
}

function OverviewView({ report }: { report: Report }) {
  return (
    <section className="tabView">
      <div className="statStrip">
        <div>
          <label>FILES</label>
          <b>{report.summary.files}</b>
        </div>
        <div>
          <label>SOURCE</label>
          <b>{report.summary.source_files}</b>
        </div>
        <div>
          <label>DIRECTORIES</label>
          <b>{report.summary.directories}</b>
        </div>
        <div>
          <label>SIZE</label>
          <b>{bytes(report.summary.total_bytes)}</b>
        </div>
        <div className="statWide">
          <label>LARGEST FILE</label>
          <b className="mono" title={report.summary.largest_file ?? ''}>
            {report.summary.largest_file ?? '—'}
          </b>
        </div>
      </div>

      <div className="panelGrid">
        <div className="panel">
          <header>LANGUAGES</header>
          <div className="panelBody">
            {report.languages
              .filter((l) => l.language !== 'Other')
              .map((l) => (
                <div className="rowLine" key={l.language}>
                  <span>{l.language}</span>
                  <em>
                    {l.files} files · {bytes(l.bytes)}
                  </em>
                </div>
              ))}
          </div>
        </div>

        <div className="panel">
          <header>STACK</header>
          <div className="panelBody">
            {report.summary.detected_frameworks.length ? (
              <div className="chipRow">
                {report.summary.detected_frameworks.map((x) => (
                  <span className="chip" key={x}>
                    {x}
                  </span>
                ))}
              </div>
            ) : (
              <div className="muted">No framework confidently detected.</div>
            )}
          </div>
        </div>

        <div className="panel">
          <header>ENTRY POINTS</header>
          <div className="panelBody">
            {report.entry_points.length ? (
              <div className="monoList">
                {report.entry_points.map((p) => (
                  <span key={p} title={p}>
                    {p}
                  </span>
                ))}
              </div>
            ) : (
              <div className="muted">None detected.</div>
            )}
          </div>
        </div>

        <div className="panel">
          <header>CONFIG FILES</header>
          <div className="panelBody">
            {report.config_files.length ? (
              <div className="chipRow">
                {report.config_files.map((p) => (
                  <span className="chip" key={p}>
                    {p}
                  </span>
                ))}
              </div>
            ) : (
              <div className="muted">None detected.</div>
            )}
          </div>
        </div>

        <div className="panel">
          <header>BUILD FILES</header>
          <div className="panelBody">
            {report.build_files.length ? (
              <div className="chipRow">
                {report.build_files.map((p) => (
                  <span className="chip" key={p}>
                    {p}
                  </span>
                ))}
              </div>
            ) : (
              <div className="muted">None detected.</div>
            )}
          </div>
        </div>

        <div className="panel">
          <header>TEST FILES</header>
          <div className="panelBody">
            {report.test_files.length ? (
              <div className="chipRow">
                {report.test_files.map((p) => (
                  <span className="chip" key={p}>
                    {p}
                  </span>
                ))}
              </div>
            ) : (
              <div className="muted">None detected.</div>
            )}
          </div>
        </div>

        <div className="panel span3">
          <header>ARCHITECTURE INSIGHTS</header>
          <div className="panelBody">
            {report.architecture.map((x, i) => (
              <div
                className={`insightRow ${x.severity === 'warning' ? 'warning' : ''}`}
                key={i}
              >
                {x.severity === 'warning' ? (
                  <AlertTriangle size={14} />
                ) : (
                  <Network size={14} />
                )}
                <div>
                  <b>{x.title}</b>
                  <p>{x.detail}</p>
                </div>
              </div>
            ))}
          </div>
        </div>

        <div className="panel span3">
          <header>
            <span>README</span>
            {report.readme && (
              <b>
                {report.readme.path}
                {report.readme.truncated ? ' (truncated)' : ''}
              </b>
            )}
          </header>
          <div className="panelBody">
            {report.readme ? (
              <article
                className="aiContent readmeRendered"
                dangerouslySetInnerHTML={{
                  __html: marked.parse(report.readme.content) as string,
                }}
              />
            ) : (
              <div className="muted">No README detected.</div>
            )}
          </div>
        </div>

        {report.warnings.length > 0 && (
          <div className="panel span3">
            <header>SCANNER NOTES</header>
            <div className="panelBody">
              {report.warnings.map((w) => (
                <div className="insightRow warning" key={w}>
                  <AlertTriangle size={14} />
                  <p>{w}</p>
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </section>
  );
}
function DepsView({ report }: { report: Report }) {
  const modules = [...report.modules].sort(
    (a, b) => b.centrality - a.centrality,
  );

  return (
    <section className="tabView">
      <div className="depsGrid">
        <div className="panel">
          <header>EXTERNAL DEPENDENCIES ({report.external_deps.length})</header>
          <div className="panelBody">
            {report.external_deps.length ? (
              report.external_deps.map((d) => (
                <div className="rowLine" key={d.name}>
                  <span>{d.name}</span>
                  <em>{d.count} refs</em>
                </div>
              ))
            ) : (
              <div className="muted">No external dependencies detected.</div>
            )}
          </div>
        </div>

        <div className="panel">
          <header>MODULES BY CENTRALITY ({report.modules.length})</header>
          <div className="panelBody">
            {modules.length ? (
              modules.map((m) => (
                <div className="rowLine" key={m.path}>
                  <span title={m.path}>{m.path}</span>
                  <em>
                    {m.incoming} in · {m.outgoing} out
                  </em>
                </div>
              ))
            ) : (
              <div className="muted">No modules detected.</div>
            )}
          </div>
        </div>

        <div className="panel span2">
          <header>LOCAL DEPENDENCY EDGES ({report.edges.length})</header>
          <div className="panelBody">
            {report.edges.length ? (
              <div className="edgesScroll">
                {report.edges.map((e) => (
                  <div className="edgeRow" key={`${e.from}->${e.to}`}>
                    <code>{e.from}</code>
                    <span>→</span>
                    <code>{e.to}</code>
                  </div>
                ))}
              </div>
            ) : (
              <div className="muted">No local edges detected.</div>
            )}
          </div>
        </div>
      </div>
    </section>
  );
}

function FileDetailView({
  selected,
}: {
  selected: FileReport;
}) {
  const metrics: [string, string | number][] = [
    ['Lines', selected.lines ?? '—'],
    ['Functions', selected.metrics.functions],
    ['Classes', selected.metrics.classes],
    ['Branch signals', selected.metrics.estimated_complexity],
    ['Work markers', selected.metrics.todo_count],
  ];

  return (
    <section className="tabView">
      <div className="fileDetail">
        <div className="fileDetailHead">
          <div className="fileTitle">
            <h3>{base(selected.path)}</h3>
            <span className="mono">{selected.path}</span>
          </div>

          <div className="badges">
            <span>{selected.language === 'Other' ? 'Unknown' : selected.language === 'Binary' ? 'Binary' : selected.language}</span>
            <span>{selected.role}</span>
          </div>
        </div>

        <div className="filePurpose">
          <label>WHAT THIS FILE DOES</label>
          <p>{selected.purpose}</p>
        </div>

        <div className="fileCards">
          <div>
            <label>SYMBOLS</label>
            <strong>{selected.symbols.length}</strong>

            {selected.symbols.slice(0, 6).map((s) => (
              <span className="cardItem" key={`${s.name}-${s.line ?? 'u'}`}>
                {s.name}
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
              <span className="cardItem" key={s.raw}>
                {s.raw}
                <small>{s.external ? 'external' : 'local'}</small>
              </span>
            ))}
          </div>

          <div>
            <label>EXPORTS</label>
            <strong>{selected.exports.length}</strong>

            {selected.exports.slice(0, 6).map((s) => (
              <span className="cardItem" key={s}>
                {s}
              </span>
            ))}
          </div>
        </div>

        <div className="fileMetrics">
          {metrics.map(([label, value]) => (
            <div key={label}>
              <b>{value}</b>
              <span>{label}</span>
            </div>
          ))}
        </div>

        {selected.warnings.length > 0 && (
          <div className="warnings">
            <label>REVIEW FLAGS</label>
            {selected.warnings.map((w) => (
              <p key={w}>
                <AlertTriangle size={13} />
                {w}
              </p>
            ))}
          </div>
        )}
      </div>
    </section>
  );
}

function EmptyState({
  busy,
  repoUrl,
  onUrlChange,
  onAnalyzeUrl,
  onOpenLocal,
}: {
  busy: boolean;
  repoUrl: string;
  onUrlChange: (v: string) => void;
  onAnalyzeUrl: () => void;
  onOpenLocal: () => void;
}) {
  return (
    <div className="emptyState landing">
      <div className="landingHero">
        <div className="landingEyebrow">
          <Sparkles size={11} />
          Repository Intelligence
        </div>
        <div className="landingMark" aria-hidden>
          <svg width="30" height="30" viewBox="0 0 24 24" fill="none">
            <path
              d="M12 3.5L19.5 12L12 20.5L4.5 12L12 3.5Z"
              stroke="currentColor"
              strokeWidth="1.8"
              strokeLinejoin="round"
            />
            <circle cx="12" cy="12" r="3.2" fill="currentColor" />
          </svg>
        </div>
        <h1>
          Understand any <span>codebase</span> in seconds.
        </h1>
        <p className="landingSub">
          Vimarśa maps structure, traces dependencies, and explains code in
          plain language — for any GitHub repository or local folder. AI
          summaries are grounded in real static analysis, not hallucinations.
        </p>
      </div>

      <div className="landingActions">
        <form
          className="landingCard primary"
          onSubmit={(e) => {
            e.preventDefault();
            onAnalyzeUrl();
          }}
        >
          <label>
            <GitBranch size={12} />
            GitHub repository
          </label>
          <div className="landingInputRow">
            <input
              value={repoUrl}
              onChange={(e) => onUrlChange(e.target.value)}
              placeholder="https://github.com/owner/repo"
              disabled={busy}
              spellCheck={false}
            />
            <button className="btn primary" type="submit" disabled={busy}>
              {busy ? (
                <>
                  <Activity size={13} className="spin" />
                  Analyzing…
                </>
              ) : (
                <>
                  <Search size={13} />
                  Analyze
                </>
              )}
            </button>
          </div>
          <span className="landingHint">
            Public repos are shallow-cloned to a temp workspace and analyzed
            locally. Nothing is uploaded unless you request an AI explanation.
          </span>
        </form>

        <div className="landingDivider">
          <i />
          <span>or</span>
          <i />
        </div>

        <button
          className="landingCard folder"
          onClick={onOpenLocal}
          disabled={busy}
        >
          <span className="folderIcon">
            <FolderOpen size={18} />
          </span>
          <span className="folderText">
            <b>Open a local folder</b>
            <em>Pick any project on your machine</em>
          </span>
          <ChevronRight size={14} className="folderArrow" />
        </button>
      </div>

      <div className="landingFeatures">
        <div className="feat">
          <span className="featIcon">
            <Layers3 size={16} />
          </span>
          <b>Map</b>
          <p>Languages, entry points, module centrality & full dependency graph</p>
        </div>
        <div className="feat">
          <span className="featIcon accent">
            <Sparkles size={16} />
          </span>
          <b>Explain</b>
          <p>AI-powered summary — strictly evidence-based, beginner friendly</p>
        </div>
        <div className="feat">
          <span className="featIcon">
            <Network size={16} />
          </span>
          <b>Navigate</b>
          <p>Browse files, symbols, imports & exports</p>
        </div>
      </div>

      <div className="landingFoot">
        <ShieldAlert size={11} />
        Your code stays on your machine. Only a compact, trimmed facts packet
        is sent to the AI service when you click “Generate Explanation”.
      </div>
    </div>
  );
}

function Sidebar({
  report,
  tab,
  onTab,
  query,
  onQuery,
  selected,
  onSelect,
  collapsed,
  onToggleDir,
}: {
  report: Report;
  tab: Tab;
  onTab: (t: Tab) => void;
  query: string;
  onQuery: (q: string) => void;
  selected: FileReport | null;
  onSelect: (f: FileReport) => void;
  collapsed: Set<string>;
  onToggleDir: (path: string) => void;
}) {
  const tree = useMemo(() => buildTree(report.files), [report]);

  const filtered = useMemo(
    () =>
      report.files.filter((f) =>
        `${f.path} ${f.role} ${f.language}`
          .toLowerCase()
          .includes(query.toLowerCase()),
      ),
    [report, query],
  );

  return (
    <aside className="sidebar">
      <div className="sidebarRepo">
        <b title={report.name}>{report.name}</b>
        <small title={report.url ?? report.root}>
          {report.url ?? report.root}
        </small>
      </div>

      <nav className="sidebarNav">
        {TABS.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            className={tab === id ? 'active' : ''}
            onClick={() => onTab(id)}
          >
            <Icon size={14} />
            <span>{label}</span>
            {id === 'files' && <em>{report.files.length}</em>}
          </button>
        ))}
      </nav>

      {tab === 'files' && (
        <div className="sidebarFiles">
          <div className="search">
            <Search size={13} />
            <input
              value={query}
              onChange={(e) => onQuery(e.target.value)}
              placeholder="Filter files"
            />
          </div>

          <div className="tree">
            {query.trim() ? (
              filtered.map((f) => (
                <button
                  key={f.path}
                  className={`treeRow file ${
                    selected?.path === f.path ? 'active' : ''
                  }`}
                  style={{ paddingLeft: 10 }}
                  onClick={() => onSelect(f)}
                >
                  <FileCode2 size={13} className="treeIcon" />
                  <span>{f.path}</span>
                </button>
              ))
            ) : (
              <TreeView
                node={tree}
                depth={0}
                prefix=""
                collapsed={collapsed}
                onToggle={onToggleDir}
                selected={selected}
                onSelect={onSelect}
              />
            )}
          </div>
        </div>
      )}
    </aside>
  );
}
function App() {
  const [report, setReport] = useState<Report | null>(null);
  const [selected, setSelected] = useState<FileReport | null>(null);
  const [tab, setTab] = useState<Tab>('overview');
  const [q, setQ] = useState('');
  const [repoUrl, setRepoUrl] = useState('');
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [busy, setBusy] = useState(false);
  const [aiBusy, setAiBusy] = useState(false);
  const [error, setError] = useState('');
  const [aiError, setAiError] = useState('');
  const [aiExplanation, setAiExplanation] = useState('');
  const [progress, setProgress] = useState<ProgressEvent | null>(null);
  // --- Groq key: stored in localStorage, no .env file needed for end users ---
  const [groqKey, setGroqKey] = useState(() => {
    try {
      return localStorage.getItem(STORAGE_KEY) ?? '';
    } catch {
      return '';
    }
  });
  const [groqKeyInput, setGroqKeyInput] = useState(() => {
    try {
      return localStorage.getItem(STORAGE_KEY) ?? '';
    } catch {
      return '';
    }
  });
  const [groqModelInput, setGroqModelInput] = useState(() => {
    try {
      return localStorage.getItem(STORAGE_MODEL) ?? DEFAULT_MODEL;
    } catch {
      return DEFAULT_MODEL;
    }
  });
  const [showKey, setShowKey] = useState(false);
  const [keySaveMsg, setKeySaveMsg] = useState('');

  const handleSaveKey = () => {
    const k = groqKeyInput.trim();
    const m = groqModelInput.trim() || DEFAULT_MODEL;
    if (!k) {
      setKeySaveMsg('Paste a key first (starts with gsk_…).');
      setTimeout(() => setKeySaveMsg(''), 2500);
      return;
    }
    try {
      localStorage.setItem(STORAGE_KEY, k);
      localStorage.setItem(STORAGE_MODEL, m);
    } catch {}
    setGroqKey(k);
    setKeySaveMsg('✓ Saved locally — key never leaves your machine except to call Groq.');
    setTimeout(() => setKeySaveMsg(''), 3000);
  };

  const handleClearKey = () => {
    try {
      localStorage.removeItem(STORAGE_KEY);
    } catch {}
    setGroqKey('');
    setGroqKeyInput('');
    setKeySaveMsg('Key cleared.');
    setTimeout(() => setKeySaveMsg(''), 2000);
    setAiError('');
  };

  // persist model whenever it changes (so user doesn't need to click Save for model-only changes)
  useEffect(() => {
    try {
      const m = groqModelInput.trim() || DEFAULT_MODEL;
      localStorage.setItem(STORAGE_MODEL, m);
    } catch {}
  }, [groqModelInput]);

  useEffect(() => {
    let unlisten: (() => void) | undefined;

    listen<ProgressEvent>('vimarsa-progress', (event) => {
      setProgress(event.payload);
    }).then((fn) => {
      unlisten = fn;
    });

    return () => {
      unlisten?.();
    };
  }, []);

  function pickDefaultFile(r: Report) {
    return r.files.find((f) => f.kind === 'source') ?? r.files[0] ?? null;
  }

  function resetErrors() {
    setError('');
    setAiError('');
    setAiExplanation('');
    setProgress(null);
  }

  function toggleDir(path: string) {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  }

  async function scanLocal() {
    resetErrors();

    const p = await open({
      directory: true,
      multiple: false,
      title: 'Select repository',
    });

    if (!p || Array.isArray(p)) return;

    setBusy(true);

    try {
      const r = await invoke<Report>('analyze_local_path', { path: p });

      setReport(r);
      setCollapsed(new Set());
      setTab('overview');
      setSelected(pickDefaultFile(r));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function analyzeUrl() {
    const url = repoUrl.trim();

    resetErrors();

    if (!url) {
      setError('Enter a GitHub repository URL.');
      return;
    }

    setBusy(true);

    try {
      const r = await invoke<Report>('analyze_repository_url', { url });

      setReport(r);
      setCollapsed(new Set());
      setTab('overview');
      setSelected(pickDefaultFile(r));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function explainWithGroq() {
    setAiError('');
    if (!groqKey.trim()) {
      setAiError(
        'No Groq API key saved. Paste your key in the box above and click Save, then try again. Get a key at https://console.groq.com/keys',
      );
      return;
    }
    setProgress(null);
    setAiBusy(true);

    try {
      const result = await invoke<string>('explain', {
        api_key: groqKey.trim(),
        model: groqModelInput.trim() || DEFAULT_MODEL,
      });
      setAiExplanation(result);
    } catch (e) {
      setAiError(String(e));
    } finally {
      setAiBusy(false);
    }
  }

  return (
    <div className="app">
      <header className="toolbar">
        <div className="toolbarBrand">
          <span className="mark" aria-hidden>
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none">
              <path
                d="M12 3.5L19.5 12L12 20.5L4.5 12L12 3.5Z"
                stroke="currentColor"
                strokeWidth="1.8"
                strokeLinejoin="round"
              />
              <circle cx="12" cy="12" r="3.2" fill="currentColor" />
            </svg>
          </span>
          <b>Vimarśa</b>
        </div>

        {report && (
          <div className="toolbarPath" title={report.url ?? report.root}>
            <GitBranch size={13} />
            <span>{report.url ?? report.root}</span>
          </div>
        )}

        <form
          className="toolbarUrl"
          onSubmit={(e) => {
            e.preventDefault();
            analyzeUrl();
          }}
        >
          <input
            value={repoUrl}
            onChange={(e) => setRepoUrl(e.target.value)}
            placeholder="https://github.com/owner/repo"
            disabled={busy}
          />
          <button className="btn primary" type="submit" disabled={busy}>
            <GitBranch size={13} />
            {busy ? 'Analyzing…' : 'Analyze'}
          </button>
        </form>

        <div className="toolbarSep" />

        <button className="btn" onClick={scanLocal} disabled={busy}>
          <FolderOpen size={13} />
          Open folder
        </button>
      </header>

      <div className={`body ${report ? '' : 'noSidebar'}`}>
        {report && (
          <Sidebar
            report={report}
            tab={tab}
            onTab={setTab}
            query={q}
            onQuery={setQ}
            selected={selected}
            onSelect={setSelected}
            collapsed={collapsed}
            onToggleDir={toggleDir}
          />
        )}

        <main className={`main ${!report ? 'landingMain' : ''}`}>
          {!report ? (
            <EmptyState
              busy={busy}
              repoUrl={repoUrl}
              onUrlChange={setRepoUrl}
              onAnalyzeUrl={analyzeUrl}
              onOpenLocal={scanLocal}
            />
          ) : (
            <div className="tabView">
              {tab === 'overview' && <OverviewView report={report} />}

              {tab === 'explanation' && (
                <ExplanationView
                  aiBusy={aiBusy}
                  aiError={aiError}
                  aiExplanation={aiExplanation}
                  onExplain={explainWithGroq}
                  onClear={() => {
                    setAiExplanation('');
                    setAiError('');
                  }}
                  groqKey={groqKey}
                  groqKeyInput={groqKeyInput}
                  onKeyInputChange={setGroqKeyInput}
                  onSaveKey={handleSaveKey}
                  onClearKey={handleClearKey}
                  showKey={showKey}
                  onToggleShowKey={() => setShowKey((v) => !v)}
                  groqModelInput={groqModelInput}
                  onModelInputChange={setGroqModelInput}
                  keySaveMsg={keySaveMsg}
                />
              )}

              {tab === 'files' &&
                (selected ? (
                  <FileDetailView selected={selected} />
                ) : (
                  <div className="emptyPanel">
                    Select a file in the sidebar.
                  </div>
                ))}

              {tab === 'deps' && <DepsView report={report} />}
            </div>
          )}
        </main>
      </div>

      <StatusBar
        progress={progress}
        busy={busy}
        aiBusy={aiBusy}
        error={error}
        aiError={aiError}
        report={report}
        tab={tab}
      />
    </div>
  );
}

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);

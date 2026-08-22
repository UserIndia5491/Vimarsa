export type SymbolInfo = {
  name: string;
  kind: string;
  line: number | null;
};

export type ImportRef = {
  raw: string;
  target: string | null;
  external: boolean;
};

export type ReadmeInfo = {
  path: string;
  content: string;
  bytes: number;
  truncated: boolean;
};

export type TreeNode = {
  path: string;
  is_dir: boolean;
  depth: number;
};

export type ExternalDep = {
  name: string;
  count: number;
};

export type FileReport = {
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
  constants: string[];
  doc: string | null;
  public_api: string[];
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

export type Report = {
  name: string;
  root: string;
  url: string | null;
  readme: ReadmeInfo | null;
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
  directory_tree: TreeNode[];
  entry_points: string[];
  config_files: string[];
  build_files: string[];
  test_files: string[];
  external_deps: ExternalDep[];
  warnings: string[];
};

export type ProgressEvent = {
  stage: string;
  message: string;
};

export type Tab = 'explanation' | 'overview' | 'files' | 'deps';
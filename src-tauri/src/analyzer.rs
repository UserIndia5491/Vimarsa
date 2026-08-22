use anyhow::{anyhow, Context, Result};
use ignore::WalkBuilder;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    path::{Path, PathBuf},
};

const MAX_SOURCE_BYTES: u64 = 1_500_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoReport {
    pub name: String,
    pub root: String,
    pub url: Option<String>,
    pub readme: Option<ReadmeInfo>,
    pub summary: RepoSummary,
    pub languages: Vec<LanguageStat>,
    pub files: Vec<FileReport>,
    pub modules: Vec<ModuleReport>,
    pub edges: Vec<DependencyEdge>,
    pub architecture: Vec<ArchitectureInsight>,
    pub directory_tree: Vec<TreeNode>,
    pub entry_points: Vec<String>,
    pub config_files: Vec<String>,
    pub build_files: Vec<String>,
    pub test_files: Vec<String>,
    pub external_deps: Vec<ExternalDep>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReadmeInfo {
    pub path: String,
    pub content: String,
    pub bytes: u64,
    pub truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeNode {
    pub path: String,
    pub is_dir: bool,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExternalDep {
    pub name: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RepoSummary {
    pub files: usize,
    pub directories: usize,
    pub source_files: usize,
    pub total_bytes: u64,
    pub largest_file: Option<String>,
    pub detected_frameworks: Vec<String>,
    pub confidence: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LanguageStat {
    pub language: String,
    pub files: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileReport {
    pub path: String,
    pub language: String,
    pub bytes: u64,
    pub lines: Option<usize>,
    pub kind: String,
    pub role: String,
    pub confidence: u8,
    pub purpose: String,
    pub symbols: Vec<Symbol>,
    pub imports: Vec<ImportRef>,
    pub exports: Vec<String>,
    pub constants: Vec<String>,
    pub doc: Option<String>,
    pub public_api: Vec<String>,
    pub metrics: CodeMetrics,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Symbol {
    pub name: String,
    pub kind: String,
    pub line: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportRef {
    pub raw: String,
    pub target: Option<String>,
    pub external: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CodeMetrics {
    pub functions: usize,
    pub classes: usize,
    pub comments: usize,
    pub todo_count: usize,
    pub max_line_length: usize,
    pub estimated_complexity: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleReport {
    pub path: String,
    pub incoming: usize,
    pub outgoing: usize,
    pub role: String,
    pub centrality: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyEdge {
    pub from: String,
    pub to: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitectureInsight {
    pub title: String,
    pub detail: String,
    pub severity: String,
}

pub fn analyze_repository(root: &str) -> Result<RepoReport> {
    let root_path = PathBuf::from(root);

    if !root_path.is_dir() {
        return Err(anyhow!("Not a directory: {root}"));
    }

    let mut files: Vec<FileReport> = Vec::new();
    let mut directories: BTreeSet<String> = BTreeSet::new();
    let mut language_acc: BTreeMap<String, (usize, u64)> = BTreeMap::new();
    let mut frameworks: BTreeSet<String> = BTreeSet::new();

    let mut total_bytes: u64 = 0;
    let mut largest: Option<(u64, String)> = None;

    let walker = WalkBuilder::new(&root_path)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .filter_entry(|entry| {
            !ignored_dir(entry.file_name().to_string_lossy().as_ref())
        })
        .build();

    for item in walker {
        let entry = item.context("walking repository")?;
        let path = entry.path();

        if path.is_dir() {
            if let Ok(rel) = path.strip_prefix(&root_path) {
                if !rel.as_os_str().is_empty() {
                    directories.insert(norm(&rel.to_string_lossy()));
                }
            }

            continue;
        }

        let bytes = entry
            .metadata()
            .context("reading file metadata")?
            .len();

        total_bytes += bytes;

        let rel = norm(
            &path
                .strip_prefix(&root_path)
                .unwrap_or(path)
                .to_string_lossy(),
        );

        if largest
            .as_ref()
            .map(|value| value.0)
            .unwrap_or(0)
            < bytes
        {
            largest = Some((bytes, rel.clone()));
        }

        let language = detect_language(path);

        let source = if bytes <= MAX_SOURCE_BYTES && is_text_language(&language) {
            fs::read_to_string(path).ok()
        } else {
            None
        };

        if let Some(source_text) = source.as_deref() {
            detect_frameworks(source_text, &language, &rel, &mut frameworks);
        }

        let report = analyze_file(
            &rel,
            bytes,
            &language,
            source.as_deref(),
        );

        let acc = language_acc
            .entry(language.clone())
            .or_insert((0, 0));

        acc.0 += 1;
        acc.1 += bytes;

        files.push(report);
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));

    let readme = detect_readme(&root_path);
    let directory_tree = build_directory_tree(&directories, &files);
    let entry_points = detect_entry_points(&files);
    let config_files = collect_by_kind(&files, "configuration");
    let test_files = collect_by_kind(&files, "test");
    let build_files = detect_build_files(&files);
    let external_deps = aggregate_external_deps(&files);

    let edges = build_edges(&files);
    let modules = build_modules(&files, &edges);
    let architecture = infer_architecture(&files, &edges, &frameworks);
    let confidence = overall_confidence(&files, &frameworks);

    let languages = language_acc
        .into_iter()
        .map(|(language, (file_count, bytes))| LanguageStat {
            language,
            files: file_count,
            bytes,
        })
        .collect();

    Ok(RepoReport {
        name: root_path
            .file_name()
            .map(|x| x.to_string_lossy().to_string())
            .unwrap_or_else(|| "Repository".into()),

        root: root_path.to_string_lossy().to_string(),
        url: None,

        readme,

        summary: RepoSummary {
            files: files.len(),
            directories: directories.len(),
            source_files: files
                .iter()
                .filter(|file| file.kind == "source")
                .count(),
            total_bytes,
            largest_file: largest.map(|value| value.1),
            detected_frameworks: frameworks.iter().cloned().collect(),
            confidence,
        },

        languages,
        files,
        modules,
        edges,
        architecture,

        directory_tree,
        entry_points,
        config_files,
        build_files,
        test_files,
        external_deps,

        warnings: vec![
            "Static analysis infers intent from structure; it does not prove runtime behavior."
                .into(),
            "Generated and dependency directories are skipped to keep the scan focused and fast."
                .into(),
        ],
    })
}

fn ignored_dir(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | "node_modules"
            | "target"
            | "dist"
            | "build"
            | "out"
            | "coverage"
            | ".next"
            | ".nuxt"
            | ".svelte-kit"
            | ".cache"
            | ".turbo"
            | "vendor"
            | "Pods"
            | "DerivedData"
    )
}

fn norm(value: &str) -> String {
    value
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_string()
}

fn is_text_language(language: &str) -> bool {
    !matches!(language, "Other" | "Binary" | "Image" | "PDF")
}

fn detect_language(path: &Path) -> String {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "rs" => "Rust",
        "js" | "mjs" | "cjs" => "JavaScript",
        "ts" | "mts" | "cts" => "TypeScript",
        "tsx" => "TypeScript/JSX",
        "jsx" => "JavaScript/JSX",
        "py" => "Python",
        "go" => "Go",
        "java" => "Java",
        "kt" | "kts" => "Kotlin",
        "c" => "C",
        "cpp" | "cc" | "cxx" => "C++",
        "h" | "hpp" => "C/C++ header",
        "cs" => "C#",
        "php" => "PHP",
        "rb" => "Ruby",
        "swift" => "Swift",
        "dart" => "Dart",
        "lua" => "Lua",
        "sh" | "bash" => "Shell",
        "sql" => "SQL",
        "html" | "htm" => "HTML",
        "css" => "CSS",
        "scss" | "sass" => "SCSS",
        "json" => "JSON",
        "yaml" | "yml" => "YAML",
        "toml" => "TOML",
        "md" | "mdx" => "Markdown",
        "xml" => "XML",
        "svg" => "SVG",
        "lock" => "Lockfile",
        "txt" | "text" => "Text",
        "ini" => "INI",
        "cfg" | "conf" => "Config",
        "properties" => "Properties",
        "env" => "Env",
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "ico" | "bmp" => "Image",
        "pdf" => "PDF",
        "" => filename_language(path),
        _ => {
            let fl = filename_language(path);
            if fl != "Other" {
                fl
            } else {
                "Other"
            }
        }
    }
    .into()
}

fn filename_language(path: &Path) -> &str {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    // handle dotfiles like .gitignore, .env, .env.example
    let lower = name.to_ascii_lowercase();
    if lower == ".gitignore" || lower == ".gitattributes" || lower == ".dockerignore" {
        return "Ignore";
    }
    if lower == ".env" || lower.starts_with(".env.") || lower == ".env.example" {
        return "Env";
    }
    if lower == "license" || lower.starts_with("license.") {
        return "Text";
    }
    match name {
        "Dockerfile" => "Dockerfile",
        "Makefile" => "Makefile",
        "CMakeLists.txt" => "CMake",
        "Cargo.toml" => "TOML",
        "package.json" => "JSON",
        "requirements.txt" => "Python requirements",
        _ => {
            // fallback for files like "tsconfig.json" etc already handled by extension,
            // but also handle ".gitignore" without extension already above
            if lower.ends_with(".env") {
                return "Env";
            }
            "Other"
        }
    }
}

fn classify_kind(path: &str, language: &str) -> String {
    let name = Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let lower_path = path.to_ascii_lowercase();

    if name.contains("readme") || name.contains("license") {
        return "documentation".into();
    }

    // helper: path is inside a directory segment `dir` (handles root and nested)
    let in_dir = |dir: &str| {
        lower_path == dir
            || lower_path.starts_with(&format!("{dir}/"))
            || lower_path.contains(&format!("/{dir}/"))
            || lower_path.ends_with(&format!("/{dir}"))
    };

    if name.contains(".test.")
        || name.contains(".spec.")
        || name.ends_with("_test.go")
        || name.ends_with("_test.py")
        || name.ends_with("_tests.py")
        || name.starts_with("test_")
        || name == "test"
        || name == "tests"
        || in_dir("__tests__")
        || in_dir("__test__")
        || in_dir("tests")
        || in_dir("test")
        || in_dir("spec")
        || in_dir("unit_tests")
        || in_dir("integration_tests")
    {
        return "test".into();
    }

    if name.contains("config")
        || in_dir("config")
        || name == ".env.example"
    {
        return "configuration".into();
    }

    if matches!(
        language,
        "JSON"
            | "YAML"
            | "TOML"
            | "Lockfile"
            | "Markdown"
            | "HTML"
            | "CSS"
            | "SCSS"
            | "SVG"
            | "XML"
            | "Dockerfile"
            | "Makefile"
            | "CMake"
    ) {
        return "supporting".into();
    }

    if language == "Other" || language == "Binary" {
        return "other".into();
    }

    "source".into()
}

fn analyze_file(
    path: &str,
    bytes: u64,
    language: &str,
    source: Option<&str>,
) -> FileReport {
    let kind = classify_kind(path, language);
    let src = source.unwrap_or("");

    let symbols = extract_symbols(src, language);
    let imports = extract_imports(src, language);
    let exports = extract_exports(src, language);
    let constants = extract_constants(src, language);
    let doc = if kind == "source" {
        extract_doc(src)
    } else {
        None
    };
    let public_api = extract_public_api(src, language, &symbols, &exports, &constants);

    let metrics = compute_metrics(src, language);

    // IMPORTANT:
    // Generate warnings while metrics is still borrowed,
    // then move metrics into FileReport afterwards.
    let warnings = file_warnings(
        path,
        language,
        src,
        &metrics,
    );

    let role = infer_role(
        path,
        language,
        src,
        &imports,
        &exports,
    );

    let confidence = role_confidence(
        path,
        language,
        &role,
        &imports,
        src,
    );

    let purpose = build_purpose(
        path,
        language,
        &role,
        &symbols,
        &imports,
        &exports,
        &metrics,
        doc.as_deref(),
    );

    FileReport {
        path: path.to_string(),
        language: language.to_string(),
        bytes,
        lines: source.map(|text| text.lines().count()),
        kind,
        role,
        confidence,
        purpose,
        symbols,
        imports,
        exports,
        constants,
        doc,
        public_api,
        metrics,
        warnings,
    }
}

fn lower(value: &str) -> String {
    value.to_ascii_lowercase()
}

fn any_word(value: &str, words: &[&str]) -> bool {
    let lowered = lower(value);

    words.iter().any(|word| {
        lowered.contains(&lower(word))
    })
}

fn imported(imports: &[ImportRef], words: &[&str]) -> bool {
    imports
        .iter()
        .any(|import| any_word(&import.raw, words))
}

fn infer_role(
    path: &str,
    language: &str,
    source: &str,
    imports: &[ImportRef],
    exports: &[String],
) -> String {
    let lowered_path = lower(path);

    let name = lower(
        Path::new(path)
            .file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or(""),
    );

    if lowered_path.contains("test") {
        return "Testing".into();
    }

    if lowered_path.contains("migration") {
        return "Database migration".into();
    }

    if lowered_path.contains("middleware") || name.contains("middleware") {
        return "Middleware".into();
    }

    if lowered_path.contains("route")
        || lowered_path.contains("controller")
        || lowered_path.contains("handler")
    {
        return "API / request handling".into();
    }

    if lowered_path.contains("auth")
        || any_word(
            source,
            &[
                "login",
                "logout",
                "authenticate",
                "authorization",
                "jwt",
                "session",
            ],
        )
    {
        return "Authentication / authorization".into();
    }

    if lowered_path.contains("database")
        || lowered_path.contains("db/")
        || name == "db"
        || imported(
            imports,
            &[
                "postgres",
                "mysql",
                "sqlite",
                "prisma",
                "drizzle",
                "sqlalchemy",
                "mongodb",
                "mongoose",
            ],
        )
    {
        return "Database / persistence".into();
    }

    if lowered_path.contains("component")
        || lowered_path.contains("ui/")
        || lowered_path.contains("views/")
        || language.contains("JSX")
        || any_word(
            source,
            &[
                "useState",
                "useEffect",
                "<div",
                "<button",
                "<View",
            ],
        )
    {
        return "UI / presentation".into();
    }

    if lowered_path.contains("service") || name.ends_with("service") {
        return "Service / business logic".into();
    }

    if lowered_path.contains("util")
        || lowered_path.contains("helper")
        || name.contains("util")
        || name.contains("helper")
    {
        return "Utility / helper".into();
    }

    if lowered_path.contains("model")
        || lowered_path.contains("schema")
        || lowered_path.contains("entity")
        || name.contains("model")
        || name.contains("schema")
    {
        return "Data model / schema".into();
    }

    if lowered_path.contains("store")
        || lowered_path.contains("state")
        || any_word(
            source,
            &["redux", "zustand", "createStore"],
        )
    {
        return "State management".into();
    }

    if lowered_path.contains("cli")
        || any_word(
            source,
            &["clap", "argparse", "commander", "process.argv"],
        )
    {
        return "CLI / command handling".into();
    }

    if !exports.is_empty() && imports.len() >= 3 {
        return "Shared module / library".into();
    }

    "General application logic".into()
}

fn role_confidence(
    path: &str,
    language: &str,
    role: &str,
    imports: &[ImportRef],
    source: &str,
) -> u8 {
    let lowered_path = lower(path);

    let mut score: u8 = 45;

    if !imports.is_empty() {
        score = score.saturating_add(10);
    }

    if !source.is_empty() {
        score = score.saturating_add(10);
    }

    if [
        "auth",
        "test",
        "route",
        "controller",
        "database",
        "service",
        "component",
    ]
    .iter()
    .any(|value| lowered_path.contains(value))
    {
        score = score.saturating_add(25);
    }

    if role == "General application logic" {
        score = score.min(68);
    }

    if language == "Other" {
        score = 25;
    }

    score.min(95)
}

fn build_purpose(
    path: &str,
    language: &str,
    role: &str,
    symbols: &[Symbol],
    imports: &[ImportRef],
    exports: &[String],
    metrics: &CodeMetrics,
    doc: Option<&str>,
) -> String {
    // Map internal "Other" to user-friendly display
    let lang_display = match language {
        "Other" => "Unknown",
        "Image" => "Image",
        "PDF" => "Document",
        _ => language,
    };
    let file = Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(path);

    if let Some(doc_text) = doc {
        let trimmed = doc_text.trim();
        if trimmed.chars().count() >= 24 {
            let snippet = if trimmed.chars().count() > 160 {
                let mut s: String = trimmed.chars().take(160).collect();
                s.push('…');
                s
            } else {
                trimmed.to_string()
            };
            return format!("{file} — {snippet} [{lang_display}, {role}]");
        }
    }

    let intro = match role {
        "Testing" => format!("{file} — test suite in {lang_display}"),
        "UI / presentation" => format!("{file} — UI/presentation component in {lang_display}"),
        "Database / persistence" => format!("{file} — database/persistence module in {lang_display}"),
        "Authentication / authorization" => {
            format!("{file} — authentication/authorization logic in {lang_display}")
        }
        "API / request handling" => format!("{file} — API/request handling in {lang_display}"),
        "Service / business logic" => format!("{file} — business/service logic in {lang_display}"),
        "Data model / schema" => format!("{file} — data model/schema in {lang_display}"),
        "Utility / helper" => format!("{file} — utility/helper module in {lang_display}"),
        "Middleware" => format!("{file} — middleware in {lang_display}"),
        "State management" => format!("{file} — state management in {lang_display}"),
        "CLI / command handling" => format!("{file} — CLI/command handling in {lang_display}"),
        "Shared module / library" => format!("{file} — shared library module in {lang_display}"),
        "Database migration" => format!("{file} — database migration in {lang_display}"),
        "General application logic" => {
            if !symbols.is_empty() {
                let top: Vec<&str> = symbols.iter().take(3).map(|s| s.name.as_str()).collect();
                let joined = top.join(", ");
                let more = if symbols.len() > 3 {
                    format!(" +{} more", symbols.len() - 3)
                } else {
                    String::new()
                };
                format!("{file} — defines {}{} in {lang_display}", joined, more)
            } else {
                let dir = Path::new(path)
                    .parent()
                    .and_then(|p| p.to_str())
                    .unwrap_or("");
                if !dir.is_empty() && dir != "." {
                    format!("{file} — {lang_display} module in {dir}/")
                } else {
                    format!("{file} — {lang_display} source file")
                }
            }
        }
        _ => format!("{file} — {role} in {lang_display}"),
    };

    let mut result = intro;

    if !symbols.is_empty() && !result.contains("defines") {
        let names = symbols
            .iter()
            .take(3)
            .map(|symbol| format!("{} ({})", symbol.name, symbol.kind))
            .collect::<Vec<_>>()
            .join(", ");
        let more = if symbols.len() > 3 {
            format!(" +{} more", symbols.len() - 3)
        } else {
            String::new()
        };
        result.push_str(&format!(". Defines {}{}", names, more));
    } else if !symbols.is_empty() && role == "General application logic" {
        // already in intro, add kinds for context
        let kinds: String = symbols
            .iter()
            .take(3)
            .map(|s| s.kind.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        if !kinds.is_empty() {
            result.push_str(&format!(" ({})", kinds));
        }
    }

    let external_examples: Vec<String> = imports
        .iter()
        .filter(|i| i.external)
        .take(2)
        .map(|i| dep_name(&i.raw))
        .collect();
    if !external_examples.is_empty() {
        result.push_str(&format!(
            ". Uses {}",
            external_examples.join(", ")
        ));
        if imports.iter().filter(|i| i.external).count() > 2 {
            result.push_str(&format!(
                " +{} more external",
                imports.iter().filter(|i| i.external).count() - 2
            ));
        }
    } else if !imports.is_empty() {
        result.push_str(&format!(". Imports {} local module(s)", imports.len()));
    }

    if !exports.is_empty() {
        let ex: Vec<&str> = exports.iter().take(3).map(|s| s.as_str()).collect();
        result.push_str(&format!(". Exports {}", ex.join(", ")));
        if exports.len() > 3 {
            result.push_str(&format!(" +{} more", exports.len() - 3));
        }
    }

    if metrics.functions > 0
        && !result.to_ascii_lowercase().contains("function")
        && !result.to_ascii_lowercase().contains("defines")
    {
        result.push_str(&format!(". ~{} function(s)", metrics.functions));
    }

    if result.chars().count() > 320 {
        let mut s: String = result.chars().take(320).collect();
        s.push('…');
        return s;
    }

    result.push('.');
    result
}

fn regex(pattern: &str) -> Regex {
    Regex::new(pattern).expect("invalid analyzer regex")
}

fn extract_symbols(
    source: &str,
    language: &str,
) -> Vec<Symbol> {
    let patterns: Vec<&str> = match language {
        "Rust" => vec![
            r"\b(?:pub\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"\b(?:pub\s+)?struct\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"\b(?:pub\s+)?enum\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"\b(?:pub\s+)?trait\s+([A-Za-z_][A-Za-z0-9_]*)",
        ],

        "Python" => vec![
            r"^\s*(?:async\s+)?def\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"^\s*class\s+([A-Za-z_][A-Za-z0-9_]*)",
        ],

        "Go" => vec![
            r"\bfunc\s+(?:\([^)]*\)\s*)?([A-Za-z_][A-Za-z0-9_]*)",
            r"\btype\s+([A-Za-z_][A-Za-z0-9_]*)\s+struct\b",
        ],

        "Java" | "Kotlin" | "C#" => vec![
            r"\bclass\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"\binterface\s+([A-Za-z_][A-Za-z0-9_]*)",
            r"\b(?:public|private|protected|static|override|async|fun|void|int|String|boolean|bool|Task)\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(",
        ],

        _ => vec![
            r"\bfunction\s+([A-Za-z_$][A-Za-z0-9_$]*)",
            r"\bclass\s+([A-Za-z_$][A-Za-z0-9_$]*)",
            r"\b(?:const|let|var)\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*=\s*(?:async\s*)?(?:\([^)]*\)|[A-Za-z_$][A-Za-z0-9_$]*)\s*=>",
        ],
    };

    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut result: Vec<Symbol> = Vec::new();
    let compiled: Vec<Regex> = patterns.iter().map(|p| regex(p)).collect();

    for (line_number, line) in source.lines().enumerate() {
        for re in &compiled {
            if let Some(captures) = re.captures(line) {
                if let Some(name_match) = captures.get(1) {
                    let name = name_match.as_str().to_string();

                    if seen.insert(name.clone()) {
                        let kind = if line.contains("class ") {
                            "class"
                        } else if line.contains("struct") {
                            "struct"
                        } else if line.contains("enum") {
                            "enum"
                        } else if line.contains("trait")
                            || line.contains("interface")
                        {
                            "interface/trait"
                        } else {
                            "function/method"
                        };

                        result.push(Symbol {
                            name,
                            kind: kind.into(),
                            line: Some(line_number + 1),
                        });
                    }
                }
            }
        }
    }

    result
}

fn extract_imports(
    source: &str,
    language: &str,
) -> Vec<ImportRef> {
    let mut result: Vec<ImportRef> = Vec::new();

    // For JS/TS, use regex over whole source to handle multiline and multiple per line
    if matches!(
        language,
        "JavaScript" | "TypeScript" | "JavaScript/JSX" | "TypeScript/JSX"
    ) {
        let re_from = regex(r#"from\s+['"]([^'"]+)['"]"#);
        let re_side = regex(r#"import\s+['"]([^'"]+)['"]"#);
        let re_require = regex(r#"require\s*\(\s*['"]([^'"]+)['"]\s*\)"#);
        for cap in re_from.captures_iter(source) {
            if let Some(m) = cap.get(1) {
                let value = m.as_str().to_string();
                if !value.is_empty() {
                    let external = !value.starts_with('.') && !value.starts_with('/');
                    result.push(ImportRef {
                        raw: value.clone(),
                        target: if external { None } else { Some(value.clone()) },
                        external,
                    });
                }
            }
        }
        for cap in re_side.captures_iter(source) {
            if let Some(m) = cap.get(1) {
                let value = m.as_str().to_string();
                if !value.is_empty() {
                    let external = !value.starts_with('.') && !value.starts_with('/');
                    result.push(ImportRef {
                        raw: value.clone(),
                        target: if external { None } else { Some(value.clone()) },
                        external,
                    });
                }
            }
        }
        for cap in re_require.captures_iter(source) {
            if let Some(m) = cap.get(1) {
                let value = m.as_str().to_string();
                if !value.is_empty() {
                    let external = !value.starts_with('.') && !value.starts_with('/');
                    result.push(ImportRef {
                        raw: value.clone(),
                        target: if external { None } else { Some(value.clone()) },
                        external,
                    });
                }
            }
        }
        result.sort_by(|a, b| a.raw.cmp(&b.raw));
        result.dedup_by(|a, b| a.raw == b.raw);
        return result;
    }

    for line in source.lines() {
        let trimmed = line.trim();

        let raw = match language {
            "Rust" => after(trimmed, "use ")
                .or_else(|| after(trimmed, "mod ")),

            "Python" => after(trimmed, "import ")
                .or_else(|| after(trimmed, "from ")),

            "Go" => quoted(trimmed),

            "Java" | "Kotlin" => after(trimmed, "import "),

            _ => js_import(trimmed),
        };

        if let Some(value) = raw {
            let value = value
                .trim_matches(';')
                .trim_matches(['"', '\''])
                .to_string();

            if value.is_empty() {
                continue;
            }

            let external = !value.starts_with('.')
                && !value.starts_with('/')
                && !value.starts_with("crate::")
                && !value.starts_with("self::")
                && !value.starts_with("super::");

            result.push(ImportRef {
                raw: value.clone(),
                target: if external {
                    None
                } else {
                    Some(value)
                },
                external,
            });
        }
    }

    result.sort_by(|a, b| a.raw.cmp(&b.raw));
    result.dedup_by(|a, b| a.raw == b.raw);

    result
}

fn after(text: &str, prefix: &str) -> Option<String> {
    text.strip_prefix(prefix)
        .map(|value| {
            value
                .split_whitespace()
                .next()
                .unwrap_or(value)
                .to_string()
        })
}

fn quoted(text: &str) -> Option<String> {
    text.split('"').nth(1).map(str::to_string)
}

fn is_keyword_prefix(text: &str, kw: &str) -> Option<String> {
    let rest = text.strip_prefix(kw)?;
    // ensure keyword is not part of longer identifier like `imports:` -> next char must not be alnum/_/$
    if let Some(c) = rest.chars().next() {
        if c.is_alphanumeric() || c == '_' || c == '$' {
            return None;
        }
    }
    Some(rest.to_string())
}

fn js_import(text: &str) -> Option<String> {
    if let Some(value) = is_keyword_prefix(text, "import") {
        if let Some(index) = value.find(" from ") {
            return Some(
                value[index + 6..]
                    .split_whitespace()
                    .next()?
                    .trim_matches(['"', '\''])
                    .into(),
            );
        }
        // side-effect import: import "module" or import './style.css'
        // check if next non-space char is quote
        let trimmed_val = value.trim_start();
        if trimmed_val.starts_with('"') || trimmed_val.starts_with('\'') {
            return trimmed_val
                .split_whitespace()
                .next()
                .map(|v| v.trim_matches(['"', '\'', ';']).into());
        }
        // For multiline `import {` without from on same line, don't treat `{` as module
        // Only return if value contains a quoted string before semicolon
        if value.contains('"') || value.contains('\'') {
            return value
                .split_whitespace()
                .next()
                .map(|v| v.trim_matches(['"', '\'']).into());
        }
        return None;
    }

    if let Some(value) = is_keyword_prefix(text, "export") {
        if let Some(index) = value.find(" from ") {
            return Some(
                value[index + 6..]
                    .split_whitespace()
                    .next()?
                    .trim_matches(['"', '\''])
                    .into(),
            );
        }
    }

    if let Some(index) = text.find("require(") {
        // ensure require is a standalone word
        let before = &text[..index];
        if !before.is_empty() && before.chars().last().map_or(false, |c| c.is_alphanumeric() || c == '_' || c == '$') {
            return None;
        }
        return text[index + 8..]
            .split(')')
            .next()
            .map(|value| {
                value
                    .trim_matches(['"', '\'', ' '])
                    .into()
            });
    }
    // also handle multiline `} from 'module'` continuation line
    if text.trim_start().starts_with('}') && text.contains(" from ") {
        if let Some(index) = text.find(" from ") {
            return Some(
                text[index + 6..]
                    .split_whitespace()
                    .next()?
                    .trim_matches(['"', '\'', ';'])
                    .into(),
            );
        }
    }

    None
}

fn extract_exports(
    source: &str,
    language: &str,
) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();

    for trimmed in source.lines().map(str::trim) {
        if matches!(
            language,
            "JavaScript"
                | "TypeScript"
                | "JavaScript/JSX"
                | "TypeScript/JSX"
        ) && trimmed.starts_with("export ")
        {
            if trimmed.starts_with("export default") {
                result.push("default".into());
            }

            for keyword in [
                "function ",
                "class ",
                "const ",
                "let ",
                "var ",
            ] {
                if let Some(index) = trimmed.find(keyword) {
                    if let Some(name) = trimmed[index + keyword.len()..]
                        .split(|c: char| {
                            !c.is_alphanumeric() && c != '_' && c != '$'
                        })
                        .next()
                    {
                        if !name.is_empty() {
                            result.push(name.into());
                        }
                    }
                }
            }
        } else if language == "Rust"
            && trimmed.starts_with("pub ")
        {
            if let Some(name) = trimmed.split_whitespace().nth(2) {
                let name = name.trim_matches(
                    |c: char| !c.is_alphanumeric() && c != '_',
                );

                if !name.is_empty() {
                    result.push(name.into());
                }
            }
        }
    }

    result.sort();
    result.dedup();

    result
}

fn compute_metrics(
    source: &str,
    language: &str,
) -> CodeMetrics {
    let comments = source
        .lines()
        .filter(|line| {
            let trimmed = line.trim();

            trimmed.starts_with("//")
                || trimmed.starts_with('#')
                || trimmed.starts_with("/*")
                || trimmed.starts_with('*')
                || trimmed.starts_with("<!--")
        })
        .count();

    let todo_count = source
        .lines()
        .filter(|line| {
            let lowered = lower(line);

            lowered.contains("todo")
                || lowered.contains("fixme")
                || lowered.contains("hack")
        })
        .count();

    let max_line_length = source
        .lines()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);

    let symbols = extract_symbols(source, language);

    let functions = symbols
        .iter()
        .filter(|symbol| symbol.kind == "function/method")
        .count();

    let classes = symbols
        .iter()
        .filter(|symbol| {
            symbol.kind == "class"
                || symbol.kind == "struct"
        })
        .count();

    let estimated_complexity =
        source.matches(" if ").count()
            + source.matches(" if(").count()
            + source.matches(" for ").count()
            + source.matches(" while ").count()
            + source.matches(" match ").count()
            + source.matches(" case ").count()
            + source.matches("&&").count()
            + source.matches("||").count();

    CodeMetrics {
        functions,
        classes,
        comments,
        todo_count,
        max_line_length,
        estimated_complexity,
    }
}

fn file_warnings(
    path: &str,
    language: &str,
    source: &str,
    metrics: &CodeMetrics,
) -> Vec<String> {
    let mut warnings: Vec<String> = Vec::new();

    if metrics.max_line_length > 180 {
        warnings.push(format!(
            "Contains very long lines (up to {} chars).",
            metrics.max_line_length
        ));
    }

    if metrics.todo_count > 0 {
        warnings.push(format!(
            "Contains {} TODO/FIXME/HACK marker(s).",
            metrics.todo_count
        ));
    }

    let lowered = lower(source);

    if lowered.contains("api_key =")
        || lowered.contains("password =")
        || lowered.contains("secret =")
    {
        warnings.push(
            "Possible hard-coded secret-like assignment; verify manually."
                .into(),
        );
    }

    if language == "SQL" && lowered.contains("select *") {
        warnings.push(
            "Uses SELECT *; explicit columns may be clearer.".into(),
        );
    }

    if path.contains(".env") && !path.ends_with(".example") {
        warnings.push(
            "Environment file may contain sensitive configuration.".into(),
        );
    }

    warnings
}

fn detect_frameworks(
    source: &str,
    language: &str,
    path: &str,
    frameworks: &mut BTreeSet<String>,
) {
    // Avoid false positives from the analyzer's own detector code when scanning itself
    if path.ends_with("analyzer.rs") {
        return;
    }
    // Skip lockfiles — they contain hashes that cause false positives (e.g., "axum" inside integrity)
    if language == "Lockfile"
        || path.ends_with(".lock")
        || path.contains("package-lock.json")
        || path.contains("yarn.lock")
        || path.contains("pnpm-lock.yaml")
        || path.contains("Cargo.lock")
    {
        return;
    }
    let lowered = lower(source);

    if matches!(
        language,
        "JavaScript"
            | "TypeScript"
            | "JavaScript/JSX"
            | "TypeScript/JSX"
    ) {
        let detections = [
            ("next/", "Next.js"),
            ("from 'react'", "React"),
            ("from \"react\"", "React"),
            ("express", "Express"),
            ("@nestjs/", "NestJS"),
            ("fastify", "Fastify"),
            ("vue", "Vue"),
            ("svelte", "Svelte"),
        ];

        for (needle, name) in detections {
            if lowered.contains(needle) {
                frameworks.insert(name.into());
            }
        }
    }

    if language == "Rust" {
        let detections = [
            ("tauri::", "Tauri"),
            ("actix_web", "Actix Web"),
            ("axum", "Axum"),
            ("rocket", "Rocket"),
        ];

        for (needle, name) in detections {
            if lowered.contains(needle) {
                frameworks.insert(name.into());
            }
        }
    }

    if language == "Python" {
        let detections = [
            ("django", "Django"),
            ("fastapi", "FastAPI"),
            ("flask", "Flask"),
        ];

        for (needle, name) in detections {
            if lowered.contains(needle) {
                frameworks.insert(name.into());
            }
        }
    }

    // Fallback for manifest / config files (package.json, Cargo.toml, requirements, etc.)
    // These files often declare frameworks without import statements.
    if matches!(
        language,
        "JSON" | "TOML" | "YAML" | "Python requirements" | "Other"
    ) || language == "Dockerfile"
        || language == "Makefile"
    {
        let manifest_detections = [
            ("\"react\"", "React"),
            ("'react'", "React"),
            ("\"next\"", "Next.js"),
            ("\"vite\"", "Vite"),
            ("\"express\"", "Express"),
            ("\"vue\"", "Vue"),
            ("\"svelte\"", "Svelte"),
            ("\"@nestjs", "NestJS"),
            ("\"fastify\"", "Fastify"),
            ("tauri", "Tauri"),
            ("actix-web", "Actix Web"),
            ("axum", "Axum"),
            ("rocket", "Rocket"),
            ("django", "Django"),
            ("fastapi", "FastAPI"),
            ("flask", "Flask"),
            ("\"django\"", "Django"),
            ("\"fastapi\"", "FastAPI"),
            ("\"flask\"", "Flask"),
        ];
        for (needle, name) in manifest_detections {
            if lowered.contains(needle) {
                frameworks.insert(name.into());
            }
        }
    }

    // Detect Vite via config/content even when language is TypeScript/JS (vite.config.*)
    if lowered.contains("vite") && (lowered.contains("defineconfig") || lowered.contains("\"vite\"") || lowered.contains("'vite'")) {
        frameworks.insert("Vite".into());
    }
}

fn build_edges(
    files: &[FileReport],
) -> Vec<DependencyEdge> {
    let known: HashMap<String, String> = files
        .iter()
        .map(|file| {
            (
                norm(&file.path),
                file.path.clone(),
            )
        })
        .collect();

    let mut result: Vec<DependencyEdge> = Vec::new();

    for file in files.iter().filter(|file| file.kind == "source") {
        for import in &file.imports {
            if let Some(raw) = &import.target {
                if let Some(target) =
                    resolve_local(&file.path, raw, &known)
                {
                    if target != file.path {
                        result.push(DependencyEdge {
                            from: file.path.clone(),
                            to: target,
                            kind: "imports".into(),
                        });
                    }
                }
            }
        }
    }

    result.sort_by(|a, b| {
        a.from
            .cmp(&b.from)
            .then(a.to.cmp(&b.to))
    });

    result.dedup_by(|a, b| {
        a.from == b.from && a.to == b.to
    });

    result
}

fn normalize_path(path: PathBuf) -> String {
    use std::path::Component;
    let mut parts: Vec<String> = Vec::new();
    for c in path.components() {
        match c {
            Component::ParentDir => {
                parts.pop();
            }
            Component::CurDir => {}
            Component::Normal(p) => parts.push(p.to_string_lossy().to_string()),
            Component::RootDir | Component::Prefix(_) => {}
        }
    }
    parts.join("/")
}

fn resolve_local(
    from: &str,
    raw: &str,
    known: &HashMap<String, String>,
) -> Option<String> {
    let base = Path::new(from)
        .parent()
        .unwrap_or(Path::new(""));

    let candidate = normalize_path(base.join(raw));

    let variants = [
        candidate.clone(),
        format!("{candidate}.ts"),
        format!("{candidate}.tsx"),
        format!("{candidate}.js"),
        format!("{candidate}.jsx"),
        format!("{candidate}.rs"),
        format!("{candidate}/index.ts"),
        format!("{candidate}/index.tsx"),
        format!("{candidate}/index.js"),
        format!("{candidate}/mod.rs"),
    ];

    variants
        .into_iter()
        .find_map(|value| known.get(&value).cloned())
}

fn build_modules(
    files: &[FileReport],
    edges: &[DependencyEdge],
) -> Vec<ModuleReport> {
    let mut incoming: HashMap<&str, usize> = HashMap::new();
    let mut outgoing: HashMap<&str, usize> = HashMap::new();

    for edge in edges {
        *outgoing
            .entry(edge.from.as_str())
            .or_default() += 1;

        *incoming
            .entry(edge.to.as_str())
            .or_default() += 1;
    }

    files
        .iter()
        .filter(|file| file.kind == "source")
        .map(|file| {
            let incoming_count =
                *incoming.get(file.path.as_str()).unwrap_or(&0);

            let outgoing_count =
                *outgoing.get(file.path.as_str()).unwrap_or(&0);

            ModuleReport {
                path: file.path.clone(),
                incoming: incoming_count,
                outgoing: outgoing_count,
                role: file.role.clone(),
                centrality: ((incoming_count + outgoing_count) as f32).sqrt(),
            }
        })
        .collect()
}

fn infer_architecture(
    files: &[FileReport],
    edges: &[DependencyEdge],
    frameworks: &BTreeSet<String>,
) -> Vec<ArchitectureInsight> {
    let mut result: Vec<ArchitectureInsight> = Vec::new();

    if !frameworks.is_empty() {
        result.push(ArchitectureInsight {
            title: "Detected technology stack".into(),
            detail: format!(
                "The repository appears to use {}.",
                frameworks
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            severity: "info".into(),
        });
    }

    let hubs = files
        .iter()
        .filter(|file| {
            edges
                .iter()
                .filter(|edge| {
                    edge.from == file.path
                        || edge.to == file.path
                })
                .count()
                >= 8
        })
        .count();

    if hubs > 0 {
        result.push(ArchitectureInsight {
            title: "Central modules".into(),
            detail: format!(
                "{hubs} source file(s) have unusually high local connectivity and may act as shared hubs."
            ),
            severity: "info".into(),
        });
    }

    let large_files = files
        .iter()
        .filter(|file| file.lines.unwrap_or(0) > 500)
        .count();

    if large_files > 0 {
        result.push(ArchitectureInsight {
            title: "Large source files".into(),
            detail: format!(
                "{large_files} source file(s) exceed 500 lines; they may combine multiple responsibilities."
            ),
            severity: "warning".into(),
        });
    }

    let todo_count = files
        .iter()
        .map(|file| file.metrics.todo_count)
        .sum::<usize>();

    if todo_count > 0 {
        result.push(ArchitectureInsight {
            title: "Unresolved work markers".into(),
            detail: format!(
                "Found {todo_count} TODO/FIXME/HACK marker(s)."
            ),
            severity: "warning".into(),
        });
    }

    if edges.is_empty() && files.len() > 3 {
        result.push(ArchitectureInsight {
            title: "Dependency graph is incomplete".into(),
            detail:
                "The scanner found few resolvable local imports. Language-specific AST parsing and package-aware resolution are planned for later passes."
                    .into(),
            severity: "warning".into(),
        });
    }

    result
}

fn overall_confidence(
    files: &[FileReport],
    frameworks: &BTreeSet<String>,
) -> u8 {
    if files.is_empty() {
        return 0;
    }

    let average =
        files
            .iter()
            .map(|file| file.confidence as u32)
            .sum::<u32>()
            / files.len() as u32;

    let framework_bonus = if frameworks.is_empty() {
        0
    } else {
        8
    };

    (average as u8 + framework_bonus).min(96)
}

const README_MAX_BYTES: u64 = 200_000;

fn detect_readme(root: &Path) -> Option<ReadmeInfo> {
    let candidates = [
        "README.md",
        "readme.md",
        "README",
        "readme",
        "README.txt",
        "README.rst",
        "README.markdown",
        "Readme.md",
        "Readme",
    ];

    for name in candidates {
        let path = root.join(name);

        if !path.is_file() {
            continue;
        }

        let bytes = fs::metadata(&path).ok().map(|meta| meta.len()).unwrap_or(0);
        let truncated = bytes > README_MAX_BYTES;

        let content = fs::read(&path)
            .ok()
            .map(|data| {
                let data = if data.len() > README_MAX_BYTES as usize {
                    &data[..README_MAX_BYTES as usize]
                } else {
                    &data
                };
                String::from_utf8_lossy(data).into_owned()
            })
            .unwrap_or_default();

        if content.is_empty() && bytes > 0 {
            continue;
        }

        return Some(ReadmeInfo {
            path: name.to_string(),
            content,
            bytes,
            truncated,
        });
    }

    None
}

fn build_directory_tree(
    directories: &BTreeSet<String>,
    files: &[FileReport],
) -> Vec<TreeNode> {
    let mut nodes: Vec<TreeNode> = Vec::new();

    for dir in directories {
        let depth = dir.matches('/').count() + 1;

        if depth <= 4 {
            nodes.push(TreeNode {
                path: dir.clone(),
                is_dir: true,
                depth,
            });
        }
    }

    for file in files {
        let depth = file.path.matches('/').count() + 1;

        if depth <= 3 {
            nodes.push(TreeNode {
                path: file.path.clone(),
                is_dir: false,
                depth,
            });
        }
    }

    nodes.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then(b.is_dir.cmp(&a.is_dir))
    });
    nodes.truncate(300);

    nodes
}

fn detect_entry_points(files: &[FileReport]) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();

    for file in files {
        if file.kind != "source" {
            continue;
        }

        let name = Path::new(&file.path)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let depth = file.path.matches('/').count();
        let shallow = depth <= 2;

        let has_main = file
            .symbols
            .iter()
            .any(|symbol| {
                symbol.name == "main" && symbol.kind == "function/method"
            });

        let has_main_py_guard = (file.language == "Python"
            && file
                .imports
                .iter()
                .any(|imp| imp.raw.contains("__main__")))
            || lower(&file.path).contains("__main__")
            || file
                .symbols
                .iter()
                .any(|s| s.name == "__main__");

        let entry = matches!(
            name.as_str(),
            "main.rs"
                | "lib.rs"
                | "mod.rs"
                | "main.py"
                | "__main__.py"
                | "manage.py"
                | "app.py"
                | "server.py"
                | "wsgi.py"
                | "asgi.py"
                | "main.go"
                | "main.c"
                | "main.cpp"
                | "program.cs"
                | "cli.py"
                | "cli.js"
                | "cli.ts"
                | "app.js"
                | "app.ts"
                | "server.js"
                | "server.ts"
        ) || (shallow
            && matches!(
                name.as_str(),
                "main.js"
                    | "main.ts"
                    | "main.tsx"
                    | "main.jsx"
                    | "index.js"
                    | "index.ts"
                    | "index.tsx"
                    | "index.jsx"
                    | "app.js"
                    | "app.ts"
                    | "app.tsx"
                    | "app.jsx"
                    | "server.js"
                    | "server.ts"
                    | "server.tsx"
                    | "_app.js"
                    | "_app.ts"
                    | "_app.tsx"
            ))
            || lower(&file.path).contains("pages/index.")
            || lower(&file.path).contains("app/page.")
            || lower(&file.path).contains("src/index.")
            || lower(&file.path).contains("src/main.")
            || lower(&file.path).contains("src/app.")
            || lower(&file.path).contains("src/server.")
            || file.path.ends_with("cmd/main.go")
            || has_main_py_guard
            || (has_main
                && matches!(
                    file.language.as_str(),
                    "Rust" | "Go" | "C" | "C++" | "Java" | "Kotlin" | "C#" | "Python" | "JavaScript" | "TypeScript" | "TypeScript/JSX" | "JavaScript/JSX"
                ));

        if entry {
            result.push(file.path.clone());
        }
    }

    result.sort();
    result.dedup();
    result.truncate(12);

    result
}

fn collect_by_kind(files: &[FileReport], kind: &str) -> Vec<String> {
    let mut result: Vec<String> = files
        .iter()
        .filter(|file| file.kind == kind)
        .map(|file| file.path.clone())
        .collect();

    result.sort();
    result.dedup();
    result.truncate(80);

    result
}

fn detect_build_files(files: &[FileReport]) -> Vec<String> {
    let mut result: Vec<String> = files
        .iter()
        .filter(|file| {
            let name = Path::new(&file.path)
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();

            matches!(
                name.as_str(),
                "dockerfile"
                    | "makefile"
                    | "cmakelists.txt"
                    | "cargo.toml"
                    | "cargo.lock"
                    | "build.gradle"
                    | "build.gradle.kts"
                    | "pom.xml"
                    | "meson.build"
                    | "justfile"
                    | "build.zig"
                    | "build.rs"
                    | "pyproject.toml"
                    | "setup.py"
                    | "setup.cfg"
                    | "requirements.txt"
                    | "pipfile"
                    | "pipfile.lock"
                    | "poetry.lock"
                    | "pdm.lock"
                    | "go.mod"
                    | "go.sum"
                    | "package.json"
                    | "package-lock.json"
                    | "yarn.lock"
                    | "pnpm-lock.yaml"
                    | "bun.lockb"
                    | "tsconfig.json"
                    | "jsconfig.json"
                    | "turbo.json"
                    | "nx.json"
                    | "lerna.json"
                    | ".babelrc"
                    | "babel.config.json"
                    | ".eslintrc.json"
                    | ".eslintrc.js"
                    | "jest.config.js"
                    | "jest.config.ts"
            ) || name.starts_with("dockerfile.")
                || name.starts_with("docker-compose")
                || name.contains("vite.config")
                || name.contains("webpack.config")
                || name.contains("rollup.config")
                || name.contains("next.config")
                || name.contains("jest.config")
                || name.contains("vitest.config")
                || name.contains("tsconfig")
                || name.contains("eslint.config")
                || name.contains("babel.config")
                || name.contains("esbuild")
                || name.contains("tsup.config")
        })
        .map(|file| file.path.clone())
        .collect();

    result.sort();
    result.dedup();
    result.truncate(60);

    result
}

fn aggregate_external_deps(files: &[FileReport]) -> Vec<ExternalDep> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();

    for file in files {
        for import in &file.imports {
            if import.external {
                *counts.entry(dep_name(&import.raw)).or_default() += 1;
            }
        }
    }

    let mut result: Vec<ExternalDep> = counts
        .into_iter()
        .map(|(name, count)| ExternalDep { name, count })
        .collect();

    result.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then(a.name.cmp(&b.name))
    });
    result.truncate(40);

    result
}

fn dep_name(raw: &str) -> String {
    let trimmed = raw.trim_matches(['"', '\'']).trim();

    if trimmed.starts_with('@') {
        return trimmed
            .split('/')
            .take(2)
            .collect::<Vec<_>>()
            .join("/");
    }

    trimmed
        .split([':', '/', ' '])
        .next()
        .unwrap_or(trimmed)
        .to_string()
}

fn extract_constants(source: &str, language: &str) -> Vec<String> {
    let patterns: Vec<&str> = match language {
        "Rust" => vec![
            r"\bconst\s+([A-Za-z_][A-Za-z0-9_]*)\s*[:=]",
            r"\bstatic\s+([A-Za-z_][A-Za-z0-9_]*)\s*[:=]",
        ],
        "Python" => vec![r"^([A-Z][A-Z0-9_]*)\s*="],
        "Go" => vec![r"\bconst\s+([A-Za-z_][A-Za-z0-9_]*)\s*="],
        "Java" | "Kotlin" | "C#" => vec![
            r"\b(?:static\s+)?final\s+[A-Za-z0-9_<>\[\],\s]+([A-Z][A-Z0-9_]*)\s*=",
        ],
        "C" | "C++" => vec![
            r"#define\s+([A-Z][A-Z0-9_]*)",
            r"\bconst\s+[A-Za-z0-9_*]+\s+([A-Z][A-Z0-9_]*)\s*=",
        ],
        "JavaScript"
        | "TypeScript"
        | "JavaScript/JSX"
        | "TypeScript/JSX" => vec![r"\bconst\s+([A-Z][A-Z0-9_]*)\s*="],
        _ => vec![],
    };

    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut result: Vec<String> = Vec::new();
    let compiled: Vec<Regex> = patterns.iter().map(|p| regex(p)).collect();

    for line in source.lines() {
        let trimmed = line.trim();

        for re in &compiled {
            if let Some(captures) = re.captures(trimmed) {
                if let Some(name_match) = captures.get(1) {
                    let name = name_match.as_str().to_string();

                    if (language.starts_with("JavaScript")
                        || language.starts_with("TypeScript"))
                        && trimmed.contains("=>")
                    {
                        continue;
                    }

                    if seen.insert(name.clone()) {
                        result.push(name);
                    }
                }
            }
        }
    }

    result.truncate(20);

    result
}

fn extract_doc(source: &str) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut started = false;
    let mut python_docstring: Option<&str> = None;

    for line in source.lines() {
        let trimmed = line.trim();

        if trimmed.is_empty() {
            if started {
                break;
            }
            continue;
        }

        if let Some(marker) = python_docstring {
            let cleaned = trimmed
                .trim_end_matches(marker)
                .trim()
                .to_string();

            if !cleaned.is_empty() {
                parts.push(cleaned);
            }

            if trimmed.ends_with(marker) {
                python_docstring = None;
            }

            continue;
        }

        let is_comment = trimmed.starts_with("//")
            || trimmed.starts_with('#')
            || trimmed.starts_with("/*")
            || trimmed.starts_with('*')
            || trimmed.starts_with("<!--");

        let is_python_docstring =
            trimmed.starts_with("\"\"\"") || trimmed.starts_with("'''");

        if !is_comment && !is_python_docstring {
            break;
        }

        started = true;

        if is_python_docstring {
            let marker = if trimmed.starts_with("\"\"\"") {
                "\"\"\""
            } else {
                "'''"
            };

            let cleaned = trimmed
                .trim_start_matches(marker)
                .trim_end_matches(marker)
                .trim()
                .to_string();

            if !cleaned.is_empty() {
                parts.push(cleaned);
            }

            if !trimmed.ends_with(marker) {
                python_docstring = Some(marker);
            }

            continue;
        }

        if trimmed.starts_with("#!") {
            continue;
        }

        let cleaned = trimmed
            .trim_start_matches("///")
            .trim_start_matches("//!")
            .trim_start_matches("//")
            .trim_start_matches("/*")
            .trim_start_matches("*/")
            .trim_start_matches('*')
            .trim_start_matches("<!--")
            .trim_start_matches("-->")
            .trim()
            .to_string();

        if !cleaned.is_empty() {
            parts.push(cleaned);
        }

        if parts.len() >= 8 {
            break;
        }
    }

    if parts.len() < 2 {
        return None;
    }

    let mut text = parts.join(" ");

    if text.chars().count() > 400 {
        text = text.chars().take(400).collect();
    }

    Some(text)
}

fn extract_public_api(
    _source: &str,
    language: &str,
    symbols: &[Symbol],
    exports: &[String],
    constants: &[String],
) -> Vec<String> {
    let mut result: Vec<String> = Vec::new();

    match language {
        "Rust" => {
            result.extend(exports.iter().cloned());
            result.extend(constants.iter().cloned());
        }
        "Python" => {
            result.extend(
                symbols
                    .iter()
                    .filter(|symbol| !symbol.name.starts_with('_'))
                    .map(|symbol| symbol.name.clone()),
            );
        }
        "Go" => {
            result.extend(
                symbols
                    .iter()
                    .filter(|symbol| {
                        symbol
                            .name
                            .chars()
                            .next()
                            .map(|c| c.is_ascii_uppercase())
                            .unwrap_or(false)
                    })
                    .map(|symbol| symbol.name.clone()),
            );
        }
        _ => {
            result.extend(exports.iter().cloned());
        }
    }

    result.sort();
    result.dedup();
    result.truncate(25);

    result
}


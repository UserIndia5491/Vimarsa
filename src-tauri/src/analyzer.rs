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
    pub summary: RepoSummary,
    pub languages: Vec<LanguageStat>,
    pub files: Vec<FileReport>,
    pub modules: Vec<ModuleReport>,
    pub edges: Vec<DependencyEdge>,
    pub architecture: Vec<ArchitectureInsight>,
    pub warnings: Vec<String>,
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
            detect_frameworks(source_text, &language, &mut frameworks);
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
    !matches!(language, "Other" | "Binary")
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
        "" => filename_language(path),
        _ => "Other",
    }
    .into()
}

fn filename_language(path: &Path) -> &str {
    match path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("")
    {
        "Dockerfile" => "Dockerfile",
        "Makefile" => "Makefile",
        "CMakeLists.txt" => "CMake",
        "Cargo.toml" => "TOML",
        "package.json" => "JSON",
        "requirements.txt" => "Python requirements",
        _ => "Other",
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

    if name.ends_with(".test.ts")
        || name.ends_with(".test.tsx")
        || name.ends_with(".spec.ts")
        || name.ends_with("_test.go")
        || name.starts_with("test_")
        || lower_path.contains("/tests/")
    {
        return "test".into();
    }

    if name.contains("config")
        || lower_path.contains("/config/")
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
) -> String {
    let file = Path::new(path)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(path);

    let noun = if role.contains("Testing") {
        "tests"
    } else if role.contains("UI") {
        "user-interface code"
    } else if role.contains("Database") {
        "database/persistence code"
    } else if role.contains("Authentication") {
        "authentication logic"
    } else if role.contains("API") {
        "request/API handling"
    } else if role.contains("Service") {
        "business/service logic"
    } else if role.contains("model") {
        "data modeling"
    } else {
        "application logic"
    };

    let mut result = format!(
        "{file} appears to contain {noun}. It is written in {language}."
    );

    if !symbols.is_empty() {
        let names = symbols
            .iter()
            .take(4)
            .map(|symbol| symbol.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        result.push_str(&format!(
            " It defines {} symbol(s), including {}.",
            symbols.len(),
            names
        ));
    }

    if !imports.is_empty() {
        result.push_str(&format!(
            " It imports {} module/package reference(s).",
            imports.len()
        ));
    }

    if !exports.is_empty() {
        result.push_str(&format!(
            " It exposes {} export(s).",
            exports.len()
        ));
    }

    if metrics.functions > 0 {
        result.push_str(&format!(
            " The code contains about {} function/method definition(s).",
            metrics.functions
        ));
    }

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

    for (line_number, line) in source.lines().enumerate() {
        for pattern in &patterns {
            if let Some(captures) = regex(pattern).captures(line) {
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

fn js_import(text: &str) -> Option<String> {
    if let Some(value) = text.strip_prefix("import") {
        if let Some(index) = value.find(" from ") {
            return Some(
                value[index + 6..]
                    .split_whitespace()
                    .next()?
                    .trim_matches(['"', '\''])
                    .into(),
            );
        }

        return value
            .split_whitespace()
            .next()
            .map(|value| value.trim_matches(['"', '\'']).into());
    }

    if let Some(value) = text.strip_prefix("export") {
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
        return text[index + 8..]
            .split(')')
            .next()
            .map(|value| {
                value
                    .trim_matches(['"', '\'', ' '])
                    .into()
            });
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
    frameworks: &mut BTreeSet<String>,
) {
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

fn resolve_local(
    from: &str,
    raw: &str,
    known: &HashMap<String, String>,
) -> Option<String> {
    let base = Path::new(from)
        .parent()
        .unwrap_or(Path::new(""));

    let candidate =
        norm(&base.join(raw).to_string_lossy());

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

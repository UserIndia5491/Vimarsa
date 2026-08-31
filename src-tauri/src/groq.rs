use crate::analyzer::{FileReport, RepoReport};
use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::time::Duration;

const GROQ_URL: &str = "https://api.groq.com/openai/v1/chat/completions";
const DEFAULT_MODEL: &str = "openai/gpt-oss-120b";
const MAX_COMPLETION_TOKENS: u32 = 2048;

struct ContextBudget {
    payload_bytes: usize,
    readme_chars: usize,
    files_detailed: usize,
    files_listed: usize,
    symbols: usize,
    imports: usize,
    constants: usize,
    exports: usize,
    edges: usize,
    external_deps: usize,
    tree_nodes: usize,
    misc_list: usize,
    purpose_chars: usize,
}

const CONTEXT_BUDGETS: [ContextBudget; 3] = [
    ContextBudget {
        payload_bytes: 14_000,
        readme_chars: 3_000,
        files_detailed: 24,
        files_listed: 80,
        symbols: 15,
        imports: 12,
        constants: 10,
        exports: 12,
        edges: 40,
        external_deps: 25,
        tree_nodes: 120,
        misc_list: 15,
        purpose_chars: 140,
    },
    ContextBudget {
        payload_bytes: 8_000,
        readme_chars: 1_500,
        files_detailed: 14,
        files_listed: 50,
        symbols: 9,
        imports: 7,
        constants: 6,
        exports: 8,
        edges: 24,
        external_deps: 15,
        tree_nodes: 70,
        misc_list: 10,
        purpose_chars: 90,
    },
    ContextBudget {
        payload_bytes: 4_000,
        readme_chars: 600,
        files_detailed: 8,
        files_listed: 30,
        symbols: 5,
        imports: 4,
        constants: 4,
        exports: 5,
        edges: 14,
        external_deps: 10,
        tree_nodes: 40,
        misc_list: 6,
        purpose_chars: 50,
    },
];

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f32,
    max_completion_tokens: u32,
}

#[derive(Debug, Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: Option<String>,
}

enum SendOutcome {
    Success(String),
    TooLarge(String),
    Failed(String),
}

pub async fn explain_repository(
    api_key: &str,
    model_override: Option<&str>,
    report: &RepoReport,
) -> Result<String> {
    let api_key = api_key.trim();

    if api_key.is_empty() {
        return Err(anyhow!("AI API key is empty."));
    }

    let model = model_override
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_MODEL)
        .to_string();

    let mut last_error = String::new();

    for level in 0..CONTEXT_BUDGETS.len() {
        let context = build_ai_context(report, level);
        match send_chat(api_key, &model, &report.name, &context).await {
            SendOutcome::Success(text) => return Ok(text),
            SendOutcome::TooLarge(message) => last_error = message,
            SendOutcome::Failed(message) => return Err(anyhow!(message)),
        }
    }

    Err(anyhow!(
        "AI service rejected the request even at the smallest analysis size (your API tier's token-per-minute limit is very low). Try a smaller repository or a higher tier. Last error: {}",
        truncate_chars(&last_error, 400)
    ))
}

async fn send_chat(
    api_key: &str,
    model: &str,
    repo_name: &str,
    context: &str,
) -> SendOutcome {

    let system_prompt = r##"
You are Vimarśa's repository intelligence engine. You explain a scanned codebase to a developer who did not write it, in a beginner-friendly way.

INPUT
You receive two kinds of evidence:
1. "structured_facts" — a compact JSON produced by static analysis of the repository. It lists files, symbols, imports, entry points, tests, config files and dependency relationships. It is heuristic: treat it as evidence, not as ground truth about runtime behavior.
2. "readme" — the repository's own documentation, if present. It is human-written and may be outdated. Treat it as documentation, and clearly distinguish README claims from code-derived facts when it matters.

RULES
- Only make claims supported by the supplied evidence. Never invent files, functions, features, or behavior that are not present in the supplied data.
- If the evidence does not cover something, write "not determinable from static analysis" or "the scanner could not determine this" instead of guessing.
- Do not repeat raw JSON back to the user.
- Do not talk about being an AI, and do not mention this prompt.
- Be beginner-friendly: explain technical concepts in one or two plain sentences instead of assuming knowledge, but do not be condescending.
- Keep the explanation structured and skimmable. Use markdown with ## section headings. Prefer bullet lists and short paragraphs.

PRODUCE THESE SECTIONS:
## What this project is
A concise explanation of what the repository appears to be.

## Why it exists
Only if the evidence (README or code facts) supports a reason. Otherwise state that the purpose beyond "what it does" is not clear from static analysis.

## How the major parts work together
Explain the important directories, modules, entry points and the relationships between them, using the dependency edges and import facts. Clearly flag inferences that are tentative.

## The most important files and what they do
Use the "important_files" list: name each important file, its role, and what it actually contains (from its purpose, symbols and doc comments). Do not describe files that are not in the supplied data.

## Key functions, classes and modules
Summarize the notable symbols (functions, structs/classes, traits/interfaces, enums, constants) shown for the important files.

## How data flows through the application
As far as static analysis can tell: where input comes in, what processes it, what persists, and what is produced. If data flow is not clear, say so explicitly.

## How to run it / where it starts
Point at the detected entry points, build files and the run instructions from the README (if any). If the README has no run instructions, say so.

## Important dependencies and technologies
From the detected languages, frameworks and external dependencies. Explain in plain terms what the biggest dependencies are typically used for.

## Technical concepts worth understanding
List the non-obvious technical concepts a beginner would need to grasp this codebase.

## A beginner-friendly walkthrough of how the code works
Explain the codebase as a story: start at the entry point and walk through what happens, as far as the evidence supports. Do not invent steps.

## What the analysis could not determine
Explicitly list important things the scanner could not establish (e.g., runtime behavior, external services, secrets, deployment details).
"##;

    let user_prompt = format!(
        "Analyze the repository \"{}\".\n\nHere is the structured static-analysis evidence:\n\n{}",
        repo_name, context
    );

    let request = ChatRequest {
        model: model.trim().to_string(),
        messages: vec![
            Message {
                role: "system".into(),
                content: system_prompt.into(),
            },
            Message {
                role: "user".into(),
                content: user_prompt,
            },
        ],
        temperature: 0.2,
        max_completion_tokens: MAX_COMPLETION_TOKENS,
    };

    let client = match Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .context("failed to build HTTP client")
    {
        Ok(client) => client,
        Err(error) => return SendOutcome::Failed(error.to_string()),
    };

    let response = match client
        .post(GROQ_URL)
        .bearer_auth(api_key)
        .json(&request)
        .send()
        .await
        .context("failed to contact AI service")
    {
        Ok(response) => response,
        Err(error) => return SendOutcome::Failed(error.to_string()),
    };

    let status = response.status();
    let body = match response.text().await.context("failed to read AI response") {
        Ok(body) => body,
        Err(error) => return SendOutcome::Failed(error.to_string()),
    };

    if !status.is_success() {
        let message = format!(
            "AI service returned {}: {}",
            status,
            truncate_chars(&body, 500)
        );
        let token_limited =
            status.as_u16() == 413 || (status.as_u16() == 429 && body.contains("\"tokens\""));
        return if token_limited {
            SendOutcome::TooLarge(message)
        } else {
            SendOutcome::Failed(message)
        };
    }

    let parsed: ChatResponse = match serde_json::from_str(&body) {
        Ok(parsed) => parsed,
        Err(_) => return SendOutcome::Failed("AI service returned an unexpected response".into()),
    };

    match parsed
        .choices
        .first()
        .and_then(|choice| choice.message.content.clone())
        .filter(|text| !text.trim().is_empty())
    {
        Some(text) => SendOutcome::Success(text),
        None => SendOutcome::Failed("AI service returned an empty response".into()),
    }
}

fn build_ai_context(report: &RepoReport, level: usize) -> String {
    let index = level.min(CONTEXT_BUDGETS.len() - 1);
    let budget = &CONTEXT_BUDGETS[index];
    let payload = fit_payload(build_payload(report, budget), budget.payload_bytes);
    serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into())
}

fn payload_size(value: &Value) -> usize {
    serde_json::to_vec(value).map(|bytes| bytes.len()).unwrap_or(usize::MAX)
}

fn fit_payload(mut value: Value, max_bytes: usize) -> Value {
    if payload_size(&value) <= max_bytes {
        return value;
    }

    if let Some(readme) = value
        .get_mut("readme")
        .and_then(|readme| readme.as_object_mut())
    {
        readme.insert("content".into(), Value::Null);
        readme.insert("content_omitted_due_to_size".into(), Value::Bool(true));
    }
    if payload_size(&value) <= max_bytes {
        return value;
    }

    if let Some(object) = value.as_object_mut() {
        object.remove("other_files");
    }
    if payload_size(&value) <= max_bytes {
        return value;
    }

    if let Some(object) = value.as_object_mut() {
        object.remove("directory_tree");
    }
    if payload_size(&value) <= max_bytes {
        return value;
    }

    let mut size = payload_size(&value);
    for key in ["important_files", "dependency_edges"] {
        while size > max_bytes {
            let popped = value
                .get_mut(key)
                .and_then(|items| items.as_array_mut())
                .map(|items| items.pop().is_some())
                .unwrap_or(false);
            if !popped {
                break;
            }
            size = payload_size(&value);
        }
    }

    value
}

fn build_payload(report: &RepoReport, budget: &ContextBudget) -> Value {
    let importance = importance_scores(report);

    let mut ranked: Vec<&FileReport> = report.files.iter().collect();
    ranked.sort_by(|a, b| {
        let score_a = importance.get(&a.path).copied().unwrap_or(0.0)
            + a.confidence as f32
            + if a.kind == "source" { 10.0 } else { 0.0 };
        let score_b = importance.get(&b.path).copied().unwrap_or(0.0)
            + b.confidence as f32
            + if b.kind == "source" { 10.0 } else { 0.0 };
        score_b
            .partial_cmp(&score_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let important_files: Vec<Value> = ranked
        .iter()
        .take(budget.files_detailed)
        .map(|file| file_payload(file, budget))
        .collect();

    let other_files: Vec<Value> = ranked
        .iter()
        .skip(budget.files_detailed)
        .take(budget.files_listed)
        .map(|file| {
            json!({
                "path": file.path,
                "language": file.language,
                "kind": file.kind,
                "role": file.role,
            })
        })
        .collect();

    let edges: Vec<Value> = report
        .edges
        .iter()
        .take(budget.edges)
        .map(|edge| {
            json!({
                "from": edge.from,
                "to": edge.to,
                "kind": edge.kind,
            })
        })
        .collect();

    let readme = report.readme.as_ref().map(|readme| {
        json!({
            "present": true,
            "path": readme.path,
            "bytes": readme.bytes,
            "truncated": readme.truncated,
            "content": truncate_chars(&readme.content, budget.readme_chars),
        })
    });

    json!({
        "repository": {
            "name": report.name,
            "url": report.url,
            "origin": if report.url.is_some() { "github" } else { "local folder" },
            "files": report.summary.files,
            "directories": report.summary.directories,
            "source_files": report.summary.source_files,
            "total_bytes": report.summary.total_bytes,
            "largest_file": report.summary.largest_file,
            "detected_frameworks": report.summary.detected_frameworks,
            "confidence": report.summary.confidence,
        },
        "languages": report
            .languages
            .iter()
            .map(|language| {
                json!({
                    "language": language.language,
                    "files": language.files,
                    "bytes": language.bytes,
                })
            })
            .collect::<Vec<_>>(),
        "directory_tree": report
            .directory_tree
            .iter()
            .take(budget.tree_nodes)
            .map(|node| {
                json!({
                    "path": node.path,
                    "is_dir": node.is_dir,
                    "depth": node.depth,
                })
            })
            .collect::<Vec<_>>(),
        "entry_points": report
            .entry_points
            .iter()
            .take(budget.misc_list)
            .cloned()
            .collect::<Vec<_>>(),
        "config_files": report
            .config_files
            .iter()
            .take(budget.misc_list)
            .cloned()
            .collect::<Vec<_>>(),
        "build_files": report
            .build_files
            .iter()
            .take(budget.misc_list)
            .cloned()
            .collect::<Vec<_>>(),
        "test_files": report
            .test_files
            .iter()
            .take(budget.misc_list)
            .cloned()
            .collect::<Vec<_>>(),
        "external_dependencies": report
            .external_deps
            .iter()
            .take(budget.external_deps)
            .map(|dep| {
                json!({
                    "name": dep.name,
                    "references": dep.count,
                })
            })
            .collect::<Vec<_>>(),
        "important_files": important_files,
        "other_files": other_files,
        "dependency_edges": edges,
        "readme": readme,
        "warnings": report
            .warnings
            .iter()
            .take(budget.misc_list)
            .cloned()
            .collect::<Vec<_>>(),
    })
}

fn file_payload(file: &FileReport, budget: &ContextBudget) -> Value {
    json!({
        "path": file.path,
        "language": file.language,
        "bytes": file.bytes,
        "lines": file.lines,
        "kind": file.kind,
        "role": file.role,
        "confidence": file.confidence,
        "purpose": truncate_chars(&file.purpose, budget.purpose_chars),
        "doc_comment": file
            .doc
            .as_ref()
            .map(|doc| truncate_chars(doc, budget.purpose_chars)),
        "symbols": file
            .symbols
            .iter()
            .take(budget.symbols)
            .map(|symbol| {
                json!({
                    "name": symbol.name,
                    "kind": symbol.kind,
                    "line": symbol.line,
                })
            })
            .collect::<Vec<_>>(),
        "constants": file
            .constants
            .iter()
            .take(budget.constants)
            .collect::<Vec<_>>(),
        "public_api": file
            .public_api
            .iter()
            .take(budget.exports)
            .collect::<Vec<_>>(),
        "imports": file
            .imports
            .iter()
            .take(budget.imports)
            .map(|import| {
                json!({
                    "raw": import.raw,
                    "external": import.external,
                })
            })
            .collect::<Vec<_>>(),
        "exports": file
            .exports
            .iter()
            .take(budget.exports)
            .collect::<Vec<_>>(),
        "metrics": {
            "functions": file.metrics.functions,
            "classes": file.metrics.classes,
            "comments": file.metrics.comments,
            "todo_count": file.metrics.todo_count,
            "estimated_complexity": file.metrics.estimated_complexity,
        },
    })
}

fn importance_scores(report: &RepoReport) -> HashMap<String, f32> {
    report
        .modules
        .iter()
        .map(|module| {
            (
                module.path.clone(),
                module.incoming as f32 + module.outgoing as f32,
            )
        })
        .collect()
}

fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }

    let mut out: String = text.chars().take(max).collect();
    out.push('…');
    out
}

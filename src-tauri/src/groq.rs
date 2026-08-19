use crate::analyzer::RepoReport;
use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

const GROQ_URL: &str = "https://api.groq.com/openai/v1/chat/completions";
const MODEL: &str = "openai/gpt-oss-120b";

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

pub async fn explain_repository(
    api_key: &str,
    report: &RepoReport,
    readme: Option<&str>,
) -> Result<String> {
    let api_key = api_key.trim();

    if api_key.is_empty() {
        return Err(anyhow!("Groq API key is empty."));
    }

    let facts = build_facts(report, readme);

    let system_prompt = r#"
You are Vimarśa's repository intelligence engine.

Your job is to turn static repository facts into a clear explanation for a developer who did not write the codebase.

IMPORTANT:
- Only make claims supported by the supplied facts and README.
- Do not invent runtime behavior.
- Do not pretend static analysis proves something it cannot prove.
- Prefer concrete explanations over generic software terminology.
- Explain relationships between parts of the repository.
- If something is uncertain, explicitly say so.
- Do not repeat raw JSON.
- Do not talk about being an AI.
- Do not mention this prompt.

Produce these sections:

## What this project is
A concise explanation of what the repository appears to be.

## How the codebase is organized
Explain the important directories, modules and entry points.

## How the pieces connect
Explain important dependency relationships and data/control flow that can be inferred.

## Main technologies
Explain the languages, frameworks and important dependencies detected.

## Important files
Mention the most important files and what each appears to be responsible for.

## Things worth knowing
Mention useful architectural observations, uncertainty, warnings or possible areas of interest.

Keep the explanation readable and beginner-friendly without becoming simplistic.
"#;

    let user_prompt = format!(
        "Here is the factual static-analysis data for the repository:\n\n{}\n\nREADME:\n\n{}",
        facts,
        readme.unwrap_or("(No README file was found.)")
    );

    let request = ChatRequest {
        model: MODEL.to_string(),
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
        max_completion_tokens: 5000,
    };

    let client = Client::new();

    let response = client
        .post(GROQ_URL)
        .bearer_auth(api_key)
        .json(&request)
        .send()
        .await
        .context("failed to contact Groq")?;

    let status = response.status();
    let body = response
        .text()
        .await
        .context("failed to read Groq response")?;

    if !status.is_success() {
        return Err(anyhow!("Groq API returned {}: {}", status, body));
    }

    let parsed: ChatResponse =
        serde_json::from_str(&body).context("Groq returned an unexpected response")?;

    parsed
        .choices
        .first()
        .and_then(|choice| choice.message.content.clone())
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| anyhow!("Groq returned an empty response"))
}

fn build_facts(report: &RepoReport, readme: Option<&str>) -> String {
    let files: Vec<_> = report
        .files
        .iter()
        .map(|file| {
            json!({
                "path": file.path,
                "language": file.language,
                "bytes": file.bytes,
                "lines": file.lines,
                "kind": file.kind,
                "role": file.role,
                "confidence": file.confidence,
                "symbols": file.symbols.iter().map(|s| json!({
                    "name": s.name,
                    "kind": s.kind,
                    "line": s.line
                })).collect::<Vec<_>>(),
                "imports": file.imports.iter().map(|i| json!({
                    "target": i.target,
                    "external": i.external
                })).collect::<Vec<_>>(),
                "exports": file.exports,
                "metrics": {
                    "functions": file.metrics.functions,
                    "classes": file.metrics.classes,
                    "comments": file.metrics.comments,
                    "todo_count": file.metrics.todo_count,
                    "estimated_complexity": file.metrics.estimated_complexity
                }
            })
        })
        .collect();

    let modules: Vec<_> = report
        .modules
        .iter()
        .map(|module| {
            json!({
                "path": module.path,
                "incoming": module.incoming,
                "outgoing": module.outgoing,
                "role": module.role,
                "centrality": module.centrality
            })
        })
        .collect();

    let edges: Vec<_> = report
        .edges
        .iter()
        .map(|edge| {
            json!({
                "from": edge.from,
                "to": edge.to,
                "kind": edge.kind
            })
        })
        .collect();

    let data = json!({
        "repository": {
            "name": report.name,
            "files": report.summary.files,
            "directories": report.summary.directories,
            "source_files": report.summary.source_files,
            "total_bytes": report.summary.total_bytes,
            "largest_file": report.summary.largest_file,
            "detected_frameworks": report.summary.detected_frameworks
        },
        "languages": report.languages,
        "files": files,
        "modules": modules,
        "dependency_edges": edges,
        "readme_present": readme.is_some()
    });

    serde_json::to_string(&data).unwrap_or_else(|_| "{}".into())
}
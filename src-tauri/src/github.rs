use anyhow::{anyhow, bail, Context, Result};
use git2::{build::RepoBuilder, FetchOptions, RemoteCallbacks};
use std::{
    fs,
    time::{Duration, SystemTime},
};
use tempfile::{Builder as TempBuilder, TempDir};
use url::Url;

#[derive(Debug, Clone)]
pub struct GitHubRepo {
    pub owner: String,
    pub name: String,
    pub clone_url: String,
}

pub fn parse_github_url(raw: &str) -> Result<GitHubRepo> {
    let trimmed = raw.trim();

    if trimmed.is_empty() {
        bail!("Please enter a GitHub repository URL.");
    }

    let with_scheme = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("https://{trimmed}")
    };

    let url = Url::parse(&with_scheme)
        .map_err(|_| anyhow!("That does not look like a valid URL: {trimmed}"))?;

    let host = url.host_str().unwrap_or("").to_ascii_lowercase();

    if host != "github.com" && host != "www.github.com" {
        bail!(
            "Only GitHub repository URLs are supported (https://github.com/owner/repo). \
             Got host: {host}."
        );
    }

    if url.scheme() != "https" && url.scheme() != "http" {
        bail!(
            "Unsupported URL scheme '{}'. Use https://github.com/owner/repo.",
            url.scheme()
        );
    }

    let mut segments: Vec<String> = url
        .path_segments()
        .map(|parts| {
            parts
                .filter(|part| !part.is_empty())
                .map(|part| part.to_string())
                .collect()
        })
        .unwrap_or_default();

    if let Some(last) = segments.last_mut() {
        if let Some(stripped) = last.strip_suffix(".git") {
            *last = stripped.to_string();
        }
    }

    if segments.len() != 2 {
        bail!(
            "Unsupported GitHub URL. Expected https://github.com/owner/repo \
             (this URL has {} path segment(s); branch and file URLs are not supported).",
            segments.len()
        );
    }

    let owner = segments[0].clone();
    let name = segments[1].clone();

    if !owner
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        bail!("Invalid GitHub repository URL — owner or repository name contains invalid characters.");
    }

    Ok(GitHubRepo {
        clone_url: format!("https://github.com/{owner}/{name}.git"),
        owner,
        name,
    })
}

pub fn clone_repository(repo: &GitHubRepo) -> Result<TempDir> {
    let workspace = TempBuilder::new()
        .prefix("vimarsa-")
        .tempdir()
        .context("could not create a temporary workspace for cloning")?;

    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|_url, _username, _allowed| {
        Err(git2::Error::from_str(
            "this repository requires authentication; Vimarśa only supports public repositories",
        ))
    });

    let mut fetch = FetchOptions::new();
    fetch
        .remote_callbacks(callbacks)
        .depth(1)
        .download_tags(git2::AutotagOption::None);

    let mut builder = RepoBuilder::new();
    builder.fetch_options(fetch);

    match builder.clone(&repo.clone_url, workspace.path()) {
        Ok(_) => Ok(workspace),
        Err(error) => Err(anyhow!(friendly_clone_error(&error))),
    }
}

fn friendly_clone_error(error: &git2::Error) -> String {
    let message = error.message().to_ascii_lowercase();

    if error.class() == git2::ErrorClass::Net || message.contains("network") {
        return "Could not reach GitHub. Check your network connection and try again.".into();
    }

    if message.contains("authentication") || message.contains("requires auth") {
        return "This repository is private or requires authentication. \
                 Vimarśa only supports public repositories."
            .into();
    }

    if message.contains("not found") || message.contains("404") {
        return "Repository not found. It may be private, deleted, or the owner/name in \
                 the URL may be wrong."
            .into();
    }

    if message.contains("nonexistent ref")
        || message.contains("no refs")
        || message.contains("unable to checkout")
        || message.contains("empty")
    {
        return "The repository appears to be empty or has no default branch.".into();
    }

    format!("Cloning failed: {}", error.message())
}

pub fn cleanup_stale_workspaces() {
    let Ok(entries) = fs::read_dir(std::env::temp_dir()) else {
        return;
    };

    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(24 * 60 * 60))
        .unwrap_or(SystemTime::now());

    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };

        if !name.starts_with("vimarsa-") {
            continue;
        }

        let Ok(metadata) = entry.metadata() else {
            continue;
        };

        if !metadata.is_dir() {
            continue;
        }

        let stale = metadata
            .modified()
            .map(|modified| modified < cutoff)
            .unwrap_or(false);

        if stale {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_normal_repository_url() {
        let repo = parse_github_url("https://github.com/octocat/Hello-World").unwrap();
        assert_eq!(repo.owner, "octocat");
        assert_eq!(repo.name, "Hello-World");
        assert_eq!(repo.clone_url, "https://github.com/octocat/Hello-World.git");
    }

    #[test]
    fn strips_git_suffix() {
        let repo = parse_github_url("https://github.com/octocat/Hello-World.git").unwrap();
        assert_eq!(repo.name, "Hello-World");
    }

    #[test]
    fn strips_trailing_slash() {
        let repo = parse_github_url("https://github.com/octocat/Hello-World/").unwrap();
        assert_eq!(repo.name, "Hello-World");
    }

    #[test]
    fn accepts_url_without_scheme() {
        let repo = parse_github_url("github.com/octocat/Hello-World").unwrap();
        assert_eq!(repo.owner, "octocat");
    }

    #[test]
    fn accepts_www_host() {
        let repo = parse_github_url("https://www.github.com/octocat/Hello-World").unwrap();
        assert_eq!(repo.name, "Hello-World");
    }

    #[test]
    fn rejects_non_github_host() {
        assert!(parse_github_url("https://gitlab.com/octocat/Hello-World").is_err());
    }

    #[test]
    fn rejects_deep_paths() {
        assert!(parse_github_url("https://github.com/octocat/Hello-World/tree/main").is_err());
    }

    #[test]
    fn rejects_too_few_segments() {
        assert!(parse_github_url("https://github.com/octocat").is_err());
    }

    #[test]
    fn rejects_ssh_urls() {
        assert!(parse_github_url("git@github.com:octocat/Hello-World.git").is_err());
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_github_url("not a url at all").is_err());
    }

    #[test]
    fn rejects_empty() {
        assert!(parse_github_url("   ").is_err());
    }
}

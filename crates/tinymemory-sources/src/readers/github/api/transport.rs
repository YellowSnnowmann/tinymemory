//! Production GitHub CLI and REST transport.

use super::super::GH_CLI_TIMEOUT;

/// Run `gh <args>` and return stdout as UTF-8.
async fn gh_json(args: &[&str]) -> Result<String, String> {
    let output = tokio::time::timeout(
        GH_CLI_TIMEOUT,
        tokio::process::Command::new("gh").args(args).output(),
    )
    .await
    .map_err(|_| format!("gh command timed out after {}s", GH_CLI_TIMEOUT.as_secs()))?
    .map_err(|e| format!("gh command failed: {e}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gh exited {}: {stderr}", output.status));
    }

    String::from_utf8(output.stdout).map_err(|e| format!("gh output not utf8: {e}"))
}

/// Unauthenticated GET against the GitHub REST API.
async fn api_get(path: &str) -> Result<String, String> {
    let url = format!("https://api.github.com{path}");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("failed to build GitHub client: {e}"))?;
    let resp = client
        .get(&url)
        .header("User-Agent", "openhuman")
        .header("Accept", "application/vnd.github.v3+json")
        .send()
        .await
        .map_err(|e| format!("GitHub API request failed: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("GitHub API returned {status}: {body}"));
    }

    resp.text()
        .await
        .map_err(|e| format!("failed to read response: {e}"))
}

/// Try `gh api` first, fall back to unauthenticated REST API.
pub(crate) async fn fetch_github(api_path: &str, use_gh: bool) -> Result<String, String> {
    if use_gh {
        match gh_json(&["api", api_path]).await {
            Ok(s) => return Ok(s),
            Err(e) => {
                tracing::debug!(
                    error = %e,
                    path = %api_path,
                    "[memory_sources:github] gh failed, falling back to API"
                );
            }
        }
    }
    api_get(&format!("/{api_path}")).await
}

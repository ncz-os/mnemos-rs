use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};

pub async fn search(
    base: &str,
    token: Option<&str>,
    query: &str,
    limit: u32,
    raw: bool,
) -> Result<()> {
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("failed to build HTTP client")?;
    let base = base.trim_end_matches('/');

    let response = match post_search(
        &client,
        &format!("{base}/v1/memories/search"),
        token,
        query,
        limit,
    )
    .await
    {
        Ok(response) => response,
        Err(SearchError::Unauthorized) => {
            eprintln!("Unauthorized: provide --token or set MNEMOS_API_KEY");
            std::process::exit(3);
        }
        Err(SearchError::Http(message)) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
        Err(SearchError::Other(err)) => return Err(err),
    };

    if raw {
        println!("{}", serde_json::to_string(&response)?);
    } else {
        print_summary(&response, query);
    }

    Ok(())
}

async fn post_search(
    client: &Client,
    url: &str,
    token: Option<&str>,
    query: &str,
    limit: u32,
) -> Result<Value, SearchError> {
    let mut request = client.post(url).json(&json!({
        "query": query,
        "limit": limit,
    }));
    if let Some(token) = token.filter(|token| !token.is_empty()) {
        request = request.bearer_auth(token);
    }

    let response = request
        .send()
        .await
        .map_err(|err| SearchError::Http(format!("Search request failed: {err}")))?;
    let status = response.status();
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        return Err(SearchError::Unauthorized);
    }
    if !status.is_success() {
        return Err(SearchError::Http(http_status_error(status)));
    }

    response
        .json::<Value>()
        .await
        .context("invalid JSON response")
        .map_err(SearchError::Other)
}

fn print_summary(response: &Value, query: &str) {
    let memories = response
        .get("memories")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);

    if memories.is_empty() {
        println!("No memories match \"{query}\".");
        return;
    }

    let count = response
        .get("count")
        .and_then(Value::as_u64)
        .unwrap_or(memories.len() as u64);

    match response.get("latency_ms").and_then(Value::as_u64) {
        Some(latency_ms) => {
            println!("{count} memories matching \"{query}\" (took {latency_ms}ms):")
        }
        None => println!("{count} memories matching \"{query}\":"),
    }

    for memory in memories {
        let id = text_field(memory, &["id", "memory_id", "uuid"]).unwrap_or("unknown");
        let category = text_field(memory, &["category", "type"]).unwrap_or("unknown");
        let created_at = text_field(memory, &["created_at", "timestamp", "created", "updated_at"])
            .unwrap_or("");
        let content = text_field(memory, &["content", "text", "summary"]).unwrap_or("");

        println!(
            "  [{:<12}] {:<10} {} — {}",
            truncate(id, 12),
            truncate(category, 10),
            format_timestamp(created_at),
            truncate(content, 60),
        );
    }
}

fn text_field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| value.get(*key)?.as_str())
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        truncated
    } else {
        value.to_string()
    }
}

fn format_timestamp(value: &str) -> String {
    if value.len() >= 16 {
        let prefix = &value[..16];
        if prefix.as_bytes().get(10) == Some(&b'T') {
            return format!("{} {}", &prefix[..10], &prefix[11..16]);
        }
        return prefix.to_string();
    }

    value.to_string()
}

fn http_status_error(status: StatusCode) -> String {
    match status.canonical_reason() {
        Some(reason) => format!("HTTP {status} {reason}"),
        None => format!("HTTP {status}"),
    }
}

enum SearchError {
    Unauthorized,
    Http(String),
    Other(anyhow::Error),
}

use std::io::{self, Read};
use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

#[derive(Deserialize, Serialize)]
struct MemoryResponse {
    id: String,
    content: String,
    category: String,
    created: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    permission_mode: Option<u64>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

pub async fn create(
    base: &str,
    token: Option<&str>,
    content: Option<&str>,
    category: &str,
    subcategory: Option<&str>,
    permission_mode: Option<u16>,
    raw: bool,
) -> Result<()> {
    let content = resolve_content(content)?;

    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .context("failed to build HTTP client")?;
    let base = base.trim_end_matches('/');

    let response = match post_create(
        &client,
        &format!("{base}/v1/memories"),
        token,
        &content,
        category,
        subcategory,
        permission_mode,
    )
    .await
    {
        Ok(response) => response,
        Err(CreateError::Unauthorized) => {
            eprintln!("Unauthorized: provide --token or set MNEMOS_API_KEY");
            std::process::exit(3);
        }
        Err(CreateError::Http(message)) => {
            eprintln!("{message}");
            std::process::exit(2);
        }
        Err(CreateError::Other(err)) => return Err(err),
    };

    if raw {
        println!("{}", serde_json::to_string(&response)?);
    } else {
        print_summary(&response);
    }

    Ok(())
}

fn resolve_content(flag: Option<&str>) -> Result<String> {
    if let Some(text) = flag {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            eprintln!("error: --content was empty");
            std::process::exit(2);
        }
        return Ok(text.to_string());
    }

    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .context("failed to read content from stdin")?;
    let trimmed = buf.trim();
    if trimmed.is_empty() {
        eprintln!("error: empty content (provide --content or pipe text on stdin)");
        std::process::exit(2);
    }
    Ok(trimmed.to_string())
}

async fn post_create(
    client: &Client,
    url: &str,
    token: Option<&str>,
    content: &str,
    category: &str,
    subcategory: Option<&str>,
    permission_mode: Option<u16>,
) -> Result<MemoryResponse, CreateError> {
    let mut body = Map::new();
    body.insert("content".into(), json!(content));
    body.insert("category".into(), json!(category));
    if let Some(sub) = subcategory {
        body.insert("subcategory".into(), json!(sub));
    }
    if let Some(mode) = permission_mode {
        body.insert("permission_mode".into(), json!(mode));
    }

    let mut request = client.post(url).json(&Value::Object(body));
    if let Some(token) = token.filter(|token| !token.is_empty()) {
        request = request.bearer_auth(token);
    }

    let response = request
        .send()
        .await
        .map_err(|err| CreateError::Http(format!("Create request failed: {err}")))?;
    let status = response.status();
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        return Err(CreateError::Unauthorized);
    }
    if !status.is_success() {
        // Surface server-supplied detail when the body is JSON like
        // {"detail": "permission_mode must be octal-style 0-777"}
        let detail = response
            .text()
            .await
            .ok()
            .and_then(|text| {
                serde_json::from_str::<Value>(&text)
                    .ok()
                    .and_then(|v| v.get("detail").and_then(Value::as_str).map(String::from))
                    .or(Some(text).filter(|t| !t.is_empty()))
            })
            .unwrap_or_default();
        return Err(CreateError::Http(http_status_error(status, &detail)));
    }

    response
        .json::<MemoryResponse>()
        .await
        .context("invalid create response")
        .map_err(CreateError::Other)
}

fn print_summary(response: &MemoryResponse) {
    // permission_mode is stored as octal-style decimal digits (e.g. the
    // integer 644 means "rw-r--r--"). Render the value as-is rather than
    // converting bases — the server validator gates each digit to 0-7,
    // so the printed integer already reads as a Unix mode.
    let mode = response
        .permission_mode
        .map(|n| n.to_string())
        .unwrap_or_else(|| "?".to_string());
    println!(
        "Created {} ({}, mode={mode})",
        response.id, response.category
    );
}

fn http_status_error(status: StatusCode, detail: &str) -> String {
    let reason = status.canonical_reason().unwrap_or("");
    if detail.is_empty() {
        if reason.is_empty() {
            format!("HTTP {status}")
        } else {
            format!("HTTP {status} {reason}")
        }
    } else if reason.is_empty() {
        format!("HTTP {status}: {detail}")
    } else {
        format!("HTTP {status} {reason}: {detail}")
    }
}

enum CreateError {
    Unauthorized,
    Http(String),
    Other(anyhow::Error),
}

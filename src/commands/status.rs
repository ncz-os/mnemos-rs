use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};

pub async fn status(base: &str, token: Option<&str>, raw: bool) -> Result<()> {
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("failed to build HTTP client")?;
    let base = base.trim_end_matches('/');

    let health = match get_json(&client, &format!("{base}/health"), None).await {
        Ok(health) => health,
        Err(err) => {
            eprintln!("MNEMOS UNREACHABLE: {err}");
            std::process::exit(2);
        }
    };

    let stats = get_json(&client, &format!("{base}/stats"), token)
        .await
        .context("failed to fetch MNEMOS stats")?;

    if raw {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "health": health,
                "stats": stats,
            }))?
        );
    } else {
        print_summary(&health, &stats);
    }

    Ok(())
}

async fn get_json(client: &Client, url: &str, token: Option<&str>) -> Result<Value> {
    let mut request = client.get(url);
    if let Some(token) = token.filter(|token| !token.is_empty()) {
        request = request.bearer_auth(token);
    }

    let response = request.send().await.context("request failed")?;
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!("{}", http_status_error(status));
    }

    response
        .json::<Value>()
        .await
        .context("invalid JSON response")
}

fn print_summary(health: &Value, stats: &Value) {
    let name = text_field(health, &["name", "node", "peer", "hostname"])
        .or_else(|| {
            let status = text_field(health, &["status"])?;
            (!is_ok_status(status)).then_some(status)
        })
        .unwrap_or("server");
    let version = text_field(health, &["version"]).unwrap_or("unknown");
    let profile = text_field(health, &["profile"]).unwrap_or("unknown");
    let db = match bool_field(health, &["database_connected"]) {
        Some(true) => "connected",
        Some(false) => "disconnected",
        None => "unknown",
    };
    let health_status = text_field(health, &["status"]).unwrap_or("unknown");

    println!("MNEMOS {name} (v{version}, {profile} profile)");
    println!("  DB:        {db}");

    if let Some(nats) = nats_summary(health, stats) {
        println!("  NATS:      {nats}");
    }
    println!("  Memories:  {}", memories_summary(stats));
    println!("  Compressions: {}", compressions_summary(stats));

    println!("  Health:    {}", health_status.to_ascii_uppercase());
}

fn memories_summary(stats: &Value) -> String {
    let total = number_field(stats, &["total_memories"]).unwrap_or(0);
    let native = number_field(stats, &["native_memories"]).unwrap_or(0);
    let federated = number_field(stats, &["federated_memories"]).unwrap_or(0);

    format!("{total} total ({native} native, {federated} federated)")
}

fn compressions_summary(stats: &Value) -> String {
    let done = number_field(stats, &["total_compressions", "compressions_done"]).unwrap_or(0);
    let unreviewed = number_field(stats, &["unreviewed_compressions"]).unwrap_or(0);

    format!("{done} done, {unreviewed} unreviewed")
}

fn nats_summary(health: &Value, stats: &Value) -> Option<String> {
    let enabled = bool_field(health, &["nats_publishing_enabled"])?;
    let status = if enabled { "enabled" } else { "disabled" };

    match number_field(stats, &["nats_streams", "streams"]) {
        Some(streams) => Some(format!("{status} ({streams} streams)")),
        None => Some(status.to_string()),
    }
}

fn text_field<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| value.get(*key)?.as_str())
}

fn bool_field(value: &Value, keys: &[&str]) -> Option<bool> {
    keys.iter().find_map(|key| value.get(*key)?.as_bool())
}

fn number_field(value: &Value, keys: &[&str]) -> Option<u64> {
    keys.iter().find_map(|key| value.get(*key)?.as_u64())
}

fn is_ok_status(status: &str) -> bool {
    matches!(
        status.to_ascii_lowercase().as_str(),
        "ok" | "healthy" | "up" | "ready"
    )
}

fn http_status_error(status: StatusCode) -> String {
    match status.canonical_reason() {
        Some(reason) => format!("HTTP {status} {reason}"),
        None => format!("HTTP {status}"),
    }
}

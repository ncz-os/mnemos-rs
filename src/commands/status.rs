use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

#[derive(Deserialize, Serialize)]
struct HealthResponse {
    status: String,
    timestamp: String,
    database_connected: bool,
    version: String,
    #[serde(default = "default_profile")]
    profile: String,
    #[serde(default)]
    distillation_worker: Option<String>,
    #[serde(default)]
    nats_publishing_enabled: bool,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

#[derive(Deserialize, Serialize)]
struct StatsResponse {
    total_memories: u64,
    #[serde(default)]
    native_memories: u64,
    #[serde(default)]
    federated_memories: u64,
    total_compressions: u64,
    average_compression_ratio: f64,
    average_quality_rating: u64,
    memories_by_category: Map<String, Value>,
    #[serde(default)]
    memories_by_subcategory: Map<String, Value>,
    unreviewed_compressions: u64,
    timestamp: String,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

fn default_profile() -> String {
    "edge".to_string()
}

pub async fn status(base: &str, token: Option<&str>, raw: bool) -> Result<()> {
    let client = Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .context("failed to build HTTP client")?;
    let base = base.trim_end_matches('/');

    let (health, health_service_unavailable) =
        match get_health(&client, &format!("{base}/health")).await {
            Ok(health) => health,
            Err(err) => {
                eprintln!("MNEMOS UNREACHABLE: {err}");
                std::process::exit(2);
            }
        };

    let health_degraded = health_service_unavailable || status_unhealthy(&health);
    let stats = match get_json::<StatsResponse>(&client, &format!("{base}/stats"), token).await {
        Ok(stats) => Some(stats),
        Err(err) if health_degraded => {
            eprintln!("MNEMOS statistics unavailable: {err}");
            None
        }
        Err(err) => return Err(err).context("failed to fetch MNEMOS stats"),
    };

    if raw {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "health": health,
                "stats": stats,
            }))?
        );
    } else {
        print_summary(&health, stats.as_ref(), health_degraded);
    }

    if health_degraded {
        anyhow::bail!("MNEMOS health is degraded");
    }

    Ok(())
}

async fn get_health(client: &Client, url: &str) -> Result<(HealthResponse, bool)> {
    let response = client.get(url).send().await.context("request failed")?;
    let status = response.status();
    if !status.is_success() && status != StatusCode::SERVICE_UNAVAILABLE {
        anyhow::bail!("{}", http_status_error(status));
    }

    let service_unavailable = status == StatusCode::SERVICE_UNAVAILABLE;
    let health = response
        .json::<HealthResponse>()
        .await
        .context("invalid JSON response")?;
    Ok((health, service_unavailable))
}

async fn get_json<T>(client: &Client, url: &str, token: Option<&str>) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    let mut request = client.get(url);
    if let Some(token) = token.filter(|token| !token.is_empty()) {
        request = request.bearer_auth(token);
    }

    let response = request.send().await.context("request failed")?;
    let status = response.status();
    if !status.is_success() {
        anyhow::bail!("{}", http_status_error(status));
    }

    response.json::<T>().await.context("invalid JSON response")
}

fn print_summary(health: &HealthResponse, stats: Option<&StatsResponse>, health_degraded: bool) {
    let name = text_field(&health.extra, &["name", "node", "peer", "hostname"])
        .or_else(|| (!is_ok_status(&health.status)).then_some(health.status.as_str()))
        .unwrap_or("server");
    let db = if health.database_connected {
        "connected"
    } else {
        "disconnected"
    };
    let health_status = if health_degraded {
        "degraded"
    } else {
        &health.status
    };

    println!(
        "MNEMOS {name} (v{}, {} profile)",
        health.version, health.profile
    );
    println!("  DB:        {db}");

    if let Some(nats) = nats_summary(health, stats) {
        println!("  NATS:      {nats}");
    }
    if let Some(worker) = health.distillation_worker.as_deref() {
        println!("  Distillation worker: {worker}");
    }
    if let Some(worker) = text_field(&health.extra, &["deletion_request_worker"]) {
        println!("  Deletion request worker: {worker}");
    }
    if let Some(worker) = text_field(&health.extra, &["hard_deletion_request_worker"]) {
        println!("  Hard deletion request worker: {worker}");
    }
    if let Some(worker) = text_field(&health.extra, &["persephone_archival_worker"]) {
        println!("  Persephone archival worker: {worker}");
    }
    match stats {
        Some(stats) => {
            println!("  Memories:  {}", memories_summary(stats));
            println!("  Compressions: {}", compressions_summary(stats));
        }
        None => {
            println!("  Memories:  unavailable");
            println!("  Compressions: unavailable");
        }
    }

    println!("  Health:    {}", health_status.to_ascii_uppercase());
}

fn memories_summary(stats: &StatsResponse) -> String {
    format!(
        "{} total ({} native, {} federated)",
        stats.total_memories, stats.native_memories, stats.federated_memories
    )
}

fn compressions_summary(stats: &StatsResponse) -> String {
    format!(
        "{} done, {} unreviewed",
        stats.total_compressions, stats.unreviewed_compressions
    )
}

fn nats_summary(health: &HealthResponse, stats: Option<&StatsResponse>) -> Option<String> {
    let status = if health.nats_publishing_enabled {
        "enabled"
    } else {
        "disabled"
    };

    match stats.and_then(|stats| number_field(&stats.extra, &["nats_streams", "streams"])) {
        Some(streams) => Some(format!("{status} ({streams} streams)")),
        None => Some(status.to_string()),
    }
}

fn required_worker_unhealthy(health: &HealthResponse) -> bool {
    let distillation_unhealthy = matches!(
        health.distillation_worker.as_deref(),
        Some("starting" | "error" | "unavailable")
    );
    let critical_worker_unhealthy = [
        text_field(&health.extra, &["deletion_request_worker"]),
        text_field(&health.extra, &["hard_deletion_request_worker"]),
        text_field(&health.extra, &["persephone_archival_worker"]),
    ]
    .into_iter()
    .any(|status| matches!(status, Some("starting" | "error")));
    distillation_unhealthy || critical_worker_unhealthy
}

fn status_unhealthy(health: &HealthResponse) -> bool {
    !is_ok_status(&health.status) || required_worker_unhealthy(health)
}

fn text_field<'a>(value: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| value.get(*key)?.as_str())
}

fn number_field(value: &Map<String, Value>, keys: &[&str]) -> Option<u64> {
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

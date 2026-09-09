use std::time::Duration;

use anyhow::{Context, Result};
use reqwest::{Client, Response, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::PeerAction;

#[derive(Debug, Deserialize)]
struct FederationPeer {
    id: String,
    name: String,
    base_url: String,
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct FederationPeerList {
    peers: Vec<FederationPeer>,
}

#[derive(Debug, Serialize)]
struct CreatePeer<'a> {
    name: &'a str,
    base_url: &'a str,
    auth_token: &'a str,
    enabled: bool,
    sync_interval_secs: u64,
    compat_mode: &'static str,
}

pub async fn peer(
    base: &str,
    server_token: Option<&str>,
    configured_peer_token: Option<String>,
    action: PeerAction,
) -> Result<()> {
    let client = Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("failed to build HTTP client")?;
    let endpoint = format!("{}/v1/federation/peers", base.trim_end_matches('/'));

    match action {
        PeerAction::Add {
            url,
            name,
            peer_token_arg: _,
            clear_token,
            preserve_token,
        } => {
            add(
                &client,
                &endpoint,
                server_token,
                &url,
                name,
                configured_peer_token,
                clear_token,
                preserve_token,
            )
            .await
        }
        PeerAction::List => list(&client, &endpoint, server_token).await,
    }
}

#[allow(clippy::too_many_arguments)]
async fn add(
    client: &Client,
    endpoint: &str,
    server_token: Option<&str>,
    url: &str,
    name: Option<String>,
    peer_token: Option<String>,
    clear_token: bool,
    preserve_token: bool,
) -> Result<()> {
    if url.trim().is_empty() {
        anyhow::bail!("peer URL cannot be empty");
    }
    let resolved_name = match name {
        Some(name) if !name.trim().is_empty() => name,
        _ => default_name_from_url(url)?,
    };
    let peer_token = peer_token.and_then(|token| (!token.trim().is_empty()).then_some(token));

    let current = fetch_peers(client, endpoint, server_token).await?;
    if let Some(existing) = current.peers.iter().find(|peer| peer.name == resolved_name) {
        let url_changed = existing.base_url != url;
        if url_changed && peer_token.is_none() && !clear_token && !preserve_token {
            anyhow::bail!(
                "changing the URL for peer '{resolved_name}' (was {}, now {url}) requires \
                 explicit token handling; set MNEMOS_PEER_TOKEN, use --credentials-file, or pass \
                 --clear-token or --preserve-token",
                existing.base_url
            );
        }

        let mut body = Map::new();
        body.insert("base_url".into(), Value::String(url.to_string()));
        if let Some(token) = peer_token.as_deref() {
            body.insert("auth_token".into(), Value::String(token.to_string()));
        } else if clear_token {
            // Core represents an anonymous peer with an empty auth token.
            body.insert("auth_token".into(), Value::String(String::new()));
        }
        let response = authorized(
            client.patch(format!("{endpoint}/{}", existing.id)),
            server_token,
        )
        .json(&body)
        .send()
        .await
        .context("peer update request failed")?;
        ensure_success(response).await?;
        if url_changed {
            println!("Updated peer '{resolved_name}' -> {url}");
        } else if clear_token {
            println!("Cleared token for peer '{resolved_name}'");
        } else if peer_token.is_some() {
            println!("Updated peer '{resolved_name}' (token refreshed)");
        } else {
            println!("Peer '{resolved_name}' already at {url}");
        }
        return Ok(());
    }

    let body = CreatePeer {
        name: &resolved_name,
        base_url: url,
        auth_token: peer_token.as_deref().unwrap_or(""),
        enabled: true,
        sync_interval_secs: 300,
        compat_mode: "strict",
    };
    let response = authorized(client.post(endpoint), server_token)
        .json(&body)
        .send()
        .await
        .context("peer registration request failed")?;
    ensure_success(response).await?;
    println!("Added peer '{resolved_name}' -> {url}");
    Ok(())
}

async fn list(client: &Client, endpoint: &str, server_token: Option<&str>) -> Result<()> {
    let peers = fetch_peers(client, endpoint, server_token).await?;
    if peers.peers.is_empty() {
        println!("No peers configured. Add one with: mnemos-desktop peer add <URL> --name <NAME>");
        return Ok(());
    }
    println!("{:<20} {:<9} URL", "NAME", "STATUS");
    for peer in peers.peers {
        let status = if peer.enabled { "enabled" } else { "disabled" };
        println!("{:<20} {:<9} {}", peer.name, status, peer.base_url);
    }
    Ok(())
}

async fn fetch_peers(
    client: &Client,
    endpoint: &str,
    server_token: Option<&str>,
) -> Result<FederationPeerList> {
    let response = authorized(client.get(endpoint), server_token)
        .send()
        .await
        .context("peer list request failed")?;
    let response = ensure_success(response).await?;
    response
        .json::<FederationPeerList>()
        .await
        .context("invalid peer list response")
}

fn authorized(request: reqwest::RequestBuilder, token: Option<&str>) -> reqwest::RequestBuilder {
    match token.filter(|token| !token.is_empty()) {
        Some(token) => request.bearer_auth(token),
        None => request,
    }
}

async fn ensure_success(response: Response) -> Result<Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let detail = response
        .text()
        .await
        .ok()
        .and_then(|text| {
            serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|value| value.get("detail")?.as_str().map(str::to_string))
                .or_else(|| (!text.is_empty()).then_some(text))
        })
        .unwrap_or_default();
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        anyhow::bail!(
            "HTTP {status}: unauthorized; set MNEMOS_TOKEN (or MNEMOS_API_KEY) or use --credentials-file"
        );
    }
    if detail.is_empty() {
        anyhow::bail!("HTTP {status}");
    }
    anyhow::bail!("HTTP {status}: {detail}")
}

fn default_name_from_url(url: &str) -> Result<String> {
    let parsed = reqwest::Url::parse(url).context("invalid peer URL")?;
    let host = parsed.host_str().context("peer URL must include a host")?;
    let mut name = host
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string();

    let starts_with_letter = name.as_bytes().first().is_some_and(u8::is_ascii_lowercase);
    if name.len() < 3 || !starts_with_letter {
        name = if name.is_empty() {
            "peer".to_string()
        } else {
            format!("peer-{name}")
        };
    }
    if name.len() > 64 {
        name.truncate(64);
        name = name.trim_end_matches('-').to_string();
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_name_matches_core_peer_name_contract() {
        assert_eq!(
            default_name_from_url("https://Pythia.Local/v1").unwrap(),
            "pythia-local"
        );
        assert_eq!(
            default_name_from_url("http://192.168.1.5:5002").unwrap(),
            "peer-192-168-1-5"
        );
        assert_eq!(
            default_name_from_url("https://[2001:db8::1]:5002").unwrap(),
            "peer-2001-db8--1"
        );
        assert!(default_name_from_url("").is_err());
    }
}

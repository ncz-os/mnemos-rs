use std::process::Command;

use httpmock::prelude::*;

fn mnemosctl() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mnemosctl"))
}

fn peer_json(id: &str, name: &str, base_url: &str, enabled: bool) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "name": name,
        "base_url": base_url,
        "enabled": enabled
    })
}

#[test]
fn add_registers_peer_with_core_api_and_distinct_tokens() {
    let server = MockServer::start();
    let list = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/federation/peers")
            .header("authorization", "Bearer root-secret");
        then.status(200).json_body_obj(&serde_json::json!({
            "count": 0,
            "peers": []
        }));
    });
    let create = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/federation/peers")
            .header("authorization", "Bearer root-secret")
            .json_body_obj(&serde_json::json!({
                "name": "pythia",
                "base_url": "https://peer.example",
                "auth_token": "peer-secret",
                "enabled": true,
                "sync_interval_secs": 300,
                "compat_mode": "strict"
            }));
        then.status(201).json_body_obj(&peer_json(
            "4e4420ea-7410-4465-8074-dc73aef13be8",
            "pythia",
            "https://peer.example",
            true,
        ));
    });

    let output = mnemosctl()
        .args([
            "--base",
            &server.base_url(),
            "--token",
            "root-secret",
            "peer",
            "add",
            "https://peer.example",
            "--name",
            "pythia",
            "--peer-token",
            "peer-secret",
        ])
        .output()
        .expect("mnemosctl runs");

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    list.assert();
    create.assert();
}

#[test]
fn add_without_peer_token_registers_anonymous_peer() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/v1/federation/peers");
        then.status(200)
            .json_body_obj(&serde_json::json!({"count": 0, "peers": []}));
    });
    let create = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/federation/peers")
            .json_body_obj(&serde_json::json!({
                "name": "desktop-local",
                "base_url": "https://desktop.local",
                "auth_token": "",
                "enabled": true,
                "sync_interval_secs": 300,
                "compat_mode": "strict"
            }));
        then.status(201).json_body_obj(&peer_json(
            "d033598d-b523-480c-b4fc-76a9a2393bf5",
            "desktop-local",
            "https://desktop.local",
            true,
        ));
    });

    let output = mnemosctl()
        .args([
            "--base",
            &server.base_url(),
            "peer",
            "add",
            "https://desktop.local",
        ])
        .output()
        .expect("mnemosctl runs");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    create.assert();
}

#[test]
fn readding_existing_peer_patches_server_row() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/v1/federation/peers");
        then.status(200).json_body_obj(&serde_json::json!({
            "count": 1,
            "peers": [peer_json(
                "53e0ca1f-c9fb-4809-9466-afc0f4a54db5",
                "pythia",
                "https://old.example",
                true
            )]
        }));
    });
    let update = server.mock(|when, then| {
        when.method(PATCH)
            .path("/v1/federation/peers/53e0ca1f-c9fb-4809-9466-afc0f4a54db5")
            .json_body_obj(&serde_json::json!({
                "base_url": "https://new.example",
                "auth_token": "new-peer-secret"
            }));
        then.status(200).json_body_obj(&peer_json(
            "53e0ca1f-c9fb-4809-9466-afc0f4a54db5",
            "pythia",
            "https://new.example",
            true,
        ));
    });

    let output = mnemosctl()
        .args([
            "--base",
            &server.base_url(),
            "peer",
            "add",
            "https://new.example",
            "--name",
            "pythia",
            "--peer-token",
            "new-peer-secret",
        ])
        .output()
        .expect("mnemosctl runs");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    update.assert();
}

#[test]
fn url_change_preserves_server_stored_token_only_when_explicit() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/v1/federation/peers");
        then.status(200).json_body_obj(&serde_json::json!({
            "count": 1,
            "peers": [peer_json("peer-id", "pythia", "https://old.example", true)]
        }));
    });
    let update = server.mock(|when, then| {
        when.method(PATCH)
            .path("/v1/federation/peers/peer-id")
            .json_body_obj(&serde_json::json!({"base_url": "https://new.example"}));
        then.status(200).json_body_obj(&peer_json(
            "peer-id",
            "pythia",
            "https://new.example",
            true,
        ));
    });

    let output = mnemosctl()
        .args([
            "--base",
            &server.base_url(),
            "peer",
            "add",
            "https://new.example",
            "--name",
            "pythia",
            "--preserve-token",
        ])
        .output()
        .expect("mnemosctl runs");
    assert!(output.status.success());
    update.assert();
}

#[test]
fn clear_token_sends_core_anonymous_token_representation() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/v1/federation/peers");
        then.status(200).json_body_obj(&serde_json::json!({
            "count": 1,
            "peers": [peer_json("peer-id", "pythia", "https://peer.example", true)]
        }));
    });
    let update = server.mock(|when, then| {
        when.method(PATCH)
            .path("/v1/federation/peers/peer-id")
            .json_body_obj(&serde_json::json!({
                "base_url": "https://peer.example",
                "auth_token": ""
            }));
        then.status(200).json_body_obj(&peer_json(
            "peer-id",
            "pythia",
            "https://peer.example",
            true,
        ));
    });

    let output = mnemosctl()
        .args([
            "--base",
            &server.base_url(),
            "peer",
            "add",
            "https://peer.example",
            "--name",
            "pythia",
            "--clear-token",
        ])
        .output()
        .expect("mnemosctl runs");
    assert!(output.status.success());
    update.assert();
}

#[test]
fn list_reads_database_backed_registry_and_does_not_claim_token_state() {
    let server = MockServer::start();
    let list = server.mock(|when, then| {
        when.method(GET).path("/v1/federation/peers");
        then.status(200).json_body_obj(&serde_json::json!({
            "count": 2,
            "peers": [
                peer_json("1", "pythia", "https://pythia.example", true),
                peer_json("2", "argonas", "https://argonas.example", false)
            ]
        }));
    });

    let output = mnemosctl()
        .args(["--base", &server.base_url(), "peer", "list"])
        .output()
        .expect("mnemosctl runs");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    list.assert();
    let stdout = String::from_utf8(output.stdout).expect("utf8 stdout");
    assert!(stdout.contains("pythia") && stdout.contains("enabled"));
    assert!(stdout.contains("argonas") && stdout.contains("disabled"));
    assert!(!stdout.contains("TOKEN"));
}

#[test]
fn server_rejection_is_not_reported_as_success() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/v1/federation/peers");
        then.status(401)
            .json_body_obj(&serde_json::json!({"detail": "invalid API key"}));
    });

    let output = mnemosctl()
        .args(["--base", &server.base_url(), "peer", "list"])
        .output()
        .expect("mnemosctl runs");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("utf8 stderr");
    assert!(stderr.contains("unauthorized"), "stderr={stderr}");
}

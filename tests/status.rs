use std::process::Command;

use httpmock::prelude::*;
use serde_json::Value;

fn mnemosctl() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mnemosctl"))
}

#[test]
fn happy_path_prints_friendly_summary() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/health");
        then.status(200).json_body_obj(&serde_json::json!({
            "name": "pythia",
            "status": "ok",
            "version": "4.2.0a1",
            "profile": "server",
            "db_connected": true
        }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/stats")
            .header("authorization", "Bearer test-token");
        then.status(200).json_body_obj(&serde_json::json!({
            "total_memories": 8888,
            "native_memories": 8888,
            "federated_memories": 0,
            "total_compressions": 489,
            "pending_compressions": 0,
            "nats_enabled": true,
            "nats_streams": 3
        }));
    });

    let output = mnemosctl()
        .args([
            "--base",
            &server.base_url(),
            "--token",
            "test-token",
            "status",
        ])
        .output()
        .expect("mnemosctl runs");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(stdout.contains("MNEMOS pythia (v4.2.0a1, server profile)"));
    assert!(stdout.contains("Memories:  8888 total (8888 native, 0 federated)"));
}

#[test]
fn health_unreachable_exits_2_with_stderr_message() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind test port");
    let addr = listener.local_addr().expect("local addr");
    drop(listener);

    let output = mnemosctl()
        .args(["--base", &format!("http://{addr}"), "status"])
        .output()
        .expect("mnemosctl runs");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(stderr.contains("MNEMOS UNREACHABLE:"));
}

#[test]
fn stats_401_prints_unavailable_but_succeeds() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/health");
        then.status(200).json_body_obj(&serde_json::json!({
            "name": "pythia",
            "status": "ok",
            "version": "4.2.0a1",
            "profile": "server",
            "db_connected": true
        }));
    });
    server.mock(|when, then| {
        when.method(GET).path("/stats");
        then.status(401);
    });

    let output = mnemosctl()
        .args(["--base", &server.base_url(), "status"])
        .output()
        .expect("mnemosctl runs");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(stdout.contains("Memories:  unavailable"));
}

#[test]
fn raw_prints_health_stats_union_shape() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/health");
        then.status(200).json_body_obj(&serde_json::json!({
            "status": "ok",
            "version": "4.2.0a1",
            "profile": "server",
            "db_connected": true
        }));
    });
    server.mock(|when, then| {
        when.method(GET).path("/stats");
        then.status(200).json_body_obj(&serde_json::json!({
            "total_memories": 42,
            "native_memories": 40,
            "federated_memories": 2,
            "total_compressions": 7
        }));
    });

    let output = mnemosctl()
        .args(["--base", &server.base_url(), "status", "--raw"])
        .output()
        .expect("mnemosctl runs");

    assert!(output.status.success());
    let stdout: Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    assert_eq!(stdout["health"]["version"], "4.2.0a1");
    assert_eq!(stdout["stats"]["total_memories"], 42);
}

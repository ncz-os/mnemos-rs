use std::process::Command;

use httpmock::prelude::*;
use serde_json::Value;

fn mnemosctl() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mnemos-desktop"))
}

#[test]
fn happy_path_prints_friendly_summary() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST)
            .path("/v1/memories/search")
            .header("authorization", "Bearer test-token")
            .json_body_obj(&serde_json::json!({
                "query": "kubernetes",
                "limit": 5
            }));
        then.status(200).json_body_obj(&serde_json::json!({
            "memories": [
                {
                    "id": "mem_177abcdef0001",
                    "category": "facts",
                    "created_at": "2026-04-29T18:02:12Z",
                    "content": "Kubernetes pod taints survive node restarts and reschedules"
                },
                {
                    "id": "mem_177abcdef0002",
                    "category": "solutions",
                    "created_at": "2026-04-28T14:30:00Z",
                    "content": "kubectl drain --delete-emptydir-data handles local pod data"
                },
                {
                    "id": "mem_177abcdef0003",
                    "category": "anchor",
                    "created_at": "2026-04-26T11:18:45Z",
                    "content": "argonautsystems cluster off hours maintenance window"
                }
            ],
            "count": 3,
            "latency_ms": 47
        }));
    });

    let output = mnemosctl()
        .args([
            "--base",
            &server.base_url(),
            "--token",
            "test-token",
            "search",
            "kubernetes",
            "--limit",
            "5",
        ])
        .output()
        .expect("mnemosctl runs");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(stdout.contains("3 memories matching \"kubernetes\" (took 47ms):"));
    assert!(stdout.contains("[mem_177abcde] facts      2026-04-29 18:02 — Kubernetes pod taints survive node restarts and reschedules"));
    assert!(stdout.contains("[mem_177abcde] solutions  2026-04-28 14:30 — kubectl drain --delete-emptydir-data handles local pod data"));
    assert!(stdout.contains("[mem_177abcde] anchor     2026-04-26 11:18 — argonautsystems cluster off hours maintenance window"));
}

#[test]
fn empty_results_prints_no_match() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/v1/memories/search");
        then.status(200).json_body_obj(&serde_json::json!({
            "memories": [],
            "count": 0
        }));
    });

    let output = mnemosctl()
        .args(["--base", &server.base_url(), "search", "nomatch"])
        .output()
        .expect("mnemosctl runs");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert_eq!(stdout.trim(), "No memories match \"nomatch\".");
}

#[test]
fn malformed_success_response_returns_an_error() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/v1/memories/search");
        then.status(200).json_body_obj(&serde_json::json!({
            "error": "upstream unavailable"
        }));
    });

    let output = mnemosctl()
        .args(["--base", &server.base_url(), "search", "kubernetes"])
        .output()
        .expect("mnemosctl runs");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(stderr.contains("invalid search response"));
}

#[test]
fn non_ascii_timestamp_does_not_panic() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/v1/memories/search");
        then.status(200).json_body_obj(&serde_json::json!({
            "memories": [{
                "id": "memory-1",
                "category": "facts",
                "created_at": "123456789012345é",
                "content": "content"
            }],
            "count": 1
        }));
    });

    let output = mnemosctl()
        .args(["--base", &server.base_url(), "search", "unicode"])
        .output()
        .expect("mnemosctl runs");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(stdout.contains("123456789012345é"));
}

#[test]
fn _401_prints_auth_error_exit_3() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/v1/memories/search");
        then.status(401);
    });

    let output = mnemosctl()
        .args(["--base", &server.base_url(), "search", "kubernetes"])
        .output()
        .expect("mnemosctl runs");

    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(stderr.contains("Unauthorized: provide --token or set MNEMOS_API_KEY"));
}

#[test]
fn _500_prints_generic_error_exit_2() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/v1/memories/search");
        then.status(500);
    });

    let output = mnemosctl()
        .args(["--base", &server.base_url(), "search", "kubernetes"])
        .output()
        .expect("mnemosctl runs");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(stderr.contains("HTTP 500 Internal Server Error"));
}

#[test]
fn raw_dumps_full_json() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/v1/memories/search");
        then.status(200).json_body_obj(&serde_json::json!({
            "memories": [
                {
                    "id": "mem_177abcdef0001",
                    "category": "facts",
                    "created_at": "2026-04-29T18:02:12Z",
                    "content": "Kubernetes pod taints survive"
                }
            ],
            "count": 1,
            "latency_ms": 47
        }));
    });

    let output = mnemosctl()
        .args([
            "--base",
            &server.base_url(),
            "search",
            "kubernetes",
            "--raw",
        ])
        .output()
        .expect("mnemosctl runs");

    assert!(output.status.success());
    let stdout: Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    assert_eq!(stdout["count"], 1);
    assert_eq!(stdout["latency_ms"], 47);
    assert_eq!(stdout["memories"][0]["id"], "mem_177abcdef0001");
}

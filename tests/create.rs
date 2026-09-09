use std::io::Write;
use std::process::{Command, Stdio};

use httpmock::prelude::*;
use serde_json::Value;

fn mnemosctl() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mnemos-desktop"));
    command
        .env_remove("MNEMOS_BASE")
        .env_remove("MNEMOS_TOKEN")
        .env_remove("MNEMOS_API_KEY")
        .env_remove("MNEMOS_PEER_TOKEN")
        .env_remove("MNEMOS_CREDENTIALS_FILE");
    command
}

#[test]
fn happy_path_explicit_permission_mode_prints_summary() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST)
            .path("/v1/memories")
            .header("authorization", "Bearer test-token")
            .json_body_obj(&serde_json::json!({
                "content": "v4.2.0a2 ships",
                "category": "facts",
                "permission_mode": 644,
            }));
        then.status(201).json_body_obj(&serde_json::json!({
            "id": "mem_177abcdef0001",
            "category": "facts",
            "permission_mode": 644,
            "content": "v4.2.0a2 ships",
            "created": "2026-08-10T12:00:00",
        }));
    });

    let output = mnemosctl()
        .env("MNEMOS_TOKEN", "test-token")
        .args([
            "--base",
            &server.base_url(),
            "create",
            "--content",
            "v4.2.0a2 ships",
            "--category",
            "facts",
            "--permission-mode",
            "644",
        ])
        .output()
        .expect("mnemosctl runs");

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert_eq!(stdout.trim(), "Created mem_177abcdef0001 (facts, mode=644)");
}

#[test]
fn omits_permission_mode_field_when_unset() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/v1/memories").matches(|req| {
            let body: Value = req
                .body
                .as_deref()
                .map(serde_json::from_slice)
                .and_then(Result::ok)
                .unwrap_or(Value::Null);
            body.is_object() && body.get("permission_mode").is_none()
        });
        then.status(201).json_body_obj(&serde_json::json!({
            "id": "mem_177abcdef0042",
            "category": "facts",
            "content": "no mode flag",
            "created": "2026-08-10T12:00:00",
        }));
    });

    let output = mnemosctl()
        .args([
            "--base",
            &server.base_url(),
            "create",
            "--content",
            "no mode flag",
        ])
        .output()
        .expect("mnemosctl runs");

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(stdout.contains("Created mem_177abcdef0042"));
}

#[test]
fn reads_content_from_stdin_when_flag_absent() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/v1/memories").matches(|req| {
            let body: Value = req
                .body
                .as_deref()
                .map(serde_json::from_slice)
                .and_then(Result::ok)
                .unwrap_or(Value::Null);
            body.get("content").and_then(Value::as_str) == Some("piped content")
        });
        then.status(201).json_body_obj(&serde_json::json!({
            "id": "mem_177abcdef0099",
            "category": "facts",
            "content": "piped content",
            "created": "2026-08-10T12:00:00",
        }));
    });

    let mut child = mnemosctl()
        .args(["--base", &server.base_url(), "create"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("mnemosctl spawns");

    child
        .stdin
        .as_mut()
        .expect("stdin pipe")
        .write_all(b"piped content\n")
        .expect("write to stdin");

    let output = child.wait_with_output().expect("mnemosctl finishes");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(stdout.contains("Created mem_177abcdef0099"));
}

#[test]
fn empty_stdin_exits_2_with_error() {
    let server = MockServer::start();
    // No mock — request must NOT reach the server when validation fails locally.
    let count = server.mock(|when, then| {
        when.method(POST).path("/v1/memories");
        then.status(500);
    });

    let mut child = mnemosctl()
        .args(["--base", &server.base_url(), "create"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("mnemosctl spawns");
    drop(child.stdin.take());
    let output = child.wait_with_output().expect("mnemosctl finishes");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(stderr.contains("empty content"));
    count.assert_hits(0);
}

#[test]
fn validation_error_422_surfaces_server_detail_exit_2() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/v1/memories");
        then.status(422).json_body_obj(&serde_json::json!({
            "detail": "permission_mode must be octal-style 0-777",
        }));
    });

    let output = mnemosctl()
        .args([
            "--base",
            &server.base_url(),
            "create",
            "--content",
            "x",
            "--permission-mode",
            "999",
        ])
        .output()
        .expect("mnemosctl runs");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(
        stderr.contains("permission_mode must be octal-style 0-777"),
        "stderr was: {stderr}"
    );
}

#[test]
fn unauthorized_401_exits_3() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/v1/memories");
        then.status(401);
    });

    let output = mnemosctl()
        .args([
            "--base",
            &server.base_url(),
            "create",
            "--content",
            "needs auth",
        ])
        .output()
        .expect("mnemosctl runs");

    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(stderr.contains("Unauthorized: set MNEMOS_TOKEN"));
}

#[test]
fn server_500_exits_2_with_status_message() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/v1/memories");
        then.status(500);
    });

    let output = mnemosctl()
        .args(["--base", &server.base_url(), "create", "--content", "boom"])
        .output()
        .expect("mnemosctl runs");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(stderr.contains("HTTP 500 Internal Server Error"));
}

#[test]
fn raw_dumps_full_json_response() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/v1/memories");
        then.status(201).json_body_obj(&serde_json::json!({
            "id": "mem_177abcdef0007",
            "category": "facts",
            "permission_mode": 600,
            "content": "raw dump",
            "created": "2026-08-10T12:00:00",
            "metadata": {},
        }));
    });

    let output = mnemosctl()
        .args([
            "--base",
            &server.base_url(),
            "create",
            "--content",
            "raw dump",
            "--raw",
        ])
        .output()
        .expect("mnemosctl runs");

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let parsed: Value = serde_json::from_slice(&output.stdout).expect("stdout is JSON");
    assert_eq!(parsed["id"], "mem_177abcdef0007");
    assert_eq!(parsed["permission_mode"], 600);
}

#[test]
fn passes_subcategory_through() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/v1/memories").matches(|req| {
            let body: Value = req
                .body
                .as_deref()
                .map(serde_json::from_slice)
                .and_then(Result::ok)
                .unwrap_or(Value::Null);
            body.get("subcategory").and_then(Value::as_str) == Some("infra")
        });
        then.status(201).json_body_obj(&serde_json::json!({
            "id": "mem_177abcdef0123",
            "category": "facts",
            "content": "scoped",
            "created": "2026-08-10T12:00:00",
        }));
    });

    let output = mnemosctl()
        .args([
            "--base",
            &server.base_url(),
            "create",
            "--content",
            "scoped",
            "--subcategory",
            "infra",
        ])
        .output()
        .expect("mnemosctl runs");

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn malformed_success_response_is_a_protocol_error() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(POST).path("/v1/memories");
        then.status(200).json_body_obj(&serde_json::json!({
            "error": "upstream unavailable"
        }));
    });

    let output = mnemosctl()
        .args([
            "--base",
            &server.base_url(),
            "create",
            "--content",
            "not created",
        ])
        .output()
        .expect("mnemosctl runs");

    assert!(!output.status.success());
    assert!(String::from_utf8(output.stdout)
        .expect("stdout is utf8")
        .is_empty());
    assert!(String::from_utf8(output.stderr)
        .expect("stderr is utf8")
        .contains("invalid create response"));
}

use std::io::Write;
use std::process::Command;

use httpmock::prelude::*;
use serde_json::Value;
use tempfile::NamedTempFile;

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
        .env("MNEMOS_TOKEN", "test-token")
        .args([
            "--base",
            &server.base_url(),
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
    assert!(stderr.contains("Unauthorized: set MNEMOS_TOKEN"));
}

#[test]
fn mode_0600_credentials_file_supplies_server_token() {
    let server = MockServer::start();
    let search = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/memories/search")
            .header("authorization", "Bearer file-token");
        then.status(200)
            .json_body_obj(&serde_json::json!({"memories": [], "count": 0}));
    });
    let mut credentials = NamedTempFile::new().expect("credentials file");
    credentials
        .write_all(b"token = \"file-token\"\n")
        .expect("write credentials");
    credentials.flush().expect("flush credentials");

    let output = mnemosctl()
        .env("MNEMOS_TOKEN", "ambient-token-must-not-win")
        .args([
            "--base",
            &server.base_url(),
            "--credentials-file",
            credentials.path().to_str().expect("utf8 path"),
            "search",
            "credentials",
        ])
        .output()
        .expect("mnemosctl runs");

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    search.assert();
}

#[cfg(unix)]
#[test]
fn credentials_file_with_permissive_mode_is_rejected_before_request() {
    use std::os::unix::fs::PermissionsExt;

    let server = MockServer::start();
    let search = server.mock(|when, then| {
        when.method(POST).path("/v1/memories/search");
        then.status(200)
            .json_body_obj(&serde_json::json!({"memories": [], "count": 0}));
    });
    let mut credentials = NamedTempFile::new().expect("credentials file");
    credentials
        .write_all(b"token = \"file-token\"\n")
        .expect("write credentials");
    credentials.flush().expect("flush credentials");
    std::fs::set_permissions(credentials.path(), std::fs::Permissions::from_mode(0o644))
        .expect("chmod credentials");

    let output = mnemosctl()
        .args([
            "--base",
            &server.base_url(),
            "--credentials-file",
            credentials.path().to_str().expect("utf8 path"),
            "search",
            "credentials",
        ])
        .output()
        .expect("mnemosctl runs");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(stderr.contains("must have mode 0600"), "stderr={stderr}");
    search.assert_hits(0);
}

#[test]
fn credentialed_search_does_not_follow_plaintext_redirects() {
    let server = MockServer::start();
    let search = server.mock(|when, then| {
        when.method(POST)
            .path("/v1/memories/search")
            .header("authorization", "Bearer env-token");
        then.status(307)
            .header("location", "http://192.0.2.10:5002/capture-token");
    });

    let output = mnemosctl()
        .env("MNEMOS_TOKEN", "env-token")
        .args(["--base", &server.base_url(), "search", "redirect"])
        .output()
        .expect("mnemosctl runs");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(stderr.contains("HTTP 307"), "stderr={stderr}");
    search.assert();
}

#[test]
fn credentialed_loopback_search_ignores_ambient_http_proxy() {
    let origin = MockServer::start();
    let search = origin.mock(|when, then| {
        when.method(POST)
            .path("/v1/memories/search")
            .header("authorization", "Bearer env-token");
        then.status(200)
            .json_body_obj(&serde_json::json!({"memories": [], "count": 0}));
    });
    let proxy = MockServer::start();
    let intercepted = proxy.mock(|when, then| {
        when.method(POST);
        then.status(502);
    });

    let output = mnemosctl()
        .env("MNEMOS_TOKEN", "env-token")
        .env("HTTP_PROXY", proxy.base_url())
        .env("http_proxy", proxy.base_url())
        .env_remove("REQUEST_METHOD")
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .args(["--base", &origin.base_url(), "search", "proxy-bypass"])
        .output()
        .expect("mnemosctl runs");

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    search.assert();
    intercepted.assert_hits(0);
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

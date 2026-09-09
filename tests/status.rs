use std::process::Command;

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
fn happy_path_prints_friendly_summary() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/health");
        then.status(200).json_body_obj(&serde_json::json!({
            "name": "pythia",
            "status": "ok",
            "timestamp": "2026-08-10T12:00:00",
            "version": "4.2.0a1",
            "profile": "server",
            "database_connected": true,
            "nats_publishing_enabled": true
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
            "average_compression_ratio": 0.57,
            "average_quality_rating": 75,
            "memories_by_category": {"facts": 8888},
            "unreviewed_compressions": 12,
            "timestamp": "2026-08-10T12:00:00",
            "nats_streams": 3
        }));
    });

    let output = mnemosctl()
        .env("MNEMOS_TOKEN", "test-token")
        .args(["--base", &server.base_url(), "status"])
        .output()
        .expect("mnemosctl runs");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(stdout.contains("MNEMOS pythia (v4.2.0a1, server profile)"));
    assert!(stdout.contains("DB:        connected"));
    assert!(stdout.contains("NATS:      enabled (3 streams)"));
    assert!(stdout.contains("Memories:  8888 total (8888 native, 0 federated)"));
    assert!(stdout.contains("Compressions: 489 done, 12 unreviewed"));
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
fn degraded_503_is_reported_with_health_details_when_stats_are_unavailable() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/health");
        then.status(503).json_body_obj(&serde_json::json!({
            "status": "degraded",
            "timestamp": "2026-08-10T12:00:00",
            "database_connected": false,
            "version": "6.2.0",
            "profile": "server",
            "distillation_worker": "healthy",
            "deletion_request_worker": "error"
        }));
    });
    server.mock(|when, then| {
        when.method(GET).path("/stats");
        then.status(503)
            .json_body_obj(&serde_json::json!({"detail": "Persistence backend not available"}));
    });

    let output = mnemosctl()
        .args(["--base", &server.base_url(), "status"])
        .output()
        .expect("mnemosctl runs");

    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(
        stdout.contains("DB:        disconnected"),
        "stdout={stdout}"
    );
    assert!(stdout.contains("Memories:  unavailable"), "stdout={stdout}");
    assert!(
        stdout.contains("Deletion request worker: error"),
        "stdout={stdout}"
    );
    assert!(stdout.contains("Health:    DEGRADED"), "stdout={stdout}");
    assert!(stderr.contains("statistics unavailable"), "stderr={stderr}");
    assert!(!stderr.contains("MNEMOS UNREACHABLE"), "stderr={stderr}");
}

#[test]
fn stats_401_returns_an_error() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/health");
        then.status(200).json_body_obj(&serde_json::json!({
            "name": "pythia",
            "status": "ok",
            "timestamp": "2026-08-10T12:00:00",
            "version": "4.2.0a1",
            "profile": "server",
            "database_connected": true
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

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(stderr.contains("failed to fetch MNEMOS stats"));
    assert!(stderr.contains("HTTP 401 Unauthorized"));
}

#[test]
fn raw_prints_health_stats_union_shape() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/health");
        then.status(200).json_body_obj(&serde_json::json!({
            "status": "ok",
            "timestamp": "2026-08-10T12:00:00",
            "version": "4.2.0a1",
            "profile": "server",
            "database_connected": true
        }));
    });
    server.mock(|when, then| {
        when.method(GET).path("/stats");
        then.status(200).json_body_obj(&serde_json::json!({
            "total_memories": 42,
            "native_memories": 40,
            "federated_memories": 2,
            "total_compressions": 7,
            "average_compression_ratio": 0.57,
            "average_quality_rating": 75,
            "memories_by_category": {"facts": 42},
            "unreviewed_compressions": 0,
            "timestamp": "2026-08-10T12:00:00"
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

#[test]
fn distillation_worker_error_is_displayed_as_degraded_and_exits_nonzero() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/health");
        then.status(200).json_body_obj(&serde_json::json!({
            "status": "healthy",
            "timestamp": "2026-08-10T12:00:00",
            "database_connected": true,
            "version": "6.2.0",
            "profile": "server",
            "distillation_worker": "error"
        }));
    });
    server.mock(|when, then| {
        when.method(GET).path("/stats");
        then.status(200).json_body_obj(&serde_json::json!({
            "total_memories": 1,
            "native_memories": 1,
            "federated_memories": 0,
            "total_compressions": 0,
            "average_compression_ratio": 0.57,
            "average_quality_rating": 75,
            "memories_by_category": {"facts": 1},
            "unreviewed_compressions": 0,
            "timestamp": "2026-08-10T12:00:00"
        }));
    });

    let output = mnemosctl()
        .args(["--base", &server.base_url(), "status"])
        .output()
        .expect("mnemosctl runs");

    assert!(!output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(stdout.contains("Distillation worker: error"));
    assert!(stdout.contains("Health:    DEGRADED"));
}

#[test]
fn distillation_worker_starting_is_displayed_as_degraded_and_exits_nonzero() {
    // `starting` is unhealthy whenever it appears. Server and dev enable
    // distillation in their manifests; edge can enable it through legacy
    // defaults, a component selection, or an explicit service override.
    for profile in ["server", "dev", "edge"] {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/health");
            then.status(200).json_body_obj(&serde_json::json!({
                "status": "healthy",
                "timestamp": "2026-08-10T12:00:00",
                "database_connected": true,
                "version": "6.2.0",
                "profile": profile,
                "distillation_worker": "starting"
            }));
        });
        server.mock(|when, then| {
            when.method(GET).path("/stats");
            then.status(200).json_body_obj(&serde_json::json!({
                "total_memories": 1,
                "native_memories": 1,
                "federated_memories": 0,
                "total_compressions": 0,
                "average_compression_ratio": 0.57,
                "average_quality_rating": 75,
                "memories_by_category": {"facts": 1},
                "unreviewed_compressions": 0,
                "timestamp": "2026-08-10T12:00:00"
            }));
        });

        let output = mnemosctl()
            .args(["--base", &server.base_url(), "status"])
            .output()
            .expect("mnemosctl runs");

        assert!(!output.status.success(), "profile={profile}");
        let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
        assert!(stdout.contains("Distillation worker: starting"));
        assert!(stdout.contains("Health:    DEGRADED"));
    }
}

#[test]
fn disabled_distillation_worker_is_healthy_for_edge_profile() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/health");
        then.status(200).json_body_obj(&serde_json::json!({
            "status": "healthy",
            "timestamp": "2026-08-10T12:00:00",
            "database_connected": true,
            "version": "6.2.0",
            "profile": "edge",
            "distillation_worker": "disabled"
        }));
    });
    server.mock(|when, then| {
        when.method(GET).path("/stats");
        then.status(200).json_body_obj(&serde_json::json!({
            "total_memories": 0,
            "total_compressions": 0,
            "average_compression_ratio": 0.57,
            "average_quality_rating": 75,
            "memories_by_category": {},
            "unreviewed_compressions": 0,
            "timestamp": "2026-08-10T12:00:00"
        }));
    });

    let output = mnemosctl()
        .args(["--base", &server.base_url(), "status"])
        .output()
        .expect("mnemosctl runs");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(stdout.contains("Distillation worker: disabled"));
    assert!(stdout.contains("Health:    HEALTHY"));
}

#[test]
fn malformed_health_2xx_is_a_protocol_error() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/health");
        then.status(200).json_body_obj(&serde_json::json!({
            "error": "upstream unavailable"
        }));
    });

    let output = mnemosctl()
        .args(["--base", &server.base_url(), "status"])
        .output()
        .expect("mnemosctl runs");

    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8(output.stdout)
        .expect("stdout is utf8")
        .is_empty());
    assert!(String::from_utf8(output.stderr)
        .expect("stderr is utf8")
        .contains("invalid JSON response"));
}

#[test]
fn malformed_stats_2xx_is_a_protocol_error() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/health");
        then.status(200).json_body_obj(&serde_json::json!({
            "status": "healthy",
            "timestamp": "2026-08-10T12:00:00",
            "database_connected": true,
            "version": "6.2.0",
            "profile": "edge",
            "distillation_worker": "disabled"
        }));
    });
    server.mock(|when, then| {
        when.method(GET).path("/stats");
        then.status(200).json_body_obj(&serde_json::json!({
            "error": "upstream unavailable"
        }));
    });

    let output = mnemosctl()
        .args(["--base", &server.base_url(), "status"])
        .output()
        .expect("mnemosctl runs");

    assert!(!output.status.success());
    assert!(String::from_utf8(output.stdout)
        .expect("stdout is utf8")
        .is_empty());
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(stderr.contains("failed to fetch MNEMOS stats"));
    assert!(stderr.contains("invalid JSON response"));
}

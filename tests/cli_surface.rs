use std::process::Command;

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
fn server_token_is_rejected_in_argv_with_safe_source_guidance() {
    let output = mnemosctl()
        .args(["--token", "real-looking-secret", "status"])
        .output()
        .expect("mnemosctl runs");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(stderr.contains("not accepted in argv"), "stderr={stderr}");
    assert!(stderr.contains("MNEMOS_TOKEN"), "stderr={stderr}");
    assert!(stderr.contains("--credentials-file"), "stderr={stderr}");
    assert!(!stderr.contains("real-looking-secret"), "stderr={stderr}");
}

#[test]
fn peer_token_is_rejected_in_argv_with_safe_source_guidance() {
    let output = mnemosctl()
        .args([
            "peer",
            "add",
            "https://peer.example",
            "--peer-token",
            "real-looking-peer-secret",
        ])
        .output()
        .expect("mnemosctl runs");

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(stderr.contains("not accepted in argv"), "stderr={stderr}");
    assert!(stderr.contains("MNEMOS_PEER_TOKEN"), "stderr={stderr}");
    assert!(stderr.contains("--credentials-file"), "stderr={stderr}");
    assert!(
        !stderr.contains("real-looking-peer-secret"),
        "stderr={stderr}"
    );
}

#[test]
fn server_bearer_over_remote_plaintext_http_is_refused_before_network_io() {
    let output = mnemosctl()
        .env("MNEMOS_TOKEN", "server-secret")
        .env("HTTP_PROXY", "http://127.0.0.1:9")
        .env_remove("NO_PROXY")
        .env_remove("no_proxy")
        .args(["--base", "http://192.0.2.10:5002", "status"])
        .output()
        .expect("mnemosctl runs");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(
        stderr.contains("refusing to send bearer credentials"),
        "stderr={stderr}"
    );
    assert!(stderr.contains("non-loopback"), "stderr={stderr}");
    assert!(stderr.contains("--allow-insecure-http"), "stderr={stderr}");
}

#[test]
fn peer_bearer_is_not_registered_for_a_remote_plaintext_peer() {
    let output = mnemosctl()
        .env("MNEMOS_PEER_TOKEN", "peer-secret")
        .args(["peer", "add", "http://192.0.2.11:5002"])
        .output()
        .expect("mnemosctl runs");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(
        stderr.contains("refusing to send bearer credentials"),
        "stderr={stderr}"
    );
    assert!(stderr.contains("--allow-insecure-http"), "stderr={stderr}");
}

#[test]
fn peer_add_protects_potential_server_stored_bearer_on_plaintext_management_url() {
    let output = mnemosctl()
        .args([
            "--base",
            "http://192.0.2.10:5002",
            "peer",
            "add",
            "https://peer.example",
        ])
        .output()
        .expect("mnemosctl runs");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(
        stderr.contains("refusing to send bearer credentials"),
        "stderr={stderr}"
    );
    assert!(stderr.contains("--allow-insecure-http"), "stderr={stderr}");
}

#[test]
fn peer_add_protects_implicitly_preserved_bearer_on_plaintext_peer_url() {
    let output = mnemosctl()
        .args(["peer", "add", "http://192.0.2.11:5002"])
        .output()
        .expect("mnemosctl runs");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(
        stderr.contains("refusing to send bearer credentials"),
        "stderr={stderr}"
    );
    assert!(stderr.contains("--allow-insecure-http"), "stderr={stderr}");
}

#[test]
fn help_does_not_advertise_unimplemented_commands() {
    let output = mnemosctl().arg("--help").output().expect("mnemosctl runs");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(!stdout.contains("mcp-stdio"));
    assert!(!stdout
        .lines()
        .any(|line| line.trim_start().starts_with("sync ")));
}

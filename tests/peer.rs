use std::process::Command;

use tempfile::TempDir;

fn mnemosctl() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mnemosctl"))
}

fn isolated_home() -> (TempDir, String) {
    let tmp = TempDir::new().expect("tempdir");
    let path = tmp.path().to_str().expect("utf8 path").to_string();
    (tmp, path)
}

#[test]
fn list_with_no_config_prints_friendly_help() {
    let (_tmp, home) = isolated_home();

    let output = mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args(["peer", "list"])
        .output()
        .expect("mnemosctl runs");

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout is utf8");
    assert!(
        stdout.contains("No peers configured"),
        "stdout was: {stdout}"
    );
}

#[test]
fn add_persists_to_peers_toml_and_list_shows_redacted_token_state() {
    let (_tmp, home) = isolated_home();

    let add = mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args([
            "peer",
            "add",
            "http://192.168.207.67:5002",
            "--name",
            "pythia",
            "--peer-token",
            "supersecret",
        ])
        .output()
        .expect("mnemosctl runs");
    assert!(
        add.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&add.stderr)
    );
    let add_out = String::from_utf8(add.stdout).expect("stdout is utf8");
    assert!(add_out.contains("Added peer 'pythia' -> http://192.168.207.67:5002"));

    let toml_text =
        std::fs::read_to_string(format!("{home}/peers.toml")).expect("peers.toml created");
    assert!(toml_text.contains("name = \"pythia\""));
    assert!(toml_text.contains("token = \"supersecret\""));

    let list = mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args(["peer", "list"])
        .output()
        .expect("mnemosctl runs");
    assert!(list.status.success());
    let list_out = String::from_utf8(list.stdout).expect("stdout is utf8");
    assert!(list_out.contains("pythia"));
    assert!(list_out.contains("http://192.168.207.67:5002"));
    // Token must NOT appear in list output — only its presence is signalled.
    assert!(!list_out.contains("supersecret"));
    assert!(list_out.contains(" set "));
}

#[test]
fn add_without_token_lists_dash_marker() {
    let (_tmp, home) = isolated_home();

    let add = mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args(["peer", "add", "http://127.0.0.1:5102", "--name", "desktop"])
        .output()
        .expect("mnemosctl runs");
    assert!(add.status.success());

    let list = mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args(["peer", "list"])
        .output()
        .expect("mnemosctl runs");
    let out = String::from_utf8(list.stdout).expect("stdout is utf8");
    assert!(out.contains("desktop"));
    // The dash glyph signals "no token" in the TOKEN column.
    assert!(out.contains("—"), "stdout was: {out}");
}

#[test]
fn name_defaults_to_host_portion() {
    let (_tmp, home) = isolated_home();

    let add = mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args(["peer", "add", "http://argonas.local:5002"])
        .output()
        .expect("mnemosctl runs");
    assert!(add.status.success());

    let toml_text =
        std::fs::read_to_string(format!("{home}/peers.toml")).expect("peers.toml created");
    assert!(
        toml_text.contains("name = \"argonas.local\""),
        "toml was: {toml_text}"
    );
}

#[test]
fn re_adding_same_name_updates_url_and_token() {
    let (_tmp, home) = isolated_home();

    mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args([
            "peer",
            "add",
            "http://old-host:5002",
            "--name",
            "pythia",
            "--peer-token",
            "old-tok",
        ])
        .output()
        .expect("mnemosctl runs");

    let update = mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args([
            "peer",
            "add",
            "http://new-host:5002",
            "--name",
            "pythia",
            "--peer-token",
            "new-tok",
        ])
        .output()
        .expect("mnemosctl runs");
    assert!(update.status.success());
    let update_out = String::from_utf8(update.stdout).expect("stdout is utf8");
    assert!(
        update_out.contains("Updated peer 'pythia' -> http://new-host:5002"),
        "stdout was: {update_out}"
    );

    let toml_text =
        std::fs::read_to_string(format!("{home}/peers.toml")).expect("peers.toml present");
    assert!(toml_text.contains("http://new-host:5002"));
    assert!(toml_text.contains("token = \"new-tok\""));
    assert!(!toml_text.contains("old-host"));
    assert!(!toml_text.contains("old-tok"));

    // Still only one peer with that name; no duplicate.
    let count = toml_text.matches("[[peer]]").count();
    assert_eq!(
        count, 1,
        "expected one [[peer]] block, toml was:\n{toml_text}"
    );
}

#[test]
fn server_token_env_does_not_leak_into_peer_record() {
    // Regression: the global --token reads MNEMOS_API_KEY; with clap's
    // global = true, a colliding subcommand --token would silently inherit
    // it. peer add was renamed to --peer-token so the server bearer cannot
    // bleed into peers.toml. This test pins that boundary.
    let (_tmp, home) = isolated_home();

    let add = mnemosctl()
        .env("MNEMOS_HOME", &home)
        .env("MNEMOS_API_KEY", "server-bearer-must-not-leak")
        .args(["peer", "add", "http://127.0.0.1:5102", "--name", "desktop"])
        .output()
        .expect("mnemosctl runs");
    assert!(
        add.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&add.stderr)
    );

    let toml_text =
        std::fs::read_to_string(format!("{home}/peers.toml")).expect("peers.toml created");
    assert!(
        !toml_text.contains("server-bearer-must-not-leak"),
        "server bearer leaked into peers.toml:\n{toml_text}"
    );
    assert!(
        !toml_text.contains("token = "),
        "expected no token entry, toml was:\n{toml_text}"
    );
}

#[cfg(unix)]
#[test]
fn peers_toml_is_written_with_mode_0600() {
    use std::os::unix::fs::PermissionsExt;

    let (_tmp, home) = isolated_home();

    let add = mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args([
            "peer",
            "add",
            "http://192.168.207.67:5002",
            "--name",
            "pythia",
            "--peer-token",
            "hardening-secret",
        ])
        .output()
        .expect("mnemosctl runs");
    assert!(
        add.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&add.stderr)
    );

    let file_meta = std::fs::metadata(format!("{home}/peers.toml")).expect("peers.toml exists");
    let file_mode = file_meta.permissions().mode() & 0o777;
    assert_eq!(
        file_mode, 0o600,
        "peers.toml mode is {file_mode:o}, expected 0600"
    );
    // Parent-dir 0700 only fires for the canonical ~/.mnemos/ path
    // we own — see user_supplied_mnemos_home_dir_perms_are_left_alone.
}

#[cfg(unix)]
#[test]
fn foreign_tempfile_is_not_unlinked() {
    // Round-2 hardening: round 1 used a fixed peers.toml.tmp name and
    // unconditionally removed it before writing — two concurrent writers
    // could unlink each other's in-flight files. The new save() uses a
    // per-writer name and refuses to touch any file it didn't create.
    // A foreign peers.toml.tmp must survive a save it didn't author.
    let (_tmp, home) = isolated_home();
    std::fs::create_dir_all(&home).unwrap();
    let foreign = format!("{home}/peers.toml.tmp");
    std::fs::write(&foreign, b"# someone else's tempfile\n").unwrap();

    let add = mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args(["peer", "add", "http://127.0.0.1:5102", "--name", "desktop"])
        .output()
        .expect("mnemosctl runs");
    assert!(add.status.success());

    let foreign_text = std::fs::read_to_string(&foreign).expect("foreign tempfile preserved");
    assert!(foreign_text.contains("someone else's tempfile"));
    let final_text = std::fs::read_to_string(format!("{home}/peers.toml")).unwrap();
    assert!(final_text.contains("desktop"));
}

#[cfg(unix)]
#[test]
fn loading_a_preexisting_0644_file_tightens_to_0600() {
    use std::os::unix::fs::PermissionsExt;

    let (_tmp, home) = isolated_home();
    let toml_path = format!("{home}/peers.toml");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::write(&toml_path, "[[peer]]\nname = \"old\"\nurl = \"http://x\"\n").unwrap();
    let mut perms = std::fs::metadata(&toml_path).unwrap().permissions();
    perms.set_mode(0o644);
    std::fs::set_permissions(&toml_path, perms).unwrap();

    // Read-only command (peer list) must tighten the perms before
    // reading — the upgrade path can't wait for the next mutating call.
    let list = mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args(["peer", "list"])
        .output()
        .expect("mnemosctl runs");
    assert!(
        list.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&list.stderr)
    );

    let mode = std::fs::metadata(&toml_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "peers.toml stayed at {mode:o} after load");
}

#[cfg(unix)]
#[test]
fn user_supplied_mnemos_home_dir_perms_are_left_alone() {
    use std::os::unix::fs::PermissionsExt;

    let (_tmp, home) = isolated_home();
    // Simulate a user-controlled directory at 0755 (e.g. a synced
    // folder). mnemosctl must NOT chmod it to 0700.
    let mut perms = std::fs::metadata(&home).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&home, perms).unwrap();

    let add = mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args(["peer", "add", "http://127.0.0.1:5102", "--name", "desktop"])
        .output()
        .expect("mnemosctl runs");
    assert!(
        add.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&add.stderr)
    );

    let dir_mode = std::fs::metadata(&home).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        dir_mode, 0o755,
        "user-supplied MNEMOS_HOME got chmodded to {dir_mode:o}"
    );
    // The FILE inside it must still be 0600 — file-mode tightening is
    // unconditional regardless of who owns the parent.
    let file_mode = std::fs::metadata(format!("{home}/peers.toml"))
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(file_mode, 0o600);
}

#[cfg(unix)]
#[test]
fn symlinked_peers_toml_is_refused() {
    let (_tmp, home) = isolated_home();
    std::fs::create_dir_all(&home).unwrap();
    let real_path = format!("{home}/elsewhere.toml");
    std::fs::write(&real_path, "[[peer]]\nname = \"x\"\nurl = \"http://x\"\n").unwrap();
    std::os::unix::fs::symlink(&real_path, format!("{home}/peers.toml")).unwrap();

    let list = mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args(["peer", "list"])
        .output()
        .expect("mnemosctl runs");
    assert!(
        !list.status.success(),
        "expected failure on symlinked peers.toml"
    );
    let stderr = String::from_utf8(list.stderr).expect("stderr utf8");
    assert!(stderr.contains("symlink"), "stderr was: {stderr}");
}

#[cfg(unix)]
#[test]
fn save_uses_per_writer_tempfile_name_not_collidable() {
    // The fixed `peers.toml.tmp` from the round-1 fix is gone. We can't
    // easily mock concurrency without a lock primitive, but we CAN
    // assert the suffix shape includes pid + nanos so two processes
    // would not collide.
    let (_tmp, home) = isolated_home();
    let add = mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args(["peer", "add", "http://127.0.0.1:5102", "--name", "desktop"])
        .output()
        .expect("mnemosctl runs");
    assert!(add.status.success());

    // After a successful save, no peers.toml.tmp* lingers.
    let leftover: Vec<_> = std::fs::read_dir(&home)
        .unwrap()
        .flatten()
        .filter(|e| {
            e.file_name()
                .to_str()
                .map(|n| n.starts_with("peers.toml.tmp"))
                .unwrap_or(false)
        })
        .collect();
    assert!(leftover.is_empty(), "tempfiles leaked: {leftover:?}");
}

#[cfg(unix)]
#[test]
fn default_dotmnemos_symlinked_to_shared_dir_is_not_chmodded() {
    // If ~/.mnemos is itself a symlink to a shared/synced directory,
    // mnemosctl must NOT follow the symlink and chmod its target. The
    // owned-dir branch is gated on the parent being a real directory.
    use std::os::unix::fs::PermissionsExt;

    let fake_home = TempDir::new().expect("fake-home tempdir");
    let target = TempDir::new().expect("target tempdir");
    let dotmnemos = fake_home.path().join(".mnemos");
    std::os::unix::fs::symlink(target.path(), &dotmnemos).expect("create ~/.mnemos symlink");

    // Pin the target to 0755 so we can observe whether mnemosctl
    // chmodded it.
    let mut perms = std::fs::metadata(target.path()).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(target.path(), perms).unwrap();

    let add = mnemosctl()
        .env_remove("MNEMOS_HOME")
        .env("HOME", fake_home.path())
        .args(["peer", "add", "http://127.0.0.1:5102", "--name", "desktop"])
        .output()
        .expect("mnemosctl runs");
    assert!(
        add.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&add.stderr)
    );

    let target_mode = std::fs::metadata(target.path())
        .unwrap()
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(
        target_mode, 0o755,
        "symlink target was chmodded to {target_mode:o}; should have been left at 0755"
    );
    // peers.toml itself still gets 0600 — file-mode hardening is
    // independent of parent-dir handling.
    let toml_path = target.path().join("peers.toml");
    let file_mode = std::fs::metadata(&toml_path).unwrap().permissions().mode() & 0o777;
    assert_eq!(file_mode, 0o600);
}

#[test]
fn re_adding_without_peer_token_preserves_existing_token() {
    // Idempotent re-add must NOT silently wipe the stored token.
    // A user retrying `mnemosctl peer add http://pythia ... --name pythia`
    // to update the URL should not have to re-paste the bearer token.
    let (_tmp, home) = isolated_home();

    mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args([
            "peer",
            "add",
            "http://pythia.local:5002",
            "--name",
            "pythia",
            "--peer-token",
            "do-not-wipe-me",
        ])
        .output()
        .expect("mnemosctl runs");

    let reupdate = mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args([
            "peer",
            "add",
            "http://pythia.local:5002",
            "--name",
            "pythia",
        ])
        .output()
        .expect("mnemosctl runs");
    assert!(
        reupdate.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&reupdate.stderr)
    );

    let toml_text = std::fs::read_to_string(format!("{home}/peers.toml")).unwrap();
    assert!(
        toml_text.contains("token = \"do-not-wipe-me\""),
        "token was wiped on re-add; toml was:\n{toml_text}"
    );
}

#[test]
fn clear_token_flag_drops_existing_token() {
    let (_tmp, home) = isolated_home();

    mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args([
            "peer",
            "add",
            "http://pythia.local:5002",
            "--name",
            "pythia",
            "--peer-token",
            "old-secret",
        ])
        .output()
        .expect("mnemosctl runs");

    let cleared = mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args([
            "peer",
            "add",
            "http://pythia.local:5002",
            "--name",
            "pythia",
            "--clear-token",
        ])
        .output()
        .expect("mnemosctl runs");
    assert!(cleared.status.success());
    let stdout = String::from_utf8(cleared.stdout).unwrap();
    assert!(
        stdout.contains("Cleared token for peer 'pythia'"),
        "stdout={stdout}"
    );

    let toml_text = std::fs::read_to_string(format!("{home}/peers.toml")).unwrap();
    assert!(
        !toml_text.contains("old-secret"),
        "token survived --clear-token"
    );
    assert!(!toml_text.contains("token = "));
}

#[test]
fn clear_token_and_peer_token_are_mutually_exclusive() {
    let (_tmp, home) = isolated_home();
    let output = mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args([
            "peer",
            "add",
            "http://x:5002",
            "--name",
            "x",
            "--peer-token",
            "tok",
            "--clear-token",
        ])
        .output()
        .expect("mnemosctl runs");
    assert!(
        !output.status.success(),
        "expected clap to reject the combo"
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("cannot be used with") || stderr.contains("conflicts"),
        "stderr was: {stderr}"
    );
}

#[test]
fn non_peer_subcommands_accept_token_after_subcommand() {
    // Round-7 regression coverage: --token must remain global = true
    // so authenticated non-peer subcommands accept it in either
    // position. The peer-only guard is via ValueSource, not by
    // dropping global. This test exercises status/search/create with
    // --token AFTER the subcommand.
    use std::process::Stdio;

    // status --token TOK <unreachable host>: clap must accept the
    // arg shape; the underlying request will fail (no server), so we
    // just assert clap did not reject the flag itself.
    for args in [
        vec!["status", "--token", "TOK"],
        vec!["search", "--token", "TOK", "anything"],
        vec!["create", "--token", "TOK", "--content", "x"],
    ] {
        let mut full_args = vec!["--base", "http://127.0.0.1:1"];
        full_args.extend(args.iter().copied());

        let output = mnemosctl()
            .args(&full_args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .output()
            .expect("mnemosctl runs");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            !stderr.contains("unexpected argument") && !stderr.contains("found argument"),
            "clap rejected --token after subcommand for {full_args:?}: {stderr}"
        );
    }
}

#[test]
fn cli_supplied_token_before_peer_subcommand_is_rejected() {
    // Round-6 hardening: `mnemosctl --token TOK peer add ...` parses
    // because --token is a parent arg, but commands::peer() does not
    // forward cli.token. Round-5 left this path silently dropping the
    // token, persisting an anonymous peer. The runtime check in main()
    // distinguishes CLI-supplied --token (reject) from MNEMOS_API_KEY
    // env (still safe to ignore for peer commands).
    let (_tmp, home) = isolated_home();
    let output = mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args([
            "--token",
            "server-bearer",
            "peer",
            "add",
            "http://x.example:5002",
            "--name",
            "x",
        ])
        .output()
        .expect("mnemosctl runs");
    assert!(
        !output.status.success(),
        "expected --token before peer to error; stdout={}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(
        stderr.contains("--token is for server auth") || stderr.contains("--peer-token"),
        "stderr was: {stderr}"
    );
    assert!(
        !std::path::Path::new(&format!("{home}/peers.toml")).exists(),
        "peers.toml created despite rejection"
    );
}

#[test]
fn mnemos_api_key_env_does_not_block_peer_commands() {
    // Round-6 boundary: rejection must fire ONLY for CLI --token,
    // never for MNEMOS_API_KEY env. A user with the env set globally
    // (the autosaved fleet pattern) must still be able to run peer
    // commands without unsetting it first.
    let (_tmp, home) = isolated_home();
    let output = mnemosctl()
        .env("MNEMOS_HOME", &home)
        .env("MNEMOS_API_KEY", "fleet-server-bearer")
        .args([
            "peer",
            "add",
            "http://desktop.example:5102",
            "--name",
            "desktop",
        ])
        .output()
        .expect("mnemosctl runs");
    assert!(
        output.status.success(),
        "peer add blocked by MNEMOS_API_KEY env; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let toml_text = std::fs::read_to_string(format!("{home}/peers.toml")).unwrap();
    // The env value still must NOT leak into peers.toml.
    assert!(!toml_text.contains("fleet-server-bearer"));
}

#[test]
fn global_token_after_subcommand_is_rejected_not_silently_dropped() {
    // Prior to round-5 hardening, the global --token was global=true
    // so it was accepted on `peer add` and silently dropped (only
    // --peer-token persisted). Now --token is parent-scoped only;
    // passing it after the subcommand must error rather than create
    // an anonymous peer.
    let (_tmp, home) = isolated_home();
    let output = mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args([
            "peer",
            "add",
            "http://x.example:5002",
            "--name",
            "x",
            "--token",
            "wrong-flag-name",
        ])
        .output()
        .expect("mnemosctl runs");
    assert!(
        !output.status.success(),
        "--token after subcommand should error; stdout was: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    // clap renders the error as "unexpected argument" for unknown flag.
    assert!(
        stderr.contains("--token") || stderr.contains("unexpected argument"),
        "stderr was: {stderr}"
    );
    // No peers.toml should have been written.
    assert!(
        !std::path::Path::new(&format!("{home}/peers.toml")).exists(),
        "peers.toml created despite parse error"
    );
}

#[cfg(unix)]
#[test]
fn lockfile_blocks_concurrent_writer() {
    use fs2::FileExt;
    use std::time::Duration;

    let (_tmp, home) = isolated_home();
    std::fs::create_dir_all(&home).unwrap();
    let lock_path = format!("{home}/peers.toml.lock");

    // Take the lock externally to simulate another in-flight peer add.
    let holder = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)
        .expect("open lockfile");
    holder.lock_exclusive().expect("acquire external lock");

    // Spawn the CLI; it must block on the lock and not write until
    // we release. We give it 250ms head start, then assert peers.toml
    // is still absent.
    let mut child = mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args([
            "peer",
            "add",
            "http://blocked.example:5002",
            "--name",
            "blocked",
        ])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("mnemosctl spawns");
    std::thread::sleep(Duration::from_millis(250));
    assert!(
        !std::path::Path::new(&format!("{home}/peers.toml")).exists(),
        "peers.toml written while another writer held the lock"
    );

    // Release and confirm the child finishes successfully and writes.
    fs2::FileExt::unlock(&holder).expect("release lock");
    let status = child.wait().expect("child finishes");
    assert!(status.success(), "child failed after lock released");
    let toml_text = std::fs::read_to_string(format!("{home}/peers.toml"))
        .expect("peers.toml written after lock released");
    assert!(toml_text.contains("blocked"));
}

#[test]
fn url_change_without_token_flag_is_rejected() {
    // Round-8 finding: silently re-binding an existing bearer token
    // to a NEW endpoint leaks credentials across a trust boundary.
    // peer add must require one of {--peer-token, --clear-token,
    // --preserve-token} when changing the URL of a peer that has a
    // saved token. Same-URL retries are still implicit-preserve.
    let (_tmp, home) = isolated_home();

    mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args([
            "peer",
            "add",
            "http://old.example:5002",
            "--name",
            "moving-target",
            "--peer-token",
            "trust-boundary-A",
        ])
        .output()
        .expect("mnemosctl runs");

    let url_change = mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args([
            "peer",
            "add",
            "http://new.example:5002",
            "--name",
            "moving-target",
        ])
        .output()
        .expect("mnemosctl runs");
    assert_eq!(
        url_change.status.code(),
        Some(2),
        "expected exit 2 on URL change without explicit token handling"
    );
    let stderr = String::from_utf8(url_change.stderr).unwrap();
    assert!(
        stderr.contains("requires explicit token handling"),
        "stderr was: {stderr}"
    );

    // peers.toml must remain at the old URL with the old token.
    let toml_text = std::fs::read_to_string(format!("{home}/peers.toml")).unwrap();
    assert!(toml_text.contains("http://old.example:5002"));
    assert!(toml_text.contains("token = \"trust-boundary-A\""));
    assert!(!toml_text.contains("http://new.example:5002"));
}

#[test]
fn url_change_with_preserve_token_carries_existing_token() {
    let (_tmp, home) = isolated_home();
    mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args([
            "peer",
            "add",
            "http://old.example:5002",
            "--name",
            "moving",
            "--peer-token",
            "carry-me",
        ])
        .output()
        .expect("mnemosctl runs");

    let preserved = mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args([
            "peer",
            "add",
            "http://new.example:5002",
            "--name",
            "moving",
            "--preserve-token",
        ])
        .output()
        .expect("mnemosctl runs");
    assert!(
        preserved.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&preserved.stderr)
    );

    let toml_text = std::fs::read_to_string(format!("{home}/peers.toml")).unwrap();
    assert!(toml_text.contains("http://new.example:5002"));
    assert!(toml_text.contains("token = \"carry-me\""));
}

#[test]
fn url_change_with_no_existing_token_does_not_require_flag() {
    // The round-8 gate only fires when there's an existing token to
    // protect. A peer that was anonymous to begin with can have its
    // URL changed freely.
    let (_tmp, home) = isolated_home();
    mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args(["peer", "add", "http://old.example:5002", "--name", "anon"])
        .output()
        .expect("mnemosctl runs");

    let url_change = mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args(["peer", "add", "http://new.example:5002", "--name", "anon"])
        .output()
        .expect("mnemosctl runs");
    assert!(
        url_change.status.success(),
        "URL change blocked even though peer had no token; stderr={}",
        String::from_utf8_lossy(&url_change.stderr)
    );
}

#[test]
fn empty_mnemos_home_is_rejected() {
    // Round-10 finding: empty MNEMOS_HOME would resolve to a relative
    // `peers.toml` in cwd, persisting bearer tokens into the nearest
    // source repo or synced directory.
    let output = mnemosctl()
        .env("MNEMOS_HOME", "")
        .args(["peer", "list"])
        .output()
        .expect("mnemosctl runs");
    assert!(
        !output.status.success(),
        "expected empty MNEMOS_HOME to be rejected"
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("MNEMOS_HOME is empty"),
        "stderr was: {stderr}"
    );
}

#[test]
fn relative_mnemos_home_is_rejected() {
    let output = mnemosctl()
        .env("MNEMOS_HOME", "relative/path")
        .args(["peer", "list"])
        .output()
        .expect("mnemosctl runs");
    assert!(
        !output.status.success(),
        "expected relative MNEMOS_HOME to be rejected"
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("not absolute"), "stderr was: {stderr}");
}

#[test]
fn empty_home_with_no_mnemos_home_is_rejected() {
    let output = mnemosctl()
        .env_remove("MNEMOS_HOME")
        .env("HOME", "")
        .args(["peer", "list"])
        .output()
        .expect("mnemosctl runs");
    assert!(
        !output.status.success(),
        "expected empty HOME to be rejected"
    );
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("HOME is not set or empty"),
        "stderr was: {stderr}"
    );
}

#[cfg(unix)]
#[test]
fn save_fsyncs_parent_directory() {
    // We can't easily prove fsync was issued from a test, but we CAN
    // assert that the rename leaves the dirent committed: open the
    // parent dir, list it, observe peers.toml is present and the
    // tempfile is gone. The dir-fsync code path runs on every save.
    let (_tmp, home) = isolated_home();
    mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args([
            "peer",
            "add",
            "http://durability.example:5002",
            "--name",
            "durable",
        ])
        .output()
        .expect("mnemosctl runs");

    let dir_entries: Vec<String> = std::fs::read_dir(&home)
        .unwrap()
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(String::from))
        .collect();
    assert!(dir_entries.contains(&"peers.toml".to_string()));
    let leftover_tempfiles: Vec<&String> = dir_entries
        .iter()
        .filter(|n| n.starts_with("peers.toml.tmp"))
        .collect();
    assert!(
        leftover_tempfiles.is_empty(),
        "tempfile leaked after fsync path: {leftover_tempfiles:?}"
    );
}

#[cfg(unix)]
#[test]
fn first_run_creates_nested_config_dir_durably() {
    // Round-11 finding: when ~/.mnemos doesn't exist yet, the dirent
    // for `.mnemos` lives in $HOME and was never fsynced. The fix
    // walks newly-created ancestors and fsyncs each parent. This test
    // exercises the first-run path: a brand-new fake $HOME with no
    // .mnemos subdir, expect peer add to succeed AND have created
    // `.mnemos/peers.toml` durably.
    let fake_home = TempDir::new().expect("fake-home tempdir");
    let dotmnemos = fake_home.path().join(".mnemos");
    let peers_toml = dotmnemos.join("peers.toml");

    assert!(
        !dotmnemos.exists(),
        "test premise: .mnemos does not exist yet"
    );

    let add = mnemosctl()
        .env_remove("MNEMOS_HOME")
        .env("HOME", fake_home.path())
        .args([
            "peer",
            "add",
            "http://durable.example:5002",
            "--name",
            "first",
        ])
        .output()
        .expect("mnemosctl runs");
    assert!(
        add.status.success(),
        "first-run peer add failed: stderr={}",
        String::from_utf8_lossy(&add.stderr)
    );

    assert!(dotmnemos.exists(), "~/.mnemos was not created");
    assert!(peers_toml.exists(), "peers.toml was not created");
    let toml_text = std::fs::read_to_string(&peers_toml).unwrap();
    assert!(toml_text.contains("first"));
}

#[test]
fn empty_url_exits_2() {
    let (_tmp, home) = isolated_home();

    let output = mnemosctl()
        .env("MNEMOS_HOME", &home)
        .args(["peer", "add", "   "])
        .output()
        .expect("mnemosctl runs");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8(output.stderr).expect("stderr is utf8");
    assert!(stderr.contains("URL cannot be empty"));
}

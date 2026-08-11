use std::process::Command;

fn mnemosctl() -> Command {
    Command::new(env!("CARGO_BIN_EXE_mnemos-desktop"))
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

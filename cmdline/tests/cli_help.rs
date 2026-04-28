use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn help_flag_prints_usage() {
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg("--help")
        .assert()
        .success()
        .stdout(contains("Usage: pandora"));
}

#[test]
fn help_command_prints_usage() {
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg("help")
        .assert()
        .success()
        .stdout(contains("Usage: pandora"));
}

use assert_cmd::Command;
use predicates::str::contains;
use tempfile::NamedTempFile;

#[test]
fn rejects_missing_mode_with_exit_2() {
    let file = NamedTempFile::new().expect("temp file");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .assert()
        .code(2)
        .stderr(contains("use one mode"));
}

#[test]
fn rejects_both_modes_as_usage_error() {
    let file = NamedTempFile::new().expect("temp file");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--lexeme")
        .arg("--ast")
        .assert()
        .code(2);
}

#[test]
fn ast_mode_prints_ast_root() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(&mut file, b"a := 1 + 2").expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--ast")
        .assert()
        .success()
        .stdout(contains("LetDecl"));
}

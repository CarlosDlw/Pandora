use assert_cmd::Command;
use predicates::str::contains;
use tempfile::NamedTempFile;

#[test]
fn lexeme_mode_returns_error_for_lexer_diagnostic() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(&mut file, b"a := @").expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--lexeme")
        .assert()
        .code(1)
        .stderr(contains("invalid character"));
}

#[test]
fn ast_mode_includes_lexer_diagnostics_in_exit_status() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(&mut file, b"a := @").expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--ast")
        .assert()
        .code(1)
        .stderr(contains("invalid character"));
}

#[test]
fn ast_mode_returns_error_for_parser_diagnostic() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(&mut file, b"a := (1 +").expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--ast")
        .assert()
        .code(1)
        .stderr(contains("expected ')'"));
}

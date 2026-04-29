use assert_cmd::Command;
use predicates::boolean::PredicateBooleanExt;
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
        .stderr(
            contains("error: invalid character")
                .and(contains("-->"))
                .and(contains("| a := @"))
                .and(contains("[5..6]")),
        );
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
        .stderr(
            contains("error: invalid character")
                .and(contains("-->"))
                .and(contains("| a := @")),
        );
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
        .stderr(
            contains("error: expected ')'")
                .and(contains("-->"))
                .and(contains("= help:")),
        );
}

#[test]
fn check_mode_reports_block_scope_violation() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(&mut file, b"{ name: str = \"carlos\" }\nprint(name)\n").expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--check")
        .assert()
        .code(1)
        .stderr(
            contains("error: undefined symbol 'name'")
                .and(contains("symbols from a block are not visible outside it")),
        );
}

#[test]
fn ast_mode_reports_missing_block_closer() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(&mut file, b"{ value := 1\n").expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--ast")
        .assert()
        .code(1)
        .stderr(
            contains("error: expected '}'")
                .and(contains("close the block with '}'")),
        );
}

#[test]
fn lexeme_mode_reports_invalid_numeric_literal_with_hint() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(&mut file, b"x := 0b102\n").expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--lexeme")
        .assert()
        .code(1)
        .stderr(
            contains("error: invalid numeric literal")
                .and(contains("check base prefixes (0b/0o/0x)")),
        );
}

#[test]
fn check_mode_reports_invalid_if_condition() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(&mut file, b"if print(1) { x := 1 }\n").expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--check")
        .assert()
        .code(1)
        .stderr(
            contains("error: if condition is not truthy/falsy-compatible")
                .and(contains("truthy/falsy-compatible value")),
        );
}

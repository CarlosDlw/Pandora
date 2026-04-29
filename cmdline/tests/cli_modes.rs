use std::path::PathBuf;

use assert_cmd::Command;
use predicates::boolean::PredicateBooleanExt;
use predicates::str::contains;
use tempfile::NamedTempFile;

#[test]
fn default_mode_executes_when_no_dump_flags_are_set() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(&mut file, b"a := 1;").expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .assert()
        .success();
}

#[test]
fn runs_example_001_simple() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/001_simple.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("20")
                .and(contains("John"))
                .and(contains("true"))
                .and(contains("3.14159"))
                .and(contains("John 20")),
        );
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

#[test]
fn runs_example_003_operators_and_literals() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/003_operators_and_numeric_literals.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("1000 10 15 255")
                .and(contains("3.14159 6020"))
                .and(contains("true true true true true")),
        );
}

#[test]
fn runs_example_004_if_else() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/004_if_else.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("non-zero is truthy")
                .and(contains("false branch"))
                .and(contains("else-if branch")),
        );
}

#[test]
fn runs_example_005_while_break_continue() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/005_while_break_continue.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("sum of odd numbers below 8: 16")
                .and(contains("done")),
        );
}

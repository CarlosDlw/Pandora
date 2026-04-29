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

#[test]
fn check_mode_reports_break_outside_loop() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(&mut file, b"break\n").expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--check")
        .assert()
        .code(1)
        .stderr(
            contains("error: break used outside of loop")
                .and(contains("use `break` only inside a loop body")),
        );
}

#[test]
fn check_mode_reports_continue_outside_loop() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(&mut file, b"continue\n").expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--check")
        .assert()
        .code(1)
        .stderr(
            contains("error: continue used outside of loop")
                .and(contains("use `continue` only inside a loop body")),
        );
}

#[test]
fn check_mode_reports_invalid_for_init() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(&mut file, b"for i := 0; i < 3; i++ { }\n").expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--check")
        .assert()
        .code(1)
        .stderr(contains("error: for init must use typed declaration"));
}

#[test]
fn check_mode_reports_return_outside_function() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(&mut file, b"return 1\n").expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--check")
        .assert()
        .code(1)
        .stderr(contains("error: return used outside of function"));
}

#[test]
fn check_mode_reports_return_without_value_in_non_unit_fn() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(&mut file, b"fn bad() -> i32 { return }\n").expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--check")
        .assert()
        .code(1)
        .stderr(contains("error: return without value requires unit return type"));
}

#[test]
fn check_mode_reports_tuple_index_out_of_range() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(&mut file, b"t: (i32, i32) = (1, 2)\nprint(t.2)\n").expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--check")
        .assert()
        .code(1)
        .stderr(contains("error: tuple index 2 out of range"));
}

#[test]
fn check_mode_reports_tuple_destructure_arity_mismatch() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(&mut file, b"t: (i32, i32) = (1, 2);\n(a, b, c) := t\n").expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--check")
        .assert()
        .code(1)
        .stderr(contains("error: tuple destructuring arity mismatch"));
}

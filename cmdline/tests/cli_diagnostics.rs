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

#[test]
fn check_mode_reports_struct_literal_missing_field() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(&mut file, b"struct Point { x: i32, y: i32 }\np: Point = Point { x: 1 }\n")
        .expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--check")
        .assert()
        .code(1)
        .stderr(contains("error: missing field 'y' in struct literal 'Point'"));
}

#[test]
fn check_mode_reports_incomplete_trait_impl() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(
        &mut file,
        b"struct Point { x: i32 }\ntrait Show { fn show(self) -> str }\nimpl Show for Point {}\n",
    )
    .expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--check")
        .assert()
        .code(1)
        .stderr(contains("error: trait impl missing required method 'show'"));
}

#[test]
fn check_mode_reports_multi_return_on_non_tuple_fn() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(&mut file, b"fn bad(a: i32, b: i32) -> i32 { return a, b }\n").expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--check")
        .assert()
        .code(1)
        .stderr(contains("error: multiple return values are allowed only for functions returning tuple"));
}

#[test]
fn check_mode_reports_invalid_array_index_type() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(&mut file, b"arr: [i32] = [1,2,3]\nprint(arr[true])\n").expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--check")
        .assert()
        .code(1)
        .stderr(contains("error: array index must be integer"));
}

#[test]
fn check_mode_reports_tuple_fn_returning_single_tuple_symbol() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(
        &mut file,
        b"fn bad(p: (i32, i32)) -> (i32, i32) { return p }\n",
    )
    .expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--check")
        .assert()
        .code(1)
        .stderr(contains("error: tuple return must use explicit positional values"));
}

#[test]
fn check_mode_reports_tuple_return_arity_mismatch() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(
        &mut file,
        b"fn bad(a: i32) -> (i32, bool) { return a, true, null }\n",
    )
    .expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--check")
        .assert()
        .code(1)
        .stderr(contains("error: tuple return arity mismatch"));
}

#[test]
fn check_mode_reports_error_builtin_invalid_args() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(&mut file, b"a := error(1); b := error(\"x\", true)\n").expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--check")
        .assert()
        .code(1)
        .stderr(contains("error message must be str").and(contains("error code must be i32")));
}

#[test]
fn check_mode_reports_question_outside_fallible_function() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(
        &mut file,
        b"fn divide(a: i32, b: i32) -> (i32, err) { return a / b, null }\nfn bad() -> i32 { return divide(4, 2)? }\n",
    )
    .expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--check")
        .assert()
        .code(1)
        .stderr(
            contains("operator '?' requires current function return type to be (T, err)")
                .and(contains("change the function return type to `(T, err)` when using '?'")),
        );
}

#[test]
fn check_mode_reports_try_catch_binding_non_err_type() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(
        &mut file,
        b"fn divide(a: i32, b: i32) -> (i32, err) { return a / b, null }\nx := try divide(1, 1) catch(e: i32) { return 0 }\n",
    )
    .expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--check")
        .assert()
        .code(1)
        .stderr(
            contains("catch binding type must be err-like")
                .and(contains("declare catch binding as `catch(e: err)` or an error-like type with message/code")),
        );
}

#[test]
fn check_mode_reports_catch_without_required_return_value() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(
        &mut file,
        b"fn divide(a: i32, b: i32) -> (i32, err) { return a / b, null }\nx := try divide(1, 1) catch(e: err) { print(e.message) }\n",
    )
    .expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .arg("--check")
        .assert()
        .code(1)
        .stderr(
            contains("catch block must end with `return <value>`")
                .and(contains("finish catch blocks with `return value`")),
        );
}

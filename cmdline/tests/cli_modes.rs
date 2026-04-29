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
fn executes_recursive_function_calls() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(
        &mut file,
        br#"fn fibbonacci(n: i32) -> i32 {
    if n <= 1 {
        return n
    }
    return fibbonacci(n - 1) + fibbonacci(n - 2)
}
print(fibbonacci(10))
"#,
    )
    .expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .assert()
        .success()
        .stdout(contains("55"));
}

#[test]
fn executes_mutual_recursive_function_calls() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(
        &mut file,
        br#"fn is_even(n: i32) -> bool {
    if n == 0 {
        return true
    }
    return is_odd(n - 1)
}
fn is_odd(n: i32) -> bool {
    if n == 0 {
        return false
    }
    return is_even(n - 1)
}
print(is_even(10), is_odd(7))
"#,
    )
    .expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .assert()
        .success()
        .stdout(contains("true true"));
}

#[test]
fn allows_null_literal_assignment_to_typed_values() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(
        &mut file,
        b"x: i32 = null; y: bool = null; print(x, y)\n",
    )
    .expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .assert()
        .success()
        .stdout(contains("null null"));
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

#[test]
fn runs_example_006_compound_assign_and_string_concat() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/006_compound_assign_and_string_concat.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("0")
                .and(contains("result: 0"))
                .and(contains("hello world"))
                .and(contains("value is 42")),
        );
}

#[test]
fn runs_example_007_for_and_inc_dec() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/007_for_and_inc_dec.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("sum: 13")
                .and(contains("2 2 2 2 1"))
                .and(contains("loop 0"))
                .and(contains("loop 1"))
                .and(contains("loop 2")),
        );
}

#[test]
fn runs_example_008_functions_and_return() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/008_functions_and_return.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("sum: 5")
                .and(contains("adder: 15"))
                .and(contains("log: 7")),
        );
}

#[test]
fn runs_example_009_function_values() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/009_function_values.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(contains("apply: 42"));
}

#[test]
fn runs_example_010_nested_capture() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/010_nested_capture.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(contains("mul3: 21"));
}

#[test]
fn runs_example_011_unit_return() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/011_unit_return.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("first 5")
                .and(contains("second 5"))
                .and(contains("done")),
        );
}

#[test]
fn runs_example_012_tuples_basics() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/012_tuples_basics.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("10 ok")
                .and(contains("true"))
                .and(contains("true")),
        );
}

#[test]
fn runs_example_013_tuples_destructuring() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/013_tuples_destructuring.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(contains("4 9 4 9"));
}

#[test]
fn runs_example_014_tuples_nested() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/014_tuples_nested.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("2 a")
                .and(contains("true"))
                .and(contains("true")),
        );
}

#[test]
fn runs_example_015_structs_basics() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/015_structs_basics.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(contains("3 4").and(contains("7")).and(contains("0 0")));
}

#[test]
fn runs_example_016_traits_impls() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/016_traits_impls.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(contains("counter").and(contains("12")));
}

#[test]
fn runs_example_017_structs_with_existing_features() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/017_structs_with_existing_features.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(contains("10 3 null"));
}

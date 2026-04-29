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

#[test]
fn runs_example_018_tuple_return_values() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/018_tuple_return_values.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("7 null")
                .and(contains("null"))
                .and(contains("null true")),
        );
}

#[test]
fn runs_example_019_err_error_usage() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/019_err_error_usage.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("err(message=\"test\", code=1")
                .and(contains("origin=\"error\""))
                .and(contains("test"))
                .and(contains("1"))
                .and(contains("division by zero"))
                .and(contains("null")),
        );
}

#[test]
fn runs_example_020_panic_runtime_error() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/020_panic_runtime_error.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .code(1)
        .stderr(contains("panic: unrecoverable").and(contains("code=42")));
}

#[test]
fn runs_example_021_try_catch_recover() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/021_try_catch_recover.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("caught panic panic from risky 7")
                .and(contains("caught err regular error 9"))
                .and(contains("42 99")),
        );
}

#[test]
fn runs_example_022_question_propagation() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/022_question_propagation.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(contains("propagated division by zero 10"));
}

#[test]
fn runs_example_023_domain_error_struct() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/023_domain_error_struct.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(contains("payment failed 402 limit exceeded").and(contains("0")));
}

#[test]
fn runs_example_024_error_context_chain() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/024_error_context_chain.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("division by zero")
                .and(contains("propagate"))
                .and(contains("division by zero")),
        );
}

#[test]
fn runs_example_025_arrays_basic_get_set() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/025_arrays_basic_get_set.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("[1, 2, 3]")
                .and(contains("1 2 3"))
                .and(contains("[1, 42, 3]"))
                .and(contains("3")),
        );
}

#[test]
fn runs_example_026_arrays_nested_len() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/026_arrays_nested_len.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("[[1, 2], [3, 4], [5, 6]]")
                .and(contains("[1, 2]"))
                .and(contains("6"))
                .and(contains("3"))
                .and(contains("2")),
        );
}

#[test]
fn runtime_reports_array_bounds_error() {
    let mut file = NamedTempFile::new().expect("temp file");
    std::io::Write::write_all(&mut file, b"arr: [i32] = [1, 2]\nprint(arr[5])\n").expect("write");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(file.path())
        .assert()
        .code(1)
        .stderr(contains("index out of bounds: index=5, len=2"));
}

#[test]
fn runs_example_027_array_spread() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/027_array_spread.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(contains("[1, 2, 3, 4]"));
}

#[test]
fn runs_example_028_optional_params_typeof() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/028_optional_params_typeof.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("hello pandora!")
                .and(contains("hello dev!!"))
                .and(contains("i128"))
                .and(contains("[i128]")),
        );
}

#[test]
fn runs_example_029_range_basics() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/029_range_basics.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(contains("[0, 1, 2, 3, 4]").and(contains("[0, 1, 2, 3, 4, 5]")));
}

#[test]
fn runs_example_030_for_in_arrays() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/030_for_in_arrays.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("10")
                .and(contains("20"))
                .and(contains("30"))
                .and(contains("0"))
                .and(contains("1"))
                .and(contains("2"))
                .and(contains("3")),
        );
}

#[test]
fn runs_example_031_integer_methods_basics() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/031_integer_methods_basics.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("13 7 30 3 1")
                .and(contains("false true 1"))
                .and(contains("false true false"))
                .and(contains("-10 10")),
        );
}

#[test]
fn runs_example_032_integer_methods_checked_and_convert() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../examples/032_integer_methods_checked_and_convert.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("25 15")
                .and(contains("25 null"))
                .and(contains("-7 true -7"))
                .and(contains("20 20 20 20")),
        );
}

#[test]
fn runs_example_033_float_methods_math_trig() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/033_float_methods_math_trig.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("6.5 2.5 9 2.25 0.5 -4.5")
                .and(contains("false false true 1"))
                .and(contains("4 5 5 4 0.5")),
        );
}

#[test]
fn runs_example_034_float_methods_state_convert() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/034_float_methods_state_convert.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("false false true")
                .and(contains("true true false"))
                .and(contains("3 3.75")),
        );
}

#[test]
fn runs_example_035_bool_methods() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/035_bool_methods.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("false true true false false")
                .and(contains("1 0 true false")),
        );
}

#[test]
fn runs_example_036_char_methods() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/036_char_methods.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("true false true true false")
                .and(contains("true false true"))
                .and(contains("true"))
                .and(contains("A a 97 a")),
        );
}

#[test]
fn runs_example_037_str_methods_basics() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/037_str_methods_basics.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("15 false (H, null)")
                .and(contains("true true true"))
                .and(contains("8 11"))
                .and(contains("Hello"))
                .and(contains("HELLO,WORLD"))
                .and(contains("  dlroW,olleH  ")),
        );
}

#[test]
fn runs_example_038_str_methods_convert() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/038_str_methods_convert.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("(123, null)")
                .and(contains("(3.14, null)"))
                .and(contains("(0, err(")),
        );
}

#[test]
fn runs_example_039_array_methods_access_search_utils() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../examples/039_array_methods_access_search_utils.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("4 false (2, null)")
                .and(contains("(null, err("))
                .and(contains("1 false true"))
                .and(contains("[1, 2, 1, 3]"))
                .and(contains("[1, 1, 2, 3]"))
                .and(contains("[1, 2]")),
        );
}

#[test]
fn runs_example_040_array_methods_modify() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../examples/040_array_methods_modify.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("30")
                .and(contains("(20, null)"))
                .and(contains("(null, err(")),
        );
}

#[test]
fn runs_example_041_function_methods() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/041_function_methods.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("2")
                .and(contains("5"))
                .and(contains("<fn>")),
        );
}

#[test]
fn runs_example_042_map_methods() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/042_map_methods.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("2 false 1 null")
                .and(contains("9 9 true"))
                .and(contains("1 null 2"))
                .and(contains("true true")),
        );
}

#[test]
fn runs_example_043_set_methods() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../examples/043_set_methods.pand");
    Command::cargo_bin("pandora")
        .expect("binary")
        .arg(&path)
        .assert()
        .success()
        .stdout(
            contains("3 false true")
                .and(contains("false true true false"))
                .and(contains("true false")),
        );
}

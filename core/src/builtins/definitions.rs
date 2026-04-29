use crate::analyzer::Type;

use super::registry::{BuiltinFunction, BuiltinMethod, BuiltinMethodKind, BuiltinRegistry, ReceiverMatcher};

pub fn default_registry() -> BuiltinRegistry {
    let mut functions = vec![BuiltinFunction {
        name: "print",
        ty: Type::Function {
            params: vec![Type::Any],
            ret: Box::new(Type::Unit),
        },
    },
    BuiltinFunction {
        name: "len",
        ty: Type::Function {
            params: vec![Type::Any],
            ret: Box::new(Type::Int {
                signed: false,
                bits: 64,
            }),
        },
    },
    BuiltinFunction {
        name: "error",
        ty: Type::Function {
            // Contract enforced in checker/runtime: (str) or (str, i32)
            params: vec![Type::Any],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "panic",
        ty: Type::Function {
            // Contract enforced in checker/runtime: (str) or (str, i32), runtime aborts.
            params: vec![Type::Any],
            ret: Box::new(Type::Unit),
        },
    },
    BuiltinFunction {
        name: "wrap",
        ty: Type::Function {
            // Contract enforced in checker/runtime: (err-like, str) or (err-like, str, i32)
            params: vec![Type::Any],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "typeof",
        ty: Type::Function {
            params: vec![Type::Any],
            ret: Box::new(Type::Str),
        },
    },
    BuiltinFunction {
        name: "helper",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Str),
        },
    },
    BuiltinFunction {
        name: "assert",
        ty: Type::Function {
            params: vec![Type::Bool, Type::Str],
            ret: Box::new(Type::Unit),
        },
    },
    BuiltinFunction {
        name: "assert_eq_i32",
        ty: Type::Function {
            params: vec![
                Type::Int {
                    signed: true,
                    bits: 32,
                },
                Type::Int {
                    signed: true,
                    bits: 32,
                },
                Type::Str,
            ],
            ret: Box::new(Type::Unit),
        },
    },
    BuiltinFunction {
        name: "assert_eq_str",
        ty: Type::Function {
            params: vec![Type::Str, Type::Str, Type::Str],
            ret: Box::new(Type::Unit),
        },
    },
    BuiltinFunction {
        name: "sum_i32",
        ty: Type::Function {
            params: vec![Type::Array(Box::new(Type::Int {
                signed: true,
                bits: 32,
            }))],
            ret: Box::new(Type::Int {
                signed: true,
                bits: 32,
            }),
        },
    },
    BuiltinFunction {
        name: "str_repeat",
        ty: Type::Function {
            params: vec![
                Type::Str,
                Type::Int {
                    signed: true,
                    bits: 32,
                },
            ],
            ret: Box::new(Type::Str),
        },
    },
    BuiltinFunction {
        name: "str_pad_left",
        ty: Type::Function {
            params: vec![
                Type::Str,
                Type::Int {
                    signed: true,
                    bits: 32,
                },
                Type::Str,
            ],
            ret: Box::new(Type::Str),
        },
    },
    BuiltinFunction {
        name: "str_pad_right",
        ty: Type::Function {
            params: vec![
                Type::Str,
                Type::Int {
                    signed: true,
                    bits: 32,
                },
                Type::Str,
            ],
            ret: Box::new(Type::Str),
        },
    },
    BuiltinFunction {
        name: "count_true",
        ty: Type::Function {
            params: vec![Type::Array(Box::new(Type::Bool))],
            ret: Box::new(Type::Int {
                signed: true,
                bits: 32,
            }),
        },
    },
    BuiltinFunction {
        name: "io_read_text",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Tuple(vec![Type::Str, Type::Err])),
        },
    },
    BuiltinFunction {
        name: "read_text",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Tuple(vec![Type::Str, Type::Err])),
        },
    },
    BuiltinFunction {
        name: "io_write_text",
        ty: Type::Function {
            params: vec![Type::Str, Type::Str],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "write_text",
        ty: Type::Function {
            params: vec![Type::Str, Type::Str],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "io_append_text",
        ty: Type::Function {
            params: vec![Type::Str, Type::Str],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "append_text",
        ty: Type::Function {
            params: vec![Type::Str, Type::Str],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "io_exists",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Bool),
        },
    },
    BuiltinFunction {
        name: "exists",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Bool),
        },
    },
    BuiltinFunction {
        name: "io_remove_file",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "remove_file",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "io_read_stdin_line",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Tuple(vec![Type::Str, Type::Err])),
        },
    },
    BuiltinFunction {
        name: "stdin_read_line",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Tuple(vec![Type::Str, Type::Err])),
        },
    },
    BuiltinFunction {
        name: "io_stdout_write",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Unit),
        },
    },
    BuiltinFunction {
        name: "stdout_write",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Unit),
        },
    },
    BuiltinFunction {
        name: "stdout_writeln",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Unit),
        },
    },
    BuiltinFunction {
        name: "io_stderr_write",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Unit),
        },
    },
    BuiltinFunction {
        name: "stderr_write",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Unit),
        },
    },
    BuiltinFunction {
        name: "stderr_writeln",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Unit),
        },
    },
    BuiltinFunction {
        name: "buffer_new",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Str),
        },
    },
    BuiltinFunction {
        name: "buffer_write",
        ty: Type::Function {
            params: vec![Type::Str, Type::Str],
            ret: Box::new(Type::Str),
        },
    },
    BuiltinFunction {
        name: "buffer_writeln",
        ty: Type::Function {
            params: vec![Type::Str, Type::Str],
            ret: Box::new(Type::Str),
        },
    },
    BuiltinFunction {
        name: "buffer_clear",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Str),
        },
    },
    BuiltinFunction {
        name: "fs_exists",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Bool),
        },
    },
    BuiltinFunction {
        name: "fs_is_file",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Bool),
        },
    },
    BuiltinFunction {
        name: "is_file",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Bool),
        },
    },
    BuiltinFunction {
        name: "fs_is_dir",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Bool),
        },
    },
    BuiltinFunction {
        name: "is_dir",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Bool),
        },
    },
    BuiltinFunction {
        name: "fs_create_dir",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "create_dir",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "fs_create_dir_all",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "create_dir_all",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "fs_read_dir",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Tuple(vec![Type::Array(Box::new(Type::Str)), Type::Err])),
        },
    },
    BuiltinFunction {
        name: "read_dir",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Tuple(vec![Type::Array(Box::new(Type::Str)), Type::Err])),
        },
    },
    BuiltinFunction {
        name: "fs_remove_file",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "fs_remove_dir",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "remove_dir",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "fs_remove_dir_all",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "remove_dir_all",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "fs_rename",
        ty: Type::Function {
            params: vec![Type::Str, Type::Str],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "rename",
        ty: Type::Function {
            params: vec![Type::Str, Type::Str],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "fs_copy",
        ty: Type::Function {
            params: vec![Type::Str, Type::Str],
            ret: Box::new(Type::Tuple(vec![Type::Int { signed: false, bits: 64 }, Type::Err])),
        },
    },
    BuiltinFunction {
        name: "copy",
        ty: Type::Function {
            params: vec![Type::Str, Type::Str],
            ret: Box::new(Type::Tuple(vec![Type::Int { signed: false, bits: 64 }, Type::Err])),
        },
    },
    BuiltinFunction {
        name: "fs_cwd",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Tuple(vec![Type::Str, Type::Err])),
        },
    },
    BuiltinFunction {
        name: "cwd",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Tuple(vec![Type::Str, Type::Err])),
        },
    },
    BuiltinFunction {
        name: "fs_set_cwd",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "set_cwd",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "fs_path_join",
        ty: Type::Function {
            params: vec![Type::Str, Type::Str],
            ret: Box::new(Type::Str),
        },
    },
    BuiltinFunction {
        name: "path_join",
        ty: Type::Function {
            params: vec![Type::Str, Type::Str],
            ret: Box::new(Type::Str),
        },
    },
    BuiltinFunction {
        name: "fs_path_parent",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Tuple(vec![Type::Str, Type::Err])),
        },
    },
    BuiltinFunction {
        name: "path_parent",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Tuple(vec![Type::Str, Type::Err])),
        },
    },
    BuiltinFunction {
        name: "fs_path_filename",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Tuple(vec![Type::Str, Type::Err])),
        },
    },
    BuiltinFunction {
        name: "path_filename",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Tuple(vec![Type::Str, Type::Err])),
        },
    },
    BuiltinFunction {
        name: "fs_path_extension",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Tuple(vec![Type::Str, Type::Err])),
        },
    },
    BuiltinFunction {
        name: "path_extension",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Tuple(vec![Type::Str, Type::Err])),
        },
    },
    BuiltinFunction {
        name: "fs_metadata_len",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Tuple(vec![Type::Int { signed: false, bits: 64 }, Type::Err])),
        },
    },
    BuiltinFunction {
        name: "metadata_len",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Tuple(vec![Type::Int { signed: false, bits: 64 }, Type::Err])),
        },
    },
    BuiltinFunction {
        name: "fs_metadata_readonly",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Tuple(vec![Type::Bool, Type::Err])),
        },
    },
    BuiltinFunction {
        name: "metadata_readonly",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Tuple(vec![Type::Bool, Type::Err])),
        },
    },
    BuiltinFunction {
        name: "fs_set_readonly",
        ty: Type::Function {
            params: vec![Type::Str, Type::Bool],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "set_readonly",
        ty: Type::Function {
            params: vec![Type::Str, Type::Bool],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "math_pi",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "pi",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "math_e",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "e",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "math_tau",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "tau",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "math_abs",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "abs",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "math_sqrt",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "sqrt",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "math_pow",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }, Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "pow",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }, Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "math_exp",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "exp",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "math_log",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "log",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "math_log10",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "log10",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "math_floor",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "floor",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "math_ceil",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "ceil",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "math_round",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "round",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "math_trunc",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "trunc",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "math_fract",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "fract",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "math_sin",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "sin",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "math_cos",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "cos",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "math_tan",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "tan",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "math_asin",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "asin",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "math_acos",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "acos",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "math_atan",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "atan",
        ty: Type::Function {
            params: vec![Type::Float { bits: 64 }],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "math_rand_f64",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "rand_f64",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Float { bits: 64 }),
        },
    },
    BuiltinFunction {
        name: "math_rand_i32",
        ty: Type::Function {
            params: vec![
                Type::Int {
                    signed: true,
                    bits: 32,
                },
                Type::Int {
                    signed: true,
                    bits: 32,
                },
            ],
            ret: Box::new(Type::Tuple(vec![
                Type::Int {
                    signed: true,
                    bits: 32,
                },
                Type::Err,
            ])),
        },
    },
    BuiltinFunction {
        name: "rand_i32",
        ty: Type::Function {
            params: vec![
                Type::Int {
                    signed: true,
                    bits: 32,
                },
                Type::Int {
                    signed: true,
                    bits: 32,
                },
            ],
            ret: Box::new(Type::Tuple(vec![
                Type::Int {
                    signed: true,
                    bits: 32,
                },
                Type::Err,
            ])),
        },
    },
    BuiltinFunction {
        name: "time_now_unix_secs",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Int {
                signed: false,
                bits: 64,
            }),
        },
    },
    BuiltinFunction {
        name: "now_unix_secs",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Int {
                signed: false,
                bits: 64,
            }),
        },
    },
    BuiltinFunction {
        name: "time_now_unix_millis",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Int {
                signed: false,
                bits: 64,
            }),
        },
    },
    BuiltinFunction {
        name: "now_unix_millis",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Int {
                signed: false,
                bits: 64,
            }),
        },
    },
    BuiltinFunction {
        name: "time_sleep_ms",
        ty: Type::Function {
            params: vec![Type::Int {
                signed: false,
                bits: 64,
            }],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "sleep_ms",
        ty: Type::Function {
            params: vec![Type::Int {
                signed: false,
                bits: 64,
            }],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "time_tick_ms",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Int {
                signed: false,
                bits: 64,
            }),
        },
    },
    BuiltinFunction {
        name: "tick_ms",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Int {
                signed: false,
                bits: 64,
            }),
        },
    },
    BuiltinFunction {
        name: "time_elapsed_ms",
        ty: Type::Function {
            params: vec![Type::Int {
                signed: false,
                bits: 64,
            }],
            ret: Box::new(Type::Int {
                signed: false,
                bits: 64,
            }),
        },
    },
    BuiltinFunction {
        name: "elapsed_ms",
        ty: Type::Function {
            params: vec![Type::Int {
                signed: false,
                bits: 64,
            }],
            ret: Box::new(Type::Int {
                signed: false,
                bits: 64,
            }),
        },
    },
    BuiltinFunction {
        name: "time_now_iso_utc",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Tuple(vec![Type::Str, Type::Err])),
        },
    },
    BuiltinFunction {
        name: "now_iso_utc",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Tuple(vec![Type::Str, Type::Err])),
        },
    },
    BuiltinFunction {
        name: "time_from_unix_secs_iso_utc",
        ty: Type::Function {
            params: vec![Type::Int {
                signed: false,
                bits: 64,
            }],
            ret: Box::new(Type::Tuple(vec![Type::Str, Type::Err])),
        },
    },
    BuiltinFunction {
        name: "from_unix_secs_iso_utc",
        ty: Type::Function {
            params: vec![Type::Int {
                signed: false,
                bits: 64,
            }],
            ret: Box::new(Type::Tuple(vec![Type::Str, Type::Err])),
        },
    },
    BuiltinFunction {
        name: "os_platform",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Str),
        },
    },
    BuiltinFunction {
        name: "platform",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Str),
        },
    },
    BuiltinFunction {
        name: "os_arch",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Str),
        },
    },
    BuiltinFunction {
        name: "arch",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Str),
        },
    },
    BuiltinFunction {
        name: "os_pid",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Int {
                signed: false,
                bits: 64,
            }),
        },
    },
    BuiltinFunction {
        name: "pid",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Int {
                signed: false,
                bits: 64,
            }),
        },
    },
    BuiltinFunction {
        name: "os_getenv",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Tuple(vec![Type::Str, Type::Err])),
        },
    },
    BuiltinFunction {
        name: "getenv",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Tuple(vec![Type::Str, Type::Err])),
        },
    },
    BuiltinFunction {
        name: "os_setenv",
        ty: Type::Function {
            params: vec![Type::Str, Type::Str],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "setenv",
        ty: Type::Function {
            params: vec![Type::Str, Type::Str],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "os_unsetenv",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "unsetenv",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "os_args",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Array(Box::new(Type::Str))),
        },
    },
    BuiltinFunction {
        name: "args",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Array(Box::new(Type::Str))),
        },
    },
    BuiltinFunction {
        name: "os_exec",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Tuple(vec![
                Type::Int {
                    signed: true,
                    bits: 32,
                },
                Type::Str,
                Type::Str,
                Type::Err,
            ])),
        },
    },
    BuiltinFunction {
        name: "exec",
        ty: Type::Function {
            params: vec![Type::Str],
            ret: Box::new(Type::Tuple(vec![
                Type::Int {
                    signed: true,
                    bits: 32,
                },
                Type::Str,
                Type::Str,
                Type::Err,
            ])),
        },
    },
    BuiltinFunction {
        name: "os_signal_term",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Int {
                signed: true,
                bits: 32,
            }),
        },
    },
    BuiltinFunction {
        name: "signal_term",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Int {
                signed: true,
                bits: 32,
            }),
        },
    },
    BuiltinFunction {
        name: "os_signal_kill",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Int {
                signed: true,
                bits: 32,
            }),
        },
    },
    BuiltinFunction {
        name: "signal_kill",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Int {
                signed: true,
                bits: 32,
            }),
        },
    },
    BuiltinFunction {
        name: "os_signal_int",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Int {
                signed: true,
                bits: 32,
            }),
        },
    },
    BuiltinFunction {
        name: "signal_int",
        ty: Type::Function {
            params: vec![],
            ret: Box::new(Type::Int {
                signed: true,
                bits: 32,
            }),
        },
    },
    BuiltinFunction {
        name: "os_send_signal",
        ty: Type::Function {
            params: vec![
                Type::Int {
                    signed: false,
                    bits: 64,
                },
                Type::Int {
                    signed: true,
                    bits: 32,
                },
            ],
            ret: Box::new(Type::Err),
        },
    },
    BuiltinFunction {
        name: "send_signal",
        ty: Type::Function {
            params: vec![
                Type::Int {
                    signed: false,
                    bits: 64,
                },
                Type::Int {
                    signed: true,
                    bits: 32,
                },
            ],
            ret: Box::new(Type::Err),
        },
    },
    ];

    let mut methods = Vec::new();
    register_integer_methods(&mut methods, ReceiverMatcher::IntSignedAny, true);
    register_integer_methods(&mut methods, ReceiverMatcher::IntUnsignedAny, false);
    register_float_methods(&mut methods);
    register_bool_methods(&mut methods);
    register_char_methods(&mut methods);
    register_str_methods(&mut methods);
    register_array_methods(&mut methods);
    register_function_methods(&mut methods);
    register_map_methods(&mut methods);
    register_set_methods(&mut methods);

    // Method callables are also registered as builtin functions with deterministic names,
    // so VM keeps receiving only Call(SymbolId, argc).
    for method in &methods {
        functions.push(BuiltinFunction {
            name: method.symbol_name,
            ty: Type::Function {
                params: vec![Type::Any],
                ret: Box::new(Type::Any),
            },
        });
    }

    BuiltinRegistry {
        functions,
        methods,
    }
}

fn register_integer_methods(out: &mut Vec<BuiltinMethod>, receiver: ReceiverMatcher, signed: bool) {
    let suffix = if signed { "is" } else { "iu" };
    let mk = |name: &'static str, params: Vec<Type>, ret: Type, symbol_name: &'static str| BuiltinMethod {
        receiver,
        name,
        symbol_name,
        kind: BuiltinMethodKind::Instance,
        params,
        ret,
    };
    out.push(mk("add", vec![Type::Any], Type::Any, if signed { "__meth_is_add" } else { "__meth_iu_add" }));
    out.push(mk("sub", vec![Type::Any], Type::Any, if signed { "__meth_is_sub" } else { "__meth_iu_sub" }));
    out.push(mk("mul", vec![Type::Any], Type::Any, if signed { "__meth_is_mul" } else { "__meth_iu_mul" }));
    out.push(mk("div", vec![Type::Any], Type::Any, if signed { "__meth_is_div" } else { "__meth_iu_div" }));
    out.push(mk("mod", vec![Type::Any], Type::Any, if signed { "__meth_is_mod" } else { "__meth_iu_mod" }));
    if signed {
        out.push(mk("neg", vec![], Type::Any, "__meth_is_neg"));
        out.push(mk("abs", vec![], Type::Any, "__meth_is_abs"));
    }
    out.push(mk("and", vec![Type::Any], Type::Any, if signed { "__meth_is_and" } else { "__meth_iu_and" }));
    out.push(mk("or", vec![Type::Any], Type::Any, if signed { "__meth_is_or" } else { "__meth_iu_or" }));
    out.push(mk("xor", vec![Type::Any], Type::Any, if signed { "__meth_is_xor" } else { "__meth_iu_xor" }));
    out.push(mk("not", vec![], Type::Any, if signed { "__meth_is_not" } else { "__meth_iu_not" }));
    out.push(mk("shl", vec![Type::Any], Type::Any, if signed { "__meth_is_shl" } else { "__meth_iu_shl" }));
    out.push(mk("shr", vec![Type::Any], Type::Any, if signed { "__meth_is_shr" } else { "__meth_iu_shr" }));
    out.push(mk("rotl", vec![Type::Any], Type::Any, if signed { "__meth_is_rotl" } else { "__meth_iu_rotl" }));
    out.push(mk("rotr", vec![Type::Any], Type::Any, if signed { "__meth_is_rotr" } else { "__meth_iu_rotr" }));
    out.push(mk("eq", vec![Type::Any], Type::Bool, if signed { "__meth_is_eq" } else { "__meth_iu_eq" }));
    out.push(mk("ne", vec![Type::Any], Type::Bool, if signed { "__meth_is_ne" } else { "__meth_iu_ne" }));
    out.push(mk("lt", vec![Type::Any], Type::Bool, if signed { "__meth_is_lt" } else { "__meth_iu_lt" }));
    out.push(mk("le", vec![Type::Any], Type::Bool, if signed { "__meth_is_le" } else { "__meth_iu_le" }));
    out.push(mk("gt", vec![Type::Any], Type::Bool, if signed { "__meth_is_gt" } else { "__meth_iu_gt" }));
    out.push(mk("ge", vec![Type::Any], Type::Bool, if signed { "__meth_is_ge" } else { "__meth_iu_ge" }));
    out.push(mk("cmp", vec![Type::Any], Type::Int { signed: true, bits: 8 }, if signed { "__meth_is_cmp" } else { "__meth_iu_cmp" }));
    out.push(mk("min", vec![Type::Any], Type::Any, if signed { "__meth_is_min" } else { "__meth_iu_min" }));
    out.push(mk("max", vec![Type::Any], Type::Any, if signed { "__meth_is_max" } else { "__meth_iu_max" }));
    out.push(mk("clamp", vec![Type::Any, Type::Any], Type::Any, if signed { "__meth_is_clamp" } else { "__meth_iu_clamp" }));
    out.push(mk("is_zero", vec![], Type::Bool, if signed { "__meth_is_is_zero" } else { "__meth_iu_is_zero" }));
    out.push(mk("is_even", vec![], Type::Bool, if signed { "__meth_is_is_even" } else { "__meth_iu_is_even" }));
    out.push(mk("is_odd", vec![], Type::Bool, if signed { "__meth_is_is_odd" } else { "__meth_iu_is_odd" }));
    out.push(mk("checked_add", vec![Type::Any], Type::Tuple(vec![Type::Any, Type::Err]), if signed { "__meth_is_checked_add" } else { "__meth_iu_checked_add" }));
    out.push(mk("checked_sub", vec![Type::Any], Type::Tuple(vec![Type::Any, Type::Err]), if signed { "__meth_is_checked_sub" } else { "__meth_iu_checked_sub" }));
    out.push(mk("checked_mul", vec![Type::Any], Type::Tuple(vec![Type::Any, Type::Err]), if signed { "__meth_is_checked_mul" } else { "__meth_iu_checked_mul" }));
    out.push(mk("checked_div", vec![Type::Any], Type::Tuple(vec![Type::Any, Type::Err]), if signed { "__meth_is_checked_div" } else { "__meth_iu_checked_div" }));
    out.push(mk("wrapping_add", vec![Type::Any], Type::Any, if signed { "__meth_is_wrapping_add" } else { "__meth_iu_wrapping_add" }));
    out.push(mk("wrapping_sub", vec![Type::Any], Type::Any, if signed { "__meth_is_wrapping_sub" } else { "__meth_iu_wrapping_sub" }));
    out.push(mk("saturating_add", vec![Type::Any], Type::Any, if signed { "__meth_is_saturating_add" } else { "__meth_iu_saturating_add" }));
    out.push(mk("saturating_sub", vec![Type::Any], Type::Any, if signed { "__meth_is_saturating_sub" } else { "__meth_iu_saturating_sub" }));
    out.push(mk("to_i32", vec![], Type::Int { signed: true, bits: 32 }, if signed { "__meth_is_to_i32" } else { "__meth_iu_to_i32" }));
    out.push(mk("to_u32", vec![], Type::Int { signed: false, bits: 32 }, if signed { "__meth_is_to_u32" } else { "__meth_iu_to_u32" }));
    out.push(mk("to_f32", vec![], Type::Float { bits: 32 }, if signed { "__meth_is_to_f32" } else { "__meth_iu_to_f32" }));
    out.push(mk("to_bool", vec![], Type::Bool, if signed { "__meth_is_to_bool" } else { "__meth_iu_to_bool" }));
    out.push(mk("to_str", vec![], Type::Str, if signed { "__meth_is_to_str" } else { "__meth_iu_to_str" }));
    let _ = suffix;
}

fn register_float_methods(out: &mut Vec<BuiltinMethod>) {
    let mk = |name: &'static str, params: Vec<Type>, ret: Type, symbol_name: &'static str| BuiltinMethod {
        receiver: ReceiverMatcher::FloatAny,
        name,
        symbol_name,
        kind: BuiltinMethodKind::Instance,
        params,
        ret,
    };
    out.push(mk("add", vec![Type::Any], Type::Any, "__meth_f_add"));
    out.push(mk("sub", vec![Type::Any], Type::Any, "__meth_f_sub"));
    out.push(mk("mul", vec![Type::Any], Type::Any, "__meth_f_mul"));
    out.push(mk("div", vec![Type::Any], Type::Any, "__meth_f_div"));
    out.push(mk("mod", vec![Type::Any], Type::Any, "__meth_f_mod"));
    out.push(mk("neg", vec![], Type::Any, "__meth_f_neg"));
    out.push(mk("eq", vec![Type::Any], Type::Bool, "__meth_f_eq"));
    out.push(mk("lt", vec![Type::Any], Type::Bool, "__meth_f_lt"));
    out.push(mk("gt", vec![Type::Any], Type::Bool, "__meth_f_gt"));
    out.push(mk(
        "cmp",
        vec![Type::Any],
        Type::Int {
            signed: true,
            bits: 8,
        },
        "__meth_f_cmp",
    ));
    out.push(mk("abs", vec![], Type::Any, "__meth_f_abs"));
    out.push(mk("sqrt", vec![], Type::Any, "__meth_f_sqrt"));
    out.push(mk("pow", vec![Type::Any], Type::Any, "__meth_f_pow"));
    out.push(mk("exp", vec![], Type::Any, "__meth_f_exp"));
    out.push(mk("log", vec![], Type::Any, "__meth_f_log"));
    out.push(mk("log10", vec![], Type::Any, "__meth_f_log10"));
    out.push(mk("floor", vec![], Type::Any, "__meth_f_floor"));
    out.push(mk("ceil", vec![], Type::Any, "__meth_f_ceil"));
    out.push(mk("round", vec![], Type::Any, "__meth_f_round"));
    out.push(mk("trunc", vec![], Type::Any, "__meth_f_trunc"));
    out.push(mk("fract", vec![], Type::Any, "__meth_f_fract"));
    out.push(mk("sin", vec![], Type::Any, "__meth_f_sin"));
    out.push(mk("cos", vec![], Type::Any, "__meth_f_cos"));
    out.push(mk("tan", vec![], Type::Any, "__meth_f_tan"));
    out.push(mk("asin", vec![], Type::Any, "__meth_f_asin"));
    out.push(mk("acos", vec![], Type::Any, "__meth_f_acos"));
    out.push(mk("atan", vec![], Type::Any, "__meth_f_atan"));
    out.push(mk("is_nan", vec![], Type::Bool, "__meth_f_is_nan"));
    out.push(mk("is_inf", vec![], Type::Bool, "__meth_f_is_inf"));
    out.push(mk("is_finite", vec![], Type::Bool, "__meth_f_is_finite"));
    out.push(mk(
        "to_i32",
        vec![],
        Type::Int {
            signed: true,
            bits: 32,
        },
        "__meth_f_to_i32",
    ));
    out.push(mk("to_str", vec![], Type::Str, "__meth_f_to_str"));
}

fn register_bool_methods(out: &mut Vec<BuiltinMethod>) {
    let mk = |name: &'static str, params: Vec<Type>, ret: Type, symbol_name: &'static str| BuiltinMethod {
        receiver: ReceiverMatcher::Bool,
        name,
        symbol_name,
        kind: BuiltinMethodKind::Instance,
        params,
        ret,
    };
    out.push(mk("and", vec![Type::Bool], Type::Bool, "__meth_b_and"));
    out.push(mk("or", vec![Type::Bool], Type::Bool, "__meth_b_or"));
    out.push(mk("xor", vec![Type::Bool], Type::Bool, "__meth_b_xor"));
    out.push(mk("not", vec![], Type::Bool, "__meth_b_not"));
    out.push(mk("eq", vec![Type::Bool], Type::Bool, "__meth_b_eq"));
    out.push(mk(
        "to_i32",
        vec![],
        Type::Int {
            signed: true,
            bits: 32,
        },
        "__meth_b_to_i32",
    ));
    out.push(mk("to_str", vec![], Type::Str, "__meth_b_to_str"));
}

fn register_char_methods(out: &mut Vec<BuiltinMethod>) {
    let mk = |name: &'static str, params: Vec<Type>, ret: Type, symbol_name: &'static str| BuiltinMethod {
        receiver: ReceiverMatcher::Char,
        name,
        symbol_name,
        kind: BuiltinMethodKind::Instance,
        params,
        ret,
    };
    out.push(mk("eq", vec![Type::Char], Type::Bool, "__meth_c_eq"));
    out.push(mk("is_digit", vec![], Type::Bool, "__meth_c_is_digit"));
    out.push(mk("is_alpha", vec![], Type::Bool, "__meth_c_is_alpha"));
    out.push(mk("is_alnum", vec![], Type::Bool, "__meth_c_is_alnum"));
    out.push(
        mk(
            "is_whitespace",
            vec![],
            Type::Bool,
            "__meth_c_is_whitespace",
        ),
    );
    out.push(mk("to_upper", vec![], Type::Char, "__meth_c_to_upper"));
    out.push(mk("to_lower", vec![], Type::Char, "__meth_c_to_lower"));
    out.push(
        mk(
            "to_i32",
            vec![],
            Type::Int {
                signed: true,
                bits: 32,
            },
            "__meth_c_to_i32",
        ),
    );
    out.push(mk("to_str", vec![], Type::Str, "__meth_c_to_str"));
}

fn register_str_methods(out: &mut Vec<BuiltinMethod>) {
    let mk = |name: &'static str, params: Vec<Type>, ret: Type, symbol_name: &'static str| BuiltinMethod {
        receiver: ReceiverMatcher::Str,
        name,
        symbol_name,
        kind: BuiltinMethodKind::Instance,
        params,
        ret,
    };
    out.push(mk(
        "len",
        vec![],
        Type::Int {
            signed: false,
            bits: 64,
        },
        "__meth_s_len",
    ));
    out.push(mk("is_empty", vec![], Type::Bool, "__meth_s_is_empty"));
    out.push(mk(
        "char_at",
        vec![Type::Int {
            signed: false,
            bits: 64,
        }],
        Type::Tuple(vec![Type::Char, Type::Err]),
        "__meth_s_char_at",
    ));
    out.push(mk("contains", vec![Type::Str], Type::Bool, "__meth_s_contains"));
    out.push(mk(
        "starts_with",
        vec![Type::Str],
        Type::Bool,
        "__meth_s_starts_with",
    ));
    out.push(mk("ends_with", vec![Type::Str], Type::Bool, "__meth_s_ends_with"));
    out.push(mk(
        "find",
        vec![Type::Str],
        Type::Int {
            signed: true,
            bits: 64,
        },
        "__meth_s_find",
    ));
    out.push(mk(
        "rfind",
        vec![Type::Str],
        Type::Int {
            signed: true,
            bits: 64,
        },
        "__meth_s_rfind",
    ));
    out.push(mk(
        "slice",
        vec![
            Type::Int {
                signed: false,
                bits: 64,
            },
            Type::Int {
                signed: false,
                bits: 64,
            },
        ],
        Type::Str,
        "__meth_s_slice",
    ));
    out.push(mk("split", vec![Type::Str], Type::Array(Box::new(Type::Str)), "__meth_s_split"));
    out.push(mk("replace", vec![Type::Str, Type::Str], Type::Str, "__meth_s_replace"));
    out.push(mk("trim", vec![], Type::Str, "__meth_s_trim"));
    out.push(mk("trim_start", vec![], Type::Str, "__meth_s_trim_start"));
    out.push(mk("trim_end", vec![], Type::Str, "__meth_s_trim_end"));
    out.push(mk("to_upper", vec![], Type::Str, "__meth_s_to_upper"));
    out.push(mk("to_lower", vec![], Type::Str, "__meth_s_to_lower"));
    out.push(mk("reverse", vec![], Type::Str, "__meth_s_reverse"));
    out.push(mk(
        "to_i32",
        vec![],
        Type::Tuple(vec![
            Type::Int {
                signed: true,
                bits: 32,
            },
            Type::Err,
        ]),
        "__meth_s_to_i32",
    ));
    out.push(mk(
        "to_f64",
        vec![],
        Type::Tuple(vec![Type::Float { bits: 64 }, Type::Err]),
        "__meth_s_to_f64",
    ));
}

fn register_array_methods(out: &mut Vec<BuiltinMethod>) {
    let mk = |name: &'static str, params: Vec<Type>, ret: Type, symbol_name: &'static str| BuiltinMethod {
        receiver: ReceiverMatcher::ArrayAny,
        name,
        symbol_name,
        kind: BuiltinMethodKind::Instance,
        params,
        ret,
    };
    out.push(mk(
        "len",
        vec![],
        Type::Int {
            signed: false,
            bits: 64,
        },
        "__meth_a_len",
    ));
    out.push(mk("is_empty", vec![], Type::Bool, "__meth_a_is_empty"));
    out.push(mk(
        "get",
        vec![Type::Int {
            signed: false,
            bits: 64,
        }],
        Type::Tuple(vec![Type::Any, Type::Err]),
        "__meth_a_get",
    ));
    out.push(mk("set", vec![Type::Int { signed: false, bits: 64 }, Type::Any], Type::Unit, "__meth_a_set"));
    out.push(mk("push", vec![Type::Any], Type::Unit, "__meth_a_push"));
    out.push(mk("pop", vec![], Type::Any, "__meth_a_pop"));
    out.push(mk(
        "insert",
        vec![Type::Int { signed: false, bits: 64 }, Type::Any],
        Type::Unit,
        "__meth_a_insert",
    ));
    out.push(mk(
        "remove",
        vec![Type::Int { signed: false, bits: 64 }],
        Type::Tuple(vec![Type::Any, Type::Err]),
        "__meth_a_remove",
    ));
    out.push(mk("clear", vec![], Type::Unit, "__meth_a_clear"));
    out.push(mk("find", vec![Type::Any], Type::Int { signed: true, bits: 64 }, "__meth_a_find"));
    out.push(mk("contains", vec![Type::Any], Type::Bool, "__meth_a_contains"));
    out.push(mk("reverse", vec![], Type::Any, "__meth_a_reverse"));
    out.push(mk("sort", vec![], Type::Any, "__meth_a_sort"));
    out.push(mk(
        "slice",
        vec![
            Type::Int {
                signed: false,
                bits: 64,
            },
            Type::Int {
                signed: false,
                bits: 64,
            },
        ],
        Type::Any,
        "__meth_a_slice",
    ));
}

fn register_function_methods(out: &mut Vec<BuiltinMethod>) {
    let mk = |name: &'static str, params: Vec<Type>, ret: Type, symbol_name: &'static str| BuiltinMethod {
        receiver: ReceiverMatcher::FunctionAny,
        name,
        symbol_name,
        kind: BuiltinMethodKind::Instance,
        params,
        ret,
    };
    out.push(mk("call", vec![Type::Any], Type::Any, "__meth_fn_call"));
    out.push(mk(
        "arity",
        vec![],
        Type::Int {
            signed: false,
            bits: 32,
        },
        "__meth_fn_arity",
    ));
    out.push(mk("bind", vec![Type::Any], Type::Any, "__meth_fn_bind"));
    out.push(mk("compose", vec![Type::Any], Type::Any, "__meth_fn_compose"));
    out.push(mk("partial", vec![Type::Any], Type::Any, "__meth_fn_partial"));
}

fn register_map_methods(out: &mut Vec<BuiltinMethod>) {
    let mk = |name: &'static str, params: Vec<Type>, ret: Type, symbol_name: &'static str| BuiltinMethod {
        receiver: ReceiverMatcher::MapAny,
        name,
        symbol_name,
        kind: BuiltinMethodKind::Instance,
        params,
        ret,
    };
    out.push(mk("len", vec![], Type::Int { signed: false, bits: 64 }, "__meth_m_len"));
    out.push(mk("is_empty", vec![], Type::Bool, "__meth_m_is_empty"));
    out.push(mk("get", vec![Type::Any], Type::Any, "__meth_m_get"));
    out.push(mk("get_or", vec![Type::Any, Type::Any], Type::Any, "__meth_m_get_or"));
    out.push(mk("get_or_insert", vec![Type::Any, Type::Any], Type::Any, "__meth_m_get_or_insert"));
    out.push(mk("contains_key", vec![Type::Any], Type::Bool, "__meth_m_contains_key"));
    out.push(mk("insert", vec![Type::Any, Type::Any], Type::Any, "__meth_m_insert"));
    out.push(mk("remove", vec![Type::Any], Type::Any, "__meth_m_remove"));
    out.push(mk("clear", vec![], Type::Unit, "__meth_m_clear"));
    out.push(mk("update", vec![Type::Any, Type::Any], Type::Any, "__meth_m_update"));
    out.push(mk("keys", vec![], Type::Any, "__meth_m_keys"));
    out.push(mk("values", vec![], Type::Any, "__meth_m_values"));
    out.push(mk("entries", vec![], Type::Any, "__meth_m_entries"));
    out.push(mk("merge", vec![Type::Any], Type::Any, "__meth_m_merge"));
    out.push(mk("merge_with", vec![Type::Any, Type::Any], Type::Any, "__meth_m_merge_with"));
    out.push(mk("clone", vec![], Type::Any, "__meth_m_clone"));
    out.push(mk("eq", vec![Type::Any], Type::Bool, "__meth_m_eq"));
    out.push(mk("ne", vec![Type::Any], Type::Bool, "__meth_m_ne"));
}

fn register_set_methods(out: &mut Vec<BuiltinMethod>) {
    let mk = |name: &'static str, params: Vec<Type>, ret: Type, symbol_name: &'static str| BuiltinMethod {
        receiver: ReceiverMatcher::SetAny,
        name,
        symbol_name,
        kind: BuiltinMethodKind::Instance,
        params,
        ret,
    };
    out.push(mk("len", vec![], Type::Int { signed: false, bits: 64 }, "__meth_set_len"));
    out.push(mk("is_empty", vec![], Type::Bool, "__meth_set_is_empty"));
    out.push(mk("contains", vec![Type::Any], Type::Bool, "__meth_set_contains"));
    out.push(mk("insert", vec![Type::Any], Type::Bool, "__meth_set_insert"));
    out.push(mk("remove", vec![Type::Any], Type::Bool, "__meth_set_remove"));
    out.push(mk("clear", vec![], Type::Unit, "__meth_set_clear"));
    out.push(mk("values", vec![], Type::Any, "__meth_set_values"));
    out.push(mk("union", vec![Type::Any], Type::Any, "__meth_set_union"));
    out.push(mk("intersection", vec![Type::Any], Type::Any, "__meth_set_intersection"));
    out.push(mk("difference", vec![Type::Any], Type::Any, "__meth_set_difference"));
    out.push(mk(
        "symmetric_difference",
        vec![Type::Any],
        Type::Any,
        "__meth_set_symmetric_difference",
    ));
    out.push(mk("is_subset", vec![Type::Any], Type::Bool, "__meth_set_is_subset"));
    out.push(mk("is_superset", vec![Type::Any], Type::Bool, "__meth_set_is_superset"));
    out.push(mk("is_disjoint", vec![Type::Any], Type::Bool, "__meth_set_is_disjoint"));
    out.push(mk("clone", vec![], Type::Any, "__meth_set_clone"));
    out.push(mk("eq", vec![Type::Any], Type::Bool, "__meth_set_eq"));
    out.push(mk("ne", vec![Type::Any], Type::Bool, "__meth_set_ne"));
}

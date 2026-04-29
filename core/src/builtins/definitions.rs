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
    }];

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

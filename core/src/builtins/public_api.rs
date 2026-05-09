use std::{
    collections::{HashMap, HashSet},
    sync::OnceLock,
};

const STDLIB_MODULE_SOURCES: &[(&str, &str)] = &[
    ("std/cli", include_str!("../../../stdlib/std/cli.pand")),
    ("std/core", include_str!("../../../stdlib/std/core.pand")),
    (
        "std/crypto",
        include_str!("../../../stdlib/std/crypto.pand"),
    ),
    ("std/csv", include_str!("../../../stdlib/std/csv.pand")),
    (
        "std/encoding",
        include_str!("../../../stdlib/std/encoding.pand"),
    ),
    ("std/env", include_str!("../../../stdlib/std/env.pand")),
    ("std/fs", include_str!("../../../stdlib/std/fs.pand")),
    ("std/http", include_str!("../../../stdlib/std/http.pand")),
    ("std/io", include_str!("../../../stdlib/std/io.pand")),
    ("std/json", include_str!("../../../stdlib/std/json.pand")),
    ("std/log", include_str!("../../../stdlib/std/log.pand")),
    ("std/math", include_str!("../../../stdlib/std/math.pand")),
    ("std/mime", include_str!("../../../stdlib/std/mime.pand")),
    ("std/net", include_str!("../../../stdlib/std/net.pand")),
    ("std/os", include_str!("../../../stdlib/std/os.pand")),
    ("std/path", include_str!("../../../stdlib/std/path.pand")),
    ("std/proc", include_str!("../../../stdlib/std/proc.pand")),
    ("std/rand", include_str!("../../../stdlib/std/rand.pand")),
    ("std/regex", include_str!("../../../stdlib/std/regex.pand")),
    ("std/sync", include_str!("../../../stdlib/std/sync.pand")),
    (
        "std/thread",
        include_str!("../../../stdlib/std/thread.pand"),
    ),
    ("std/time", include_str!("../../../stdlib/std/time.pand")),
    ("std/url", include_str!("../../../stdlib/std/url.pand")),
    ("std/xml", include_str!("../../../stdlib/std/xml.pand")),
];

const INTERNAL_STDLIB_PREFIXES: &[&str] = &[
    "io_",
    "fs_",
    "math_",
    "time_",
    "os_",
    "proc_",
    "thread_",
    "sync_",
    "net_",
    "http_",
    "crypto_",
    "rand_",
    "encoding_",
    "regex_",
    "cli_",
    "env_",
    "log_",
    "json_",
    "csv_",
    "mime_",
    "url_",
    "xml_",
];

const PRELUDE_BUILTIN_SYMBOLS: &[&str] = &["print", "len", "error", "panic", "wrap", "typeof"];

pub fn stdlib_module_exports(path: &str) -> Option<&'static HashSet<String>> {
    static STDLIB_EXPORTS: OnceLock<HashMap<&'static str, HashSet<String>>> = OnceLock::new();
    STDLIB_EXPORTS
        .get_or_init(|| {
            STDLIB_MODULE_SOURCES
                .iter()
                .map(|(path, source)| (*path, collect_stdlib_exports(source)))
                .collect()
        })
        .get(path)
}

pub fn public_stdlib_function_names() -> &'static HashSet<String> {
    static PUBLIC_STDLIB_FUNCTIONS: OnceLock<HashSet<String>> = OnceLock::new();
    PUBLIC_STDLIB_FUNCTIONS.get_or_init(|| {
        STDLIB_MODULE_SOURCES
            .iter()
            .flat_map(|(_, source)| collect_stdlib_exports(source).into_iter())
            .collect()
    })
}

pub fn normalize_stdlib_path(path: &str) -> String {
    path.trim_matches('"').to_string()
}

pub fn is_internal_stdlib_symbol(name: &str) -> bool {
    INTERNAL_STDLIB_PREFIXES
        .iter()
        .any(|prefix| name.starts_with(prefix))
}

pub fn is_prelude_builtin_symbol(name: &str) -> bool {
    PRELUDE_BUILTIN_SYMBOLS.contains(&name)
}

fn collect_stdlib_exports(source: &str) -> HashSet<String> {
    source
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let name = line.strip_prefix("fn ")?.split('(').next()?.trim();
            (!name.is_empty()).then(|| name.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use crate::builtins::default_registry;

    use super::{
        is_internal_stdlib_symbol, is_prelude_builtin_symbol, normalize_stdlib_path,
        public_stdlib_function_names, stdlib_module_exports,
    };

    #[test]
    fn every_public_stdlib_export_exists_in_registry() {
        let registry = default_registry();
        let registered = registry
            .functions
            .iter()
            .map(|function| function.name)
            .collect::<HashSet<_>>();
        let missing = public_stdlib_function_names()
            .iter()
            .filter(|name| !registered.contains(name.as_str()))
            .cloned()
            .collect::<Vec<_>>();

        assert!(
            missing.is_empty(),
            "missing public stdlib functions in registry: {missing:?}"
        );
    }

    #[test]
    fn registry_public_functions_are_declared_in_stdlib_modules() {
        let registry = default_registry();
        let documented = public_stdlib_function_names();
        let undocumented = registry
            .functions
            .iter()
            .map(|function| function.name)
            .filter(|name| {
                !name.starts_with("__meth_")
                    && !is_internal_stdlib_symbol(name)
                    && !is_prelude_builtin_symbol(name)
                    && !documented.contains(*name)
            })
            .collect::<Vec<_>>();

        assert!(
            undocumented.is_empty(),
            "registry exposes public functions missing from stdlib modules: {undocumented:?}"
        );
    }

    #[test]
    fn stdlib_modules_keep_known_exports_accessible() {
        let core_exports = stdlib_module_exports("std/core").expect("std/core exports");
        let io_exports = stdlib_module_exports("std/io").expect("std/io exports");
        let http_exports = stdlib_module_exports("std/http").expect("std/http exports");

        assert!(core_exports.contains("helper"));
        assert!(io_exports.contains("read_text"));
        assert!(http_exports.contains("parse_request"));
    }

    #[test]
    fn prelude_builtins_are_not_treated_as_module_exports() {
        let core_exports = stdlib_module_exports("std/core").expect("std/core exports");

        assert!(is_prelude_builtin_symbol("print"));
        assert!(is_prelude_builtin_symbol("typeof"));
        assert!(!core_exports.contains("print"));
        assert!(!core_exports.contains("typeof"));
    }

    // --- Comprehensive intrinsic categorization (Phase 6 coverage) ---
    #[test]
    fn io_intrinsics_are_categorized_as_internal() {
        assert!(is_internal_stdlib_symbol("io_stdout_write"));
        assert!(is_internal_stdlib_symbol("io_stderr_write"));
        assert!(is_internal_stdlib_symbol("io_read_text"));
        assert!(is_internal_stdlib_symbol("io_write_text"));
        assert!(is_internal_stdlib_symbol("io_exists"));
        assert!(is_internal_stdlib_symbol("io_append_text"));
        assert!(!is_prelude_builtin_symbol("io_stdout_write"));
    }

    #[test]
    fn fs_intrinsics_are_categorized_as_internal() {
        assert!(is_internal_stdlib_symbol("fs_create_dir"));
        assert!(is_internal_stdlib_symbol("fs_create_dir_all"));
        assert!(is_internal_stdlib_symbol("fs_remove_file"));
        assert!(is_internal_stdlib_symbol("fs_remove_dir"));
        assert!(is_internal_stdlib_symbol("fs_remove_dir_all"));
        assert!(is_internal_stdlib_symbol("fs_path_normalize"));
    }

    #[test]
    fn math_intrinsics_are_categorized_as_internal() {
        assert!(is_internal_stdlib_symbol("math_rand_i32"));
        assert!(is_internal_stdlib_symbol("math_rand_u64"));
    }

    #[test]
    fn time_intrinsics_are_categorized_as_internal() {
        assert!(is_internal_stdlib_symbol("time_sleep_ms"));
    }

    #[test]
    fn http_intrinsics_are_categorized_as_internal() {
        assert!(is_internal_stdlib_symbol("http_parse_request"));
        assert!(is_internal_stdlib_symbol("http_parse_response"));
    }

    #[test]
    fn json_intrinsics_are_categorized_as_internal() {
        assert!(is_internal_stdlib_symbol("json_parse"));
        assert!(is_internal_stdlib_symbol("json_stringify"));
    }

    #[test]
    fn crypto_intrinsics_are_categorized_as_internal() {
        assert!(is_internal_stdlib_symbol("crypto_random_bytes"));
    }

    #[test]
    fn all_prelude_builtins_are_explicit() {
        assert!(is_prelude_builtin_symbol("print"));
        assert!(is_prelude_builtin_symbol("len"));
        assert!(is_prelude_builtin_symbol("error"));
        assert!(is_prelude_builtin_symbol("panic"));
        assert!(is_prelude_builtin_symbol("wrap"));
        assert!(is_prelude_builtin_symbol("typeof"));
    }

    #[test]
    fn non_prelude_names_return_false() {
        assert!(!is_prelude_builtin_symbol("helper"));
        assert!(!is_prelude_builtin_symbol("read_text"));
        assert!(!is_prelude_builtin_symbol("parse_request"));
    }

    // --- Stdlib module coverage (Phase 6 coverage) ---
    #[test]
    fn all_stdlib_modules_are_accessible() {
        let modules = vec![
            "std/cli",
            "std/core",
            "std/crypto",
            "std/csv",
            "std/encoding",
            "std/env",
            "std/fs",
            "std/http",
            "std/io",
            "std/json",
            "std/log",
            "std/math",
            "std/mime",
            "std/net",
            "std/os",
            "std/path",
            "std/proc",
            "std/rand",
            "std/regex",
            "std/sync",
            "std/thread",
            "std/time",
            "std/url",
            "std/xml",
        ];

        for module_path in modules {
            assert!(
                stdlib_module_exports(module_path).is_some(),
                "module {module_path} should be accessible"
            );
        }
    }

    #[test]
    fn nonexistent_module_returns_none() {
        assert!(stdlib_module_exports("std/nonexistent").is_none());
        assert!(stdlib_module_exports("other/module").is_none());
    }

    #[test]
    fn public_stdlib_function_names_is_non_empty() {
        let public_fns = public_stdlib_function_names();
        assert!(
            !public_fns.is_empty(),
            "public stdlib must have some functions"
        );
    }

    #[test]
    fn public_stdlib_includes_known_functions() {
        let public = public_stdlib_function_names();
        assert!(public.contains("helper"));
        assert!(public.contains("read_text"));
        assert!(public.contains("parse_request"));
        assert!(public.contains("assert"));
    }

    #[test]
    fn internal_intrinsics_not_in_public_stdlib_functions() {
        let public = public_stdlib_function_names();
        assert!(!public.contains("io_stdout_write"));
        assert!(!public.contains("fs_create_dir"));
        assert!(!public.contains("json_parse"));
    }

    #[test]
    fn prelude_builtins_not_in_public_stdlib_functions() {
        let public = public_stdlib_function_names();
        assert!(!public.contains("print"));
        assert!(!public.contains("len"));
        assert!(!public.contains("error"));
        assert!(!public.contains("typeof"));
    }

    #[test]
    fn normalize_stdlib_path_removes_quotes() {
        assert_eq!(normalize_stdlib_path("\"std/core\""), "std/core");
        assert_eq!(normalize_stdlib_path("std/core"), "std/core");
        assert_eq!(normalize_stdlib_path("\"std/http\""), "std/http");
    }

    #[test]
    fn stdlib_core_exports_contain_helpers() {
        let core_exports = stdlib_module_exports("std/core").expect("std/core");
        assert!(core_exports.contains("helper"));
        assert!(core_exports.contains("assert"));
        assert!(core_exports.contains("assert_eq_i32"));
    }

    #[test]
    fn stdlib_io_exports_contain_known_functions() {
        let io_exports = stdlib_module_exports("std/io").expect("std/io");
        assert!(io_exports.contains("read_text"));
    }

    #[test]
    fn stdlib_http_exports_contain_known_functions() {
        let http_exports = stdlib_module_exports("std/http").expect("std/http");
        assert!(http_exports.contains("parse_request"));
    }
}

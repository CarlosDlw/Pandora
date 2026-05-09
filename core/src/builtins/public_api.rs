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
        is_internal_stdlib_symbol, is_prelude_builtin_symbol, public_stdlib_function_names,
        stdlib_module_exports,
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
}

use foundation::{
    diagnostics::Diagnostics,
    frontend::PandoraFrontend,
    ids::FileId,
};

use crate::{
    analyzer::analyze_with_registry,
    builtins::{default_registry, BuiltinRegistry},
    lexer::lex,
    lowering::lower_with_registry,
    parser::parse,
    stdlib::embedded_core_std_pbc,
    vm::{compile_program, execute},
};

/// End-to-end compile/execute for one source file; always returns accumulated diagnostics.
pub fn compile_file(file_id: FileId, source: &str) -> Diagnostics {
    compile_file_with_registry(file_id, source, &default_registry())
}

pub fn compile_file_with_registry(file_id: FileId, source: &str, registry: &BuiltinRegistry) -> Diagnostics {
    let _core_std = embedded_core_std_pbc();
    let lex_output = lex(file_id, source);
    let mut diagnostics = lex_output.diagnostics;

    let (ast, parser_diagnostics) = parse(file_id, source.len() as u32, lex_output.tokens);
    diagnostics.extend(parser_diagnostics);
    if diagnostics.has_errors() {
        return diagnostics;
    }

    let (hir, mut symbols, lower_diagnostics) = lower_with_registry(&ast, registry);
    diagnostics.extend(lower_diagnostics);
    if diagnostics.has_errors() {
        return diagnostics;
    }

    let (semantic_model, analyze_diagnostics) = analyze_with_registry(&hir, &mut symbols, registry);
    diagnostics.extend(analyze_diagnostics);
    if diagnostics.has_errors() {
        return diagnostics;
    }

    let (chunk, compile_diagnostics) = compile_program(&hir, &semantic_model);
    diagnostics.extend(compile_diagnostics);
    if diagnostics.has_errors() {
        return diagnostics;
    }

    if let Err(vm_diagnostics) = execute(&chunk, &symbols) {
        diagnostics.extend(vm_diagnostics);
    }
    diagnostics
}

/// Adapter to plug `core` into `foundation::pipeline::Pipeline`.
#[derive(Default)]
pub struct CoreFrontend;

impl PandoraFrontend for CoreFrontend {
    fn compile_file(
        &mut self,
        file_id: FileId,
        source: &str,
        builtins: Option<&std::sync::Arc<dyn std::any::Any + Send + Sync>>,
    ) -> Diagnostics {
        if let Some(any_registry) = builtins {
            if let Some(registry) = any_registry.downcast_ref::<BuiltinRegistry>() {
                return compile_file_with_registry(file_id, source, registry);
            }
        }
        compile_file(file_id, source)
    }
}

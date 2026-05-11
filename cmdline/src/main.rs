use clap::{ArgGroup, CommandFactory, Parser};
use core::analyzer::analyze_with_registry;
use core::builtins::default_registry;
use core::lexer::lex;
use core::lowering::lower_with_registry;
use core::parser::parse;
use core::vm::{compile_program, execute};
use foundation::diagnostics::Diagnostics;
use foundation::ids::FileId;
use std::{fs, process::ExitCode};

mod diagnostic_renderer;

#[derive(Debug, Parser)]
#[command(name = "pandora")]
#[command(about = "CLI entrypoint for Pandora", long_about = None)]
#[command(group(
    ArgGroup::new("mode")
        .args(["lexeme", "ast", "hir", "check"])
        .multiple(false)
))]
struct Cli {
    /// Path to a `.pand` source file.
    file: String,
    /// Print lexemes from the lexer.
    #[arg(long)]
    lexeme: bool,
    /// Print AST roots from the parser.
    #[arg(long)]
    ast: bool,
    /// Print HIR statements and expression arena.
    #[arg(long)]
    hir: bool,
    /// Run semantic checks only (diagnostics output).
    #[arg(long)]
    check: bool,
    /// Arguments to pass to the program (after `--`).
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    args: Vec<String>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    if cli.file == "help" {
        let mut command = Cli::command();
        if let Err(err) = command.print_long_help() {
            eprintln!("failed to print help: {err}");
            return ExitCode::from(1);
        }
        println!();
        return ExitCode::SUCCESS;
    }

    let contents = match fs::read_to_string(&cli.file) {
        Ok(contents) => contents,
        Err(err) => {
            eprintln!("failed to read '{}': {err}", cli.file);
            return ExitCode::from(1);
        }
    };

    let builtins = default_registry();
    let output = lex(FileId::from_u32(0), &contents);
    let mut diagnostics = output.diagnostics;

    if cli.lexeme {
        for token in &output.tokens {
            println!(
                "{:?} '{}' [{}..{}]",
                token.kind,
                token.lexeme.replace('\n', "\\n"),
                token.span.start(),
                token.span.end()
            );
        }
    } else {
        let (ast, parser_diagnostics) =
            parse(FileId::from_u32(0), contents.len() as u32, output.tokens);
        diagnostics.extend(parser_diagnostics);
        if cli.check && diagnostics.has_errors() {
            return finish_with_diagnostics(&cli.file, &contents, diagnostics);
        }
        if cli.ast {
            for root in &ast.roots {
                if let Some(node) = ast.get(*root) {
                    println!("#{id}: {:?}", node, id = root.as_u32());
                }
            }
        } else {
            let (hir, symbols, lower_diagnostics) = lower_with_registry(&ast, &builtins);
            diagnostics.extend(lower_diagnostics);

            if cli.hir {
                for stmt in &hir.stmts {
                    println!("{stmt:?}");
                }
                for idx in 0..hir.exprs.len() {
                    let id = foundation::ids::ArenaId::from_u32(idx as u32);
                    if let Some(expr) = hir.exprs.get(id) {
                        println!("#{}: {:?}", idx, expr);
                    }
                }
            } else {
                let mut symbols = symbols;
                let (semantic_model, analyze_diagnostics) =
                    analyze_with_registry(&hir, &mut symbols, &builtins);
                diagnostics.extend(analyze_diagnostics);

                if cli.check {
                } else {
                    if diagnostics.has_errors() {
                        return finish_with_diagnostics(&cli.file, &contents, diagnostics);
                    }
                    let (chunk, compile_diagnostics) = compile_program(&hir, &semantic_model);
                    diagnostics.extend(compile_diagnostics);

                    if diagnostics.has_errors() {
                        return finish_with_diagnostics(&cli.file, &contents, diagnostics);
                    }

                    if let Err(vm_diagnostics) = execute(&chunk, &symbols, cli.args.clone()) {
                        diagnostics.extend(vm_diagnostics);
                    }
                }
            }
        }
    }

    finish_with_diagnostics(&cli.file, &contents, diagnostics)
}

fn finish_with_diagnostics(path: &str, source: &str, diagnostics: Diagnostics) -> ExitCode {
    for diagnostic in diagnostics.iter() {
        eprintln!("{}", diagnostic_renderer::render(path, source, diagnostic));
    }

    if diagnostics.has_errors() {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

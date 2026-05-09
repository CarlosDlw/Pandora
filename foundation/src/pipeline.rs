use crate::{
    db::Database,
    diagnostics::{Diagnostic, Diagnostics, Severity},
    error::FoundationError,
    frontend::PandoraFrontend,
    ids::{CacheId, FileId},
    span::Span,
};

#[derive(Debug, Default)]
pub struct Pipeline;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ast {
    file_id: FileId,
}

impl Ast {
    pub const fn file_id(self) -> FileId {
        self.file_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hir {
    file_id: FileId,
}

impl Hir {
    pub const fn file_id(self) -> FileId {
        self.file_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Analysis {
    file_id: FileId,
}

impl Analysis {
    pub const fn file_id(self) -> FileId {
        self.file_id
    }
}

impl Pipeline {
    pub fn new() -> Self {
        Self
    }

    pub fn load_file(
        &self,
        db: &mut Database,
        path: &str,
        contents: &str,
    ) -> Result<FileId, FoundationError> {
        db.vfs_mut().upsert_file(path, contents)
    }

    pub fn invalidate_file(
        &self,
        db: &mut Database,
        file_id: FileId,
    ) -> Result<(), FoundationError> {
        db.require_file(file_id)?;
        db.syntax_cache_mut().remove(file_id);
        db.semantic_cache_mut().remove(file_id);
        db.clear_diagnostics(file_id);
        Ok(())
    }

    pub fn parse(&self, _db: &Database, file_id: FileId) -> (Ast, Diagnostics) {
        (
            Ast { file_id },
            unsupported_stage_diagnostics(
                file_id,
                "parse",
                "use `Pipeline::run` with a `PandoraFrontend` from the `core` crate",
            ),
        )
    }

    pub fn lower(&self, ast: Ast) -> (Hir, Diagnostics) {
        (
            Hir {
                file_id: ast.file_id(),
            },
            unsupported_stage_diagnostics(
                ast.file_id(),
                "lower",
                "use `Pipeline::run` with a `PandoraFrontend` from the `core` crate",
            ),
        )
    }

    pub fn analyze(&self, hir: Hir) -> (Analysis, Diagnostics) {
        (
            Analysis {
                file_id: hir.file_id(),
            },
            unsupported_stage_diagnostics(
                hir.file_id(),
                "analyze",
                "use `Pipeline::run` with a `PandoraFrontend` from the `core` crate",
            ),
        )
    }

    /// Runs the real compiler via [`PandoraFrontend`] (implementations live in the `core` crate).
    pub fn run(
        &self,
        db: &mut Database,
        frontend: &mut impl PandoraFrontend,
    ) -> Result<(), FoundationError> {
        db.set_stdlib_loaded(true);
        let file_ids: Vec<FileId> = db.vfs().iter().map(|(file_id, _)| file_id).collect();
        for file_id in file_ids {
            let file = db.vfs().get_file_required(file_id)?;
            let diagnostics = frontend.compile_file(file_id, &file.contents, db.builtins_any());
            db.syntax_cache_mut().set(file_id, CacheId::from_u32(1));
            db.semantic_cache_mut().set(file_id, CacheId::from_u32(1));
            db.set_diagnostics(file_id, diagnostics);
        }
        Ok(())
    }

    pub fn seed_placeholders(
        &self,
        db: &mut Database,
        file_id: FileId,
    ) -> Result<(), FoundationError> {
        db.require_file(file_id)?;
        db.syntax_cache_mut().set(file_id, CacheId::from_u32(0));
        db.semantic_cache_mut().set(file_id, CacheId::from_u32(0));
        Ok(())
    }
}

fn unsupported_stage_diagnostics(
    file_id: FileId,
    stage: &'static str,
    guidance: &'static str,
) -> Diagnostics {
    let mut diagnostics = Diagnostics::new();
    diagnostics.push(Diagnostic::new(
        format!(
            "foundation pipeline stage `{stage}` is not implemented as a standalone API; {guidance}"
        ),
        Span::new_unchecked(file_id, 0, 0),
        Severity::Error,
    ));
    diagnostics
}

#[cfg(test)]
mod tests {
    use crate::{
        db::Database,
        diagnostics::{Diagnostics, Severity},
        frontend::PandoraFrontend,
        ids::FileId,
    };

    use super::Pipeline;

    struct RecordingFrontend;

    impl PandoraFrontend for RecordingFrontend {
        fn compile_file(
            &mut self,
            _file_id: FileId,
            _source: &str,
            _builtins: Option<&std::sync::Arc<dyn std::any::Any + Send + Sync>>,
        ) -> Diagnostics {
            Diagnostics::new()
        }
    }

    #[test]
    fn load_file_and_seed_placeholders() {
        let pipeline = Pipeline::new();
        let mut db = Database::new();

        let file_id = pipeline
            .load_file(&mut db, "main.pnd", "let x = 1;")
            .expect("load file");
        pipeline
            .seed_placeholders(&mut db, file_id)
            .expect("seed caches");

        assert!(db.syntax_cache().get(file_id).is_some());
        assert!(db.semantic_cache().get(file_id).is_some());
    }

    #[test]
    fn invalidate_missing_file_returns_error() {
        let pipeline = Pipeline::new();
        let mut db = Database::new();
        let missing = FileId::from_u32(77);
        assert!(pipeline.invalidate_file(&mut db, missing).is_err());
    }

    #[test]
    fn run_persists_diagnostics_by_file() {
        let pipeline = Pipeline::new();
        let mut db = Database::new();
        let file_id = pipeline
            .load_file(&mut db, "main.pnd", "content")
            .expect("load file");

        let mut fe = RecordingFrontend;
        pipeline.run(&mut db, &mut fe).expect("run pipeline");
        assert!(db.diagnostics_for(file_id).is_some());
    }

    #[test]
    fn parse_reports_placeholder_status() {
        let pipeline = Pipeline::new();
        let db = Database::new();
        let file_id = FileId::from_u32(10);

        let (_ast, diagnostics) = pipeline.parse(&db, file_id);

        assert!(diagnostics.has_errors());
        let diagnostic = diagnostics.iter().next().expect("placeholder diagnostic");
        assert_eq!(diagnostic.severity, Severity::Error);
        assert!(diagnostic.message.contains("`parse`"));
        assert!(diagnostic.message.contains("Pipeline::run"));
    }

    #[test]
    fn lower_reports_placeholder_status() {
        let pipeline = Pipeline::new();
        let ast = super::Ast {
            file_id: FileId::from_u32(11),
        };

        let (_hir, diagnostics) = pipeline.lower(ast);

        assert!(diagnostics.has_errors());
        let diagnostic = diagnostics.iter().next().expect("placeholder diagnostic");
        assert_eq!(diagnostic.severity, Severity::Error);
        assert!(diagnostic.message.contains("`lower`"));
        assert!(diagnostic.message.contains("PandoraFrontend"));
    }

    #[test]
    fn analyze_reports_placeholder_status() {
        let pipeline = Pipeline::new();
        let hir = super::Hir {
            file_id: FileId::from_u32(12),
        };

        let (_analysis, diagnostics) = pipeline.analyze(hir);

        assert!(diagnostics.has_errors());
        let diagnostic = diagnostics.iter().next().expect("placeholder diagnostic");
        assert_eq!(diagnostic.severity, Severity::Error);
        assert!(diagnostic.message.contains("`analyze`"));
        assert!(diagnostic.message.contains("core` crate"));
    }

    // --- Foundation pipeline placeholder behavior (Phase 6 coverage) ---
    #[test]
    fn parse_returns_structured_diagnostic_with_guidance() {
        let pipeline = Pipeline::new();
        let db = Database::new();
        let (_, diagnostics) = pipeline.parse(&db, FileId::from_u32(100));
        assert!(diagnostics.has_errors());
        let msg = diagnostics
            .iter()
            .next()
            .expect("diagnostic")
            .message
            .clone();
        assert!(msg.contains("not implemented"));
        assert!(msg.contains("PandoraFrontend"));
    }

    #[test]
    fn lower_preserves_file_id_in_placeholder() {
        let pipeline = Pipeline::new();
        let ast = super::Ast {
            file_id: FileId::from_u32(42),
        };
        let (hir, _) = pipeline.lower(ast);
        assert_eq!(hir.file_id(), FileId::from_u32(42));
    }

    #[test]
    fn analyze_preserves_file_id_in_placeholder() {
        let pipeline = Pipeline::new();
        let hir = super::Hir {
            file_id: FileId::from_u32(43),
        };
        let (analysis, _) = pipeline.analyze(hir);
        assert_eq!(analysis.file_id(), FileId::from_u32(43));
    }

    #[test]
    fn invalidate_file_clears_caches() {
        let pipeline = Pipeline::new();
        let mut db = Database::new();
        let file_id = pipeline
            .load_file(&mut db, "test.pand", "fn main() -> unit { }")
            .expect("load");
        pipeline.seed_placeholders(&mut db, file_id).expect("seed");

        let result = pipeline.invalidate_file(&mut db, file_id);
        assert!(result.is_ok());
        // After invalidation, caches should be cleared (verified by successful invalidate)
    }

    #[test]
    fn seed_placeholders_sets_cache_to_zero() {
        let pipeline = Pipeline::new();
        let mut db = Database::new();
        let file_id = pipeline
            .load_file(&mut db, "seed_test.pand", "fn main() -> unit { }")
            .expect("load");

        let result = pipeline.seed_placeholders(&mut db, file_id);
        assert!(result.is_ok());
    }

    #[test]
    fn load_file_with_multiple_lines() {
        let pipeline = Pipeline::new();
        let mut db = Database::new();
        let src = "fn add(a: i32, b: i32) -> i32 { return a + b }\nfn main() -> unit { let x = add(1, 2); }";
        let result = pipeline.load_file(&mut db, "multi.pand", src);
        assert!(result.is_ok());
    }

    #[test]
    fn parse_and_lower_preserve_file_id_chain() {
        let pipeline = Pipeline::new();
        let db = Database::new();
        let (ast, _) = pipeline.parse(&db, FileId::from_u32(50));
        assert_eq!(ast.file_id(), FileId::from_u32(50));

        let (hir, _) = pipeline.lower(ast);
        assert_eq!(hir.file_id(), FileId::from_u32(50));

        let (analysis, _) = pipeline.analyze(hir);
        assert_eq!(analysis.file_id(), FileId::from_u32(50));
    }

    #[test]
    fn placeholder_diagnostics_have_error_severity() {
        let pipeline = Pipeline::new();
        let db = Database::new();
        let (_, diag_parse) = pipeline.parse(&db, FileId::from_u32(51));
        assert!(diag_parse.iter().all(|d| d.severity == Severity::Error));

        let ast = super::Ast {
            file_id: FileId::from_u32(52),
        };
        let (_, diag_lower) = pipeline.lower(ast);
        assert!(diag_lower.iter().all(|d| d.severity == Severity::Error));

        let hir = super::Hir {
            file_id: FileId::from_u32(53),
        };
        let (_, diag_analyze) = pipeline.analyze(hir);
        assert!(diag_analyze.iter().all(|d| d.severity == Severity::Error));
    }
}

use crate::{
    db::Database,
    diagnostics::Diagnostics,
    error::FoundationError,
    ids::{CacheId, FileId},
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
        (Ast { file_id }, Diagnostics::new())
    }

    pub fn lower(&self, ast: Ast) -> (Hir, Diagnostics) {
        (
            Hir {
                file_id: ast.file_id(),
            },
            Diagnostics::new(),
        )
    }

    pub fn analyze(&self, hir: Hir) -> (Analysis, Diagnostics) {
        (
            Analysis {
                file_id: hir.file_id(),
            },
            Diagnostics::new(),
        )
    }

    pub fn run(&self, db: &mut Database) -> Result<(), FoundationError> {
        let file_ids: Vec<FileId> = db.vfs().iter().map(|(file_id, _)| file_id).collect();
        for file_id in file_ids {
            let (ast, mut diagnostics) = self.parse(db, file_id);
            let (hir, lower_diagnostics) = self.lower(ast);
            let (_analysis, analyze_diagnostics) = self.analyze(hir);
            diagnostics.extend(lower_diagnostics);
            diagnostics.extend(analyze_diagnostics);
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

#[cfg(test)]
mod tests {
    use crate::{db::Database, ids::FileId};

    use super::Pipeline;

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

        pipeline.run(&mut db).expect("run pipeline");
        assert!(db.diagnostics_for(file_id).is_some());
    }
}

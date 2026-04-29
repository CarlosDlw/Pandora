use std::collections::HashMap;
use std::sync::Arc;

use crate::{
    diagnostics::Diagnostics,
    error::FoundationError,
    ids::{CacheId, FileId},
    vfs::VirtualFileSystem,
};

#[derive(Debug, Default)]
pub struct SyntaxCache {
    entries: HashMap<FileId, CacheId>,
}

impl SyntaxCache {
    pub fn get(&self, file_id: FileId) -> Option<CacheId> {
        self.entries.get(&file_id).copied()
    }

    pub fn set(&mut self, file_id: FileId, cache_id: CacheId) {
        self.entries.insert(file_id, cache_id);
    }

    pub fn remove(&mut self, file_id: FileId) {
        self.entries.remove(&file_id);
    }
}

#[derive(Debug, Default)]
pub struct SemanticCache {
    entries: HashMap<FileId, CacheId>,
}

impl SemanticCache {
    pub fn get(&self, file_id: FileId) -> Option<CacheId> {
        self.entries.get(&file_id).copied()
    }

    pub fn set(&mut self, file_id: FileId, cache_id: CacheId) {
        self.entries.insert(file_id, cache_id);
    }

    pub fn remove(&mut self, file_id: FileId) {
        self.entries.remove(&file_id);
    }
}

#[derive(Default)]
pub struct Database {
    vfs: VirtualFileSystem,
    syntax_cache: SyntaxCache,
    semantic_cache: SemanticCache,
    diagnostics_by_file: HashMap<FileId, Diagnostics>,
    builtins: Option<Arc<dyn std::any::Any + Send + Sync>>,
    stdlib_loaded: bool,
}

impl Database {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn vfs(&self) -> &VirtualFileSystem {
        &self.vfs
    }

    pub fn vfs_mut(&mut self) -> &mut VirtualFileSystem {
        &mut self.vfs
    }

    pub fn syntax_cache(&self) -> &SyntaxCache {
        &self.syntax_cache
    }

    pub fn syntax_cache_mut(&mut self) -> &mut SyntaxCache {
        &mut self.syntax_cache
    }

    pub fn semantic_cache(&self) -> &SemanticCache {
        &self.semantic_cache
    }

    pub fn semantic_cache_mut(&mut self) -> &mut SemanticCache {
        &mut self.semantic_cache
    }

    pub fn require_file(&self, file_id: FileId) -> Result<(), FoundationError> {
        self.vfs.get_file_required(file_id).map(|_| ())
    }

    pub fn diagnostics_for(&self, file_id: FileId) -> Option<&Diagnostics> {
        self.diagnostics_by_file.get(&file_id)
    }

    pub fn set_diagnostics(&mut self, file_id: FileId, diagnostics: Diagnostics) {
        self.diagnostics_by_file.insert(file_id, diagnostics);
    }

    pub fn clear_diagnostics(&mut self, file_id: FileId) {
        self.diagnostics_by_file.remove(&file_id);
    }

    pub fn set_builtins_any(&mut self, builtins: Arc<dyn std::any::Any + Send + Sync>) {
        self.builtins = Some(builtins);
    }

    pub fn builtins_any(&self) -> Option<&Arc<dyn std::any::Any + Send + Sync>> {
        self.builtins.as_ref()
    }

    pub fn set_stdlib_loaded(&mut self, loaded: bool) {
        self.stdlib_loaded = loaded;
    }

    pub fn stdlib_loaded(&self) -> bool {
        self.stdlib_loaded
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        diagnostics::Diagnostics,
        ids::{CacheId, FileId},
    };

    use super::Database;

    #[test]
    fn require_file_validates_presence() {
        let mut db = Database::new();
        let file_id = db
            .vfs_mut()
            .upsert_file("main.pnd", "content")
            .expect("insert file");
        assert!(db.require_file(file_id).is_ok());
        assert!(db.require_file(FileId::from_u32(99)).is_err());
    }

    #[test]
    fn cache_wrappers_store_and_remove_values() {
        let mut db = Database::new();
        let file_id = FileId::from_u32(1);
        let cache_id = CacheId::from_u32(10);

        db.syntax_cache_mut().set(file_id, cache_id);
        assert_eq!(db.syntax_cache().get(file_id), Some(cache_id));
        db.syntax_cache_mut().remove(file_id);
        assert_eq!(db.syntax_cache().get(file_id), None);
    }

    #[test]
    fn stores_diagnostics_per_file() {
        let mut db = Database::new();
        let file_id = FileId::from_u32(3);
        db.set_diagnostics(file_id, Diagnostics::new());
        assert!(db.diagnostics_for(file_id).is_some());
        db.clear_diagnostics(file_id);
        assert!(db.diagnostics_for(file_id).is_none());
    }
}

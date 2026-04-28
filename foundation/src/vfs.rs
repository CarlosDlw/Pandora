use std::collections::HashMap;

use crate::{error::FoundationError, ids::FileId};

#[derive(Debug, Clone)]
pub struct VirtualFile {
    pub path: String,
    pub contents: String,
}

#[derive(Debug, Default)]
pub struct VirtualFileSystem {
    files: HashMap<FileId, VirtualFile>,
    path_index: HashMap<String, FileId>,
    next_id: u32,
}

impl VirtualFileSystem {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn upsert_file(
        &mut self,
        path: impl Into<String>,
        contents: impl Into<String>,
    ) -> Result<FileId, FoundationError> {
        let path = path.into();
        let contents = contents.into();

        if let Some(&file_id) = self.path_index.get(&path) {
            let file = self
                .files
                .get_mut(&file_id)
                .ok_or(FoundationError::InconsistentState(
                    "path index points to missing file",
                ))?;
            file.contents = contents;
            return Ok(file_id);
        }

        let file_id = FileId::from_u32(self.next_id);
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(FoundationError::IdExhausted { kind: "FileId" })?;

        self.files.insert(
            file_id,
            VirtualFile {
                path: path.clone(),
                contents,
            },
        );
        self.path_index.insert(path, file_id);

        Ok(file_id)
    }

    pub fn get_file(&self, file_id: FileId) -> Option<&VirtualFile> {
        self.files.get(&file_id)
    }

    pub fn get_file_by_path(&self, path: &str) -> Option<&VirtualFile> {
        let file_id = self.path_index.get(path)?;
        self.files.get(file_id)
    }

    pub fn get_file_required(&self, file_id: FileId) -> Result<&VirtualFile, FoundationError> {
        self.get_file(file_id).ok_or(FoundationError::FileNotFound)
    }

    pub fn iter(&self) -> impl Iterator<Item = (FileId, &VirtualFile)> {
        self.files.iter().map(|(id, file)| (*id, file))
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }
}

#[cfg(test)]
mod tests {
    use super::VirtualFileSystem;

    #[test]
    fn upsert_preserves_file_id_for_same_path() {
        let mut vfs = VirtualFileSystem::new();
        let first = vfs
            .upsert_file("src/main.pnd", "alpha")
            .expect("first upsert should succeed");
        let second = vfs
            .upsert_file("src/main.pnd", "beta")
            .expect("second upsert should succeed");

        assert_eq!(first, second);
        let file = vfs
            .get_file_by_path("src/main.pnd")
            .expect("file should exist");
        assert_eq!(file.contents, "beta");
    }

    #[test]
    fn stores_multiple_files() {
        let mut vfs = VirtualFileSystem::new();
        vfs.upsert_file("a.pnd", "one").expect("insert a");
        vfs.upsert_file("b.pnd", "two").expect("insert b");
        assert_eq!(vfs.len(), 2);
    }
}

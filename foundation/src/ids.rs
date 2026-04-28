use crate::error::FoundationError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ArenaId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CacheId(pub u32);

impl ArenaId {
    pub const MAX: u32 = u32::MAX;

    pub const fn from_u32(value: u32) -> Self {
        Self(value)
    }

    pub const fn as_u32(self) -> u32 {
        self.0
    }

    pub fn try_from_usize(value: usize) -> Result<Self, FoundationError> {
        let value = u32::try_from(value).map_err(|_| FoundationError::IdExhausted {
            kind: "ArenaId",
        })?;
        Ok(Self(value))
    }
}

impl FileId {
    pub const MAX: u32 = u32::MAX;

    pub const fn from_u32(value: u32) -> Self {
        Self(value)
    }

    pub const fn as_u32(self) -> u32 {
        self.0
    }

    pub fn try_from_usize(value: usize) -> Result<Self, FoundationError> {
        let value = u32::try_from(value).map_err(|_| FoundationError::IdExhausted {
            kind: "FileId",
        })?;
        Ok(Self(value))
    }
}

impl CacheId {
    pub const MAX: u32 = u32::MAX;

    pub const fn from_u32(value: u32) -> Self {
        Self(value)
    }

    pub const fn as_u32(self) -> u32 {
        self.0
    }

    pub fn try_from_usize(value: usize) -> Result<Self, FoundationError> {
        let value = u32::try_from(value).map_err(|_| FoundationError::IdExhausted {
            kind: "CacheId",
        })?;
        Ok(Self(value))
    }
}

#[cfg(test)]
mod tests {
    use super::{ArenaId, CacheId, FileId};

    #[test]
    fn ids_roundtrip_to_u32() {
        let arena = ArenaId::from_u32(7);
        let file = FileId::from_u32(9);
        let cache = CacheId::from_u32(11);
        assert_eq!(arena.as_u32(), 7);
        assert_eq!(file.as_u32(), 9);
        assert_eq!(cache.as_u32(), 11);
    }

    #[test]
    fn ids_convert_from_usize() {
        let id = ArenaId::try_from_usize(3).expect("should convert");
        assert_eq!(id.as_u32(), 3);
    }
}

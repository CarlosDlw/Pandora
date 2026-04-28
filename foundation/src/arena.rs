use crate::{error::FoundationError, ids::ArenaId};

#[derive(Debug, Default)]
pub struct Arena<T> {
    items: Vec<T>,
}

impl<T> Arena<T> {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn insert(&mut self, value: T) -> Result<ArenaId, FoundationError> {
        let id = ArenaId::try_from_usize(self.items.len())?;
        self.items.push(value);
        Ok(id)
    }

    pub fn get(&self, id: ArenaId) -> Option<&T> {
        self.items.get(id.as_u32() as usize)
    }

    pub fn get_mut(&mut self, id: ArenaId) -> Option<&mut T> {
        self.items.get_mut(id.as_u32() as usize)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::Arena;

    #[test]
    fn insert_returns_stable_ids() {
        let mut arena = Arena::new();
        let first = arena.insert("a").expect("insert should succeed");
        let second = arena.insert("b").expect("insert should succeed");
        assert_eq!(first.as_u32(), 0);
        assert_eq!(second.as_u32(), 1);
        assert_eq!(arena.get(first), Some(&"a"));
        assert_eq!(arena.get(second), Some(&"b"));
    }
}

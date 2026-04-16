use std::collections::HashMap;
use std::hash::Hash;

/// A set of items with reference counting for deduplication.
///
/// When multiple cells share the same style, they reference the same entry
/// in this set. Reference count increases on insert, decreases on removal.
/// Items with zero references are automatically removed.
#[derive(Debug, Clone)]
pub struct RefCountedSet<T: Clone + Eq + Hash> {
    /// The items and their (ref_count, stable_index) pairs.
    items: HashMap<T, (u32, usize)>,
    /// Next available stable index.
    next_index: usize,
}

impl<T: Clone + Eq + Hash> RefCountedSet<T> {
    /// Create a new empty RefCountedSet.
    pub fn new() -> Self {
        Self {
            items: HashMap::new(),
            next_index: 0,
        }
    }

    /// Insert an item and return its stable index. If the item already exists,
    /// just increment its reference count and return the existing index.
    ///
    /// Returns (index, is_new) where index is stable for this item across inserts.
    pub fn insert(&mut self, item: T) -> (usize, bool) {
        if let Some((count, idx)) = self.items.get_mut(&item) {
            // Item exists, increment ref count
            *count += 1;
            (*idx, false)
        } else {
            // New item, assign stable index
            let idx = self.next_index;
            self.next_index += 1;
            self.items.insert(item, (1, idx));
            (idx, true)
        }
    }

    /// Remove one reference to an item. If count reaches zero, the item is removed.
    /// Returns the new ref count, or None if the item was removed.
    pub fn remove(&mut self, item: &T) -> Option<u32> {
        match self.items.get_mut(item) {
            Some((count, _)) if *count > 1 => {
                *count -= 1;
                Some(*count)
            }
            Some((_, _)) => {
                // Count was 1, remove the item entirely
                self.items.remove(item);
                None
            }
            None => None,
        }
    }

    /// Remove an item by index. Returns the item if it existed and was removed.
    pub fn remove_by_index(&mut self, index: usize) -> Option<T> {
        // Find item with matching index
        let key = self.items.iter().find(|(_, (_, idx))| *idx == index).map(|(k, _)| k.clone())?;
        self.remove(&key);
        Some(key)
    }

    /// Get the reference count for an item.
    pub fn ref_count(&self, item: &T) -> u32 {
        self.items.get(item).map(|(c, _)| *c).unwrap_or(0)
    }

    /// Check if an item exists in the set.
    pub fn contains(&self, item: &T) -> bool {
        self.items.contains_key(item)
    }

    /// Get an item by index.
    pub fn get(&self, index: usize) -> Option<&T> {
        self.items.iter().find(|(_, (_, idx))| *idx == index).map(|(k, _)| k)
    }

    /// Get the index for an item if it exists.
    pub fn get_index(&self, item: &T) -> Option<usize> {
        self.items.get(item).map(|(_, idx)| *idx)
    }

    /// Get the number of unique items.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Check if empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Iterate over all items with their indices.
    pub fn iter(&self) -> impl Iterator<Item = (&T, usize)> {
        self.items.iter().map(|(k, (_, idx))| (k, *idx))
    }

    /// Clear all items and reset reference counts.
    pub fn clear(&mut self) {
        self.items.clear();
        self.next_index = 0;
    }

    /// Get total reference count across all items.
    pub fn total_refs(&self) -> u32 {
        self.items.values().map(|(c, _)| c).sum()
    }
}

impl<T: Clone + Eq + Hash> Default for RefCountedSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_and_find() {
        let mut set = RefCountedSet::new();
        let (idx1, is_new) = set.insert("hello".to_string());
        assert!(is_new);
        assert_eq!(idx1, 0);

        // Insert same item again
        let (idx2, is_new) = set.insert("hello".to_string());
        assert!(!is_new);
        assert_eq!(idx1, idx2);
        assert_eq!(set.ref_count(&"hello".to_string()), 2);
    }

    #[test]
    fn test_different_items() {
        let mut set = RefCountedSet::new();
        let (idx1, _) = set.insert("hello".to_string());
        let (idx2, _) = set.insert("world".to_string());
        assert_ne!(idx1, idx2);
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_remove() {
        let mut set = RefCountedSet::new();
        set.insert("hello".to_string());
        set.insert("hello".to_string());
        assert_eq!(set.ref_count(&"hello".to_string()), 2);

        set.remove(&"hello".to_string());
        assert_eq!(set.ref_count(&"hello".to_string()), 1);

        set.remove(&"hello".to_string());
        assert_eq!(set.ref_count(&"hello".to_string()), 0);
        assert!(!set.contains(&"hello".to_string()));
        assert_eq!(set.len(), 0);
    }

    #[test]
    fn test_get_by_index() {
        let mut set = RefCountedSet::new();
        let (idx, _) = set.insert("hello".to_string());
        assert_eq!(set.get(idx), Some(&"hello".to_string()));
        assert_eq!(set.get_index(&"hello".to_string()), Some(idx));
    }
}

use std::collections::HashSet;

/// Tracks which pane ids are part of the active broadcast group.
///
/// Pane key handler consults `BroadcastGroup` before deciding whether
/// to fan out keystrokes to all members. App-level wiring (toggle key
/// + key event fan-out) is layered on top of this data structure.
#[derive(Default, Debug, Clone)]
pub struct BroadcastGroup {
    members: HashSet<u64>,
    enabled: bool,
}

impl BroadcastGroup {
    /// Creates an empty, disabled group.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enables or disables broadcasting. Membership is preserved across
    /// toggles so the user can quickly turn fan-out on and off.
    pub fn enable(&mut self, on: bool) {
        self.enabled = on;
    }

    /// Returns whether broadcasting is currently active.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Adds a pane id to the broadcast set. Idempotent.
    pub fn add(&mut self, pane_id: u64) {
        self.members.insert(pane_id);
    }

    /// Removes a pane id from the broadcast set. Idempotent.
    pub fn remove(&mut self, pane_id: u64) {
        self.members.remove(&pane_id);
    }

    /// Toggles membership for `pane_id`, returning the new membership state
    /// (`true` if the id is now in the set).
    pub fn toggle(&mut self, pane_id: u64) -> bool {
        if self.members.contains(&pane_id) {
            self.members.remove(&pane_id);
            false
        } else {
            self.members.insert(pane_id);
            true
        }
    }

    /// Returns whether `pane_id` is part of the broadcast set.
    pub fn contains(&self, pane_id: u64) -> bool {
        self.members.contains(&pane_id)
    }

    /// Returns a snapshot of the current member ids.
    pub fn members(&self) -> Vec<u64> {
        self.members.iter().copied().collect()
    }

    /// Removes every member but preserves the enabled flag.
    pub fn clear(&mut self) {
        self.members.clear();
    }

    /// Number of pane ids currently in the broadcast set.
    pub fn len(&self) -> usize {
        self.members.len()
    }

    /// Returns whether the broadcast set has no members.
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_disabled_and_empty() {
        let g = BroadcastGroup::new();
        assert!(!g.is_enabled());
        assert_eq!(g.len(), 0);
        assert!(g.is_empty());
        assert!(g.members().is_empty());
    }

    #[test]
    fn add_and_contains() {
        let mut g = BroadcastGroup::new();
        g.add(7);
        g.add(42);
        g.add(7); // idempotent
        assert!(g.contains(7));
        assert!(g.contains(42));
        assert!(!g.contains(99));
        assert_eq!(g.len(), 2);
    }

    #[test]
    fn toggle_returns_new_membership() {
        let mut g = BroadcastGroup::new();
        assert!(g.toggle(1)); // added
        assert!(g.contains(1));
        assert!(!g.toggle(1)); // removed
        assert!(!g.contains(1));
        assert!(g.toggle(1)); // re-added
        assert!(g.contains(1));
    }

    #[test]
    fn remove_after_add_clears() {
        let mut g = BroadcastGroup::new();
        g.add(5);
        assert!(g.contains(5));
        g.remove(5);
        assert!(!g.contains(5));
        assert!(g.is_empty());
        // Removing a non-existent id is a no-op.
        g.remove(123);
        assert!(g.is_empty());
    }

    #[test]
    fn members_returns_sorted_or_unsorted_set() {
        let mut g = BroadcastGroup::new();
        g.add(3);
        g.add(1);
        g.add(2);
        let mut m = g.members();
        m.sort();
        assert_eq!(m, vec![1, 2, 3]);
        assert_eq!(m.len(), g.len());
    }

    #[test]
    fn clear_removes_all() {
        let mut g = BroadcastGroup::new();
        g.add(1);
        g.add(2);
        g.enable(true);
        g.clear();
        assert!(g.is_empty());
        // Enabled flag preserved across clear().
        assert!(g.is_enabled());
    }

    #[test]
    fn enable_toggles_flag() {
        let mut g = BroadcastGroup::new();
        assert!(!g.is_enabled());
        g.enable(true);
        assert!(g.is_enabled());
        g.enable(false);
        assert!(!g.is_enabled());
    }
}

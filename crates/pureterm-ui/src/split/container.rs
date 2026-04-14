/// Direction of a split.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Horizontal,
    Vertical,
}

/// Direction of a split relative to the current pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitSide {
    /// New pane to the right of current (Direction::Vertical, insert after)
    Right,
    /// New pane to the left of current (Direction::Vertical, insert before)
    Left,
    /// New pane below current (Direction::Horizontal, insert after)
    Down,
    /// New pane above current (Direction::Horizontal, insert before)
    Up,
}

impl SplitSide {
    /// Get the layout direction for this split side.
    pub fn direction(&self) -> Direction {
        match self {
            SplitSide::Right | SplitSide::Left => Direction::Vertical,
            SplitSide::Down | SplitSide::Up => Direction::Horizontal,
        }
    }

    /// Whether the new pane should be inserted before the target.
    pub fn insert_before(&self) -> bool {
        matches!(self, SplitSide::Left | SplitSide::Up)
    }
}

/// A child element in a split container.
#[derive(Debug)]
pub enum SplitChild {
    Container(Box<SplitContainer>),
    Pane(u64), // Pane ID
}

/// A container that holds multiple panes split in a direction.
#[derive(Debug)]
pub struct SplitContainer {
    pub direction: Direction,
    pub children: Vec<SplitChild>,
    pub ratios: Vec<f32>,
    pub id: u64,
}

use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_SPLIT_ID: AtomicU64 = AtomicU64::new(1);

fn next_split_id() -> u64 {
    NEXT_SPLIT_ID.fetch_add(1, Ordering::SeqCst)
}

impl SplitContainer {
    pub fn new(direction: Direction) -> Self {
        Self {
            direction,
            children: Vec::new(),
            ratios: Vec::new(),
            id: next_split_id(),
        }
    }

    /// Create a split container with a single pane.
    pub fn single(pane_id: u64) -> Self {
        let mut container = Self::new(Direction::Horizontal);
        container.add_pane(pane_id, 1.0);
        container
    }

    /// Add a pane to this container.
    pub fn add_pane(&mut self, pane_id: u64, ratio: f32) {
        self.children.push(SplitChild::Pane(pane_id));
        self.ratios.push(ratio);
        self.normalize_ratios();
    }

    /// Split an existing pane in the given direction.
    pub fn split_pane(&mut self, pane_id: u64, new_pane_id: u64, direction: Direction) -> bool {
        for i in 0..self.children.len() {
            match &mut self.children[i] {
                SplitChild::Container(container) => {
                    if container.split_pane(pane_id, new_pane_id, direction) {
                        return true;
                    }
                }
                SplitChild::Pane(id) if *id == pane_id => {
                    if direction == self.direction {
                        // Same direction - just insert next to it
                        self.children.insert(i + 1, SplitChild::Pane(new_pane_id));
                        self.ratios.insert(i + 1, self.ratios[i] / 2.0);
                        self.ratios[i] /= 2.0;
                    } else {
                        // Perpendicular - create a nested container
                        let old_pane = std::mem::replace(
                            &mut self.children[i],
                            SplitChild::Container(Box::new(SplitContainer::new(direction))),
                        );
                        if let SplitChild::Pane(old_id) = old_pane {
                            if let SplitChild::Container(ref mut container) = self.children[i] {
                                container.add_pane(old_id, 0.5);
                                container.add_pane(new_pane_id, 0.5);
                            }
                        }
                    }
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    /// Split an existing pane with directional control (up/down/left/right).
    pub fn split_pane_directional(&mut self, pane_id: u64, new_pane_id: u64, side: SplitSide) -> bool {
        let direction = side.direction();
        let insert_before = side.insert_before();

        for i in 0..self.children.len() {
            match &mut self.children[i] {
                SplitChild::Container(container) => {
                    if container.split_pane_directional(pane_id, new_pane_id, side) {
                        return true;
                    }
                }
                SplitChild::Pane(id) if *id == pane_id => {
                    if direction == self.direction {
                        // Same direction - insert adjacent
                        if insert_before {
                            self.children.insert(i, SplitChild::Pane(new_pane_id));
                            let half = self.ratios[i] / 2.0;
                            self.ratios.insert(i, half);
                            self.ratios[i + 1] = half;
                        } else {
                            self.children.insert(i + 1, SplitChild::Pane(new_pane_id));
                            let half = self.ratios[i] / 2.0;
                            self.ratios[i] = half;
                            self.ratios.insert(i + 1, half);
                        }
                    } else {
                        // Perpendicular - create a nested container
                        let old_pane = std::mem::replace(
                            &mut self.children[i],
                            SplitChild::Container(Box::new(SplitContainer::new(direction))),
                        );
                        if let SplitChild::Pane(old_id) = old_pane {
                            if let SplitChild::Container(ref mut container) = self.children[i] {
                                if insert_before {
                                    container.add_pane(new_pane_id, 0.5);
                                    container.add_pane(old_id, 0.5);
                                } else {
                                    container.add_pane(old_id, 0.5);
                                    container.add_pane(new_pane_id, 0.5);
                                }
                            }
                        }
                    }
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    /// Close a pane and return its ID.
    pub fn close_pane(&mut self, pane_id: u64) -> Option<u64> {
        for i in 0..self.children.len() {
            match &mut self.children[i] {
                SplitChild::Container(container) => {
                    if let Some(id) = container.close_pane(pane_id) {
                        // Remove empty containers
                        if container.children.is_empty() {
                            self.children.remove(i);
                            self.ratios.remove(i);
                            self.normalize_ratios();
                        } else if container.children.len() == 1 {
                            // Collapse single-child container into the parent
                            let only_child = container.children.pop().unwrap();
                            self.children[i] = only_child;
                            // ratios stay the same — single child inherits parent's ratio
                        }
                        return Some(id);
                    }
                }
                SplitChild::Pane(id) if *id == pane_id => {
                    self.children.remove(i);
                    self.ratios.remove(i);
                    self.normalize_ratios();
                    return Some(pane_id);
                }
                _ => {}
            }
        }
        None
    }

    /// Find a pane by ID.
    pub fn find_pane(&self, pane_id: u64) -> Option<&u64> {
        for child in &self.children {
            match child {
                SplitChild::Pane(id) if *id == pane_id => return Some(id),
                SplitChild::Container(container) => {
                    if let Some(id) = container.find_pane(pane_id) {
                        return Some(id);
                    }
                }
                _ => {}
            }
        }
        None
    }

    /// Find a container by ID (mutable).
    pub fn find_container_mut(&mut self, id: u64) -> Option<&mut SplitContainer> {
        if self.id == id {
            return Some(self);
        }
        for child in &mut self.children {
            if let SplitChild::Container(container) = child {
                if let Some(found) = container.find_container_mut(id) {
                    return Some(found);
                }
            }
        }
        None
    }

    /// Get all pane IDs in this container.
    pub fn pane_ids(&self) -> Vec<u64> {
        let mut ids = Vec::new();
        for child in &self.children {
            match child {
                SplitChild::Pane(id) => ids.push(*id),
                SplitChild::Container(container) => ids.extend(container.pane_ids()),
            }
        }
        ids
    }

    /// Equalize all ratios.
    pub fn equalize(&mut self) {
        if self.children.is_empty() {
            return;
        }
        let ratio = 1.0 / self.children.len() as f32;
        self.ratios = vec![ratio; self.children.len()];
        for child in &mut self.children {
            if let SplitChild::Container(container) = child {
                container.equalize();
            }
        }
    }

    /// Adjust the ratio between two adjacent panes.
    pub fn adjust_ratio(&mut self, index: usize, delta: f32) {
        if index + 1 >= self.ratios.len() {
            return;
        }
        let new_ratio = self.ratios[index] + delta;
        if new_ratio > 0.1 && self.ratios[index + 1] - delta > 0.1 {
            self.ratios[index] = new_ratio;
            self.ratios[index + 1] -= delta;
        }
    }

    fn normalize_ratios(&mut self) {
        if self.ratios.is_empty() {
            return;
        }
        let total: f32 = self.ratios.iter().sum();
        if total > 0.0 {
            for r in &mut self.ratios {
                *r /= total;
            }
        }
    }

    /// Get the number of panes.
    pub fn pane_count(&self) -> usize {
        self.children.iter().map(|c| match c {
            SplitChild::Pane(_) => 1,
            SplitChild::Container(container) => container.pane_count(),
        }).sum()
    }
}

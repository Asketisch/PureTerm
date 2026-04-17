/// 分割的方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Horizontal,
    Vertical,
}

/// 相对于当前面板的分割方向。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitSide {
    /// 当前右侧的新面板(Direction::Vertical, 在之后插入)
    Right,
    /// 当前左侧的新面板(Direction::Vertical, 在之前插入)
    Left,
    /// 当前下方的新面板(Direction::Horizontal, 在之后插入)
    Down,
    /// 当前上方的新面板(Direction::Horizontal, 在之前插入)
    Up,
}

impl SplitSide {
    /// 获取此分割侧的布局方向。
    pub fn direction(&self) -> Direction {
        match self {
            SplitSide::Right | SplitSide::Left => Direction::Vertical,
            SplitSide::Down | SplitSide::Up => Direction::Horizontal,
        }
    }

    /// 新面板是否应该插入到目标之前。
    pub fn insert_before(&self) -> bool {
        matches!(self, SplitSide::Left | SplitSide::Up)
    }
}

/// 分割容器中的子元素。
#[derive(Debug)]
pub enum SplitChild {
    Container(Box<SplitContainer>),
    Pane(u64), // Pane ID
}

/// 在一个方向上持有多个分割面板的容器。
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

    /// 创建带有单个面板的分割容器。
    pub fn single(pane_id: u64) -> Self {
        let mut container = Self::new(Direction::Horizontal);
        container.add_pane(pane_id, 1.0);
        container
    }

    /// 添加面板到此容器。
    pub fn add_pane(&mut self, pane_id: u64, ratio: f32) {
        self.children.push(SplitChild::Pane(pane_id));
        self.ratios.push(ratio);
        self.normalize_ratios();
    }

    /// 在给定方向上分割现有面板。
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
                        // 相同方向 - 只是插入到旁边
                        self.children.insert(i + 1, SplitChild::Pane(new_pane_id));
                        self.ratios.insert(i + 1, self.ratios[i] / 2.0);
                        self.ratios[i] /= 2.0;
                    } else {
                        // 垂直 - 创建嵌套容器
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

    /// 使用方向控制分割现有面板(上/下/左/右)。
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
                        // 相同方向 - 相邻插入
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
                        // 垂直 - 创建嵌套容器
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

    /// 关闭面板并返回其 ID。
    pub fn close_pane(&mut self, pane_id: u64) -> Option<u64> {
        for i in 0..self.children.len() {
            match &mut self.children[i] {
                SplitChild::Container(container) => {
                    if let Some(id) = container.close_pane(pane_id) {
                        // 移除空容器
                        if container.children.is_empty() {
                            self.children.remove(i);
                            self.ratios.remove(i);
                            self.normalize_ratios();
                        } else if container.children.len() == 1 {
                            // 将单子容器折叠到父容器中
                            let only_child = container.children.pop().unwrap();
                            self.children[i] = only_child;
                            // ratios 保持不变 — 单个子项继承父项的 ratio
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

    /// 通过 ID 查找面板。
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

    /// 通过 ID 查找容器(可变)。
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

    /// 获取此容器中的所有面板 ID。
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

    /// 使所有比率相等。
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

    /// 调整两个相邻面板之间的比率。
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

    /// 获取面板数量。
    pub fn pane_count(&self) -> usize {
        self.children.iter().map(|c| match c {
            SplitChild::Pane(_) => 1,
            SplitChild::Container(container) => container.pane_count(),
        }).sum()
    }
}

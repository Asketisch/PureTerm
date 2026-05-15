//! Workgroups — saved tab/split arrangements.
//!
//! A pure data model representing a named arrangement of tabs, where each
//! tab carries a tree of split nodes. Leaves reference an optional profile
//! id used to spawn the pane; containers carry a direction + children +
//! ratios. Conversion to/from the runtime `SplitContainer` (in pureterm-ui)
//! is intentionally a separate concern.

use serde::{Deserialize, Serialize};

/// A serializable representation of a single split node — leaf carries
/// the profile-id used to spawn the pane; container carries direction +
/// children + ratios.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum SplitNode {
    Leaf {
        profile_id: Option<String>,
        title: String,
    },
    Container {
        direction: SplitDir,
        children: Vec<SplitNode>,
        ratios: Vec<f32>,
    },
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum SplitDir {
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TabState {
    pub title: String,
    pub root: SplitNode,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Workgroup {
    pub id: String,
    pub name: String,
    pub tabs: Vec<TabState>,
}

/// In-memory store for workgroups. Persistence is handled by `AppConfig`.
#[derive(Clone, Debug, Default)]
pub struct WorkgroupStore {
    workgroups: Vec<Workgroup>,
}

impl WorkgroupStore {
    pub fn new(workgroups: Vec<Workgroup>) -> Self {
        Self { workgroups }
    }

    pub fn list(&self) -> &[Workgroup] {
        &self.workgroups
    }

    pub fn add(&mut self, wg: Workgroup) {
        self.workgroups.push(wg);
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let before = self.workgroups.len();
        self.workgroups.retain(|w| w.id != id);
        self.workgroups.len() != before
    }

    pub fn get(&self, id: &str) -> Option<&Workgroup> {
        self.workgroups.iter().find(|w| w.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_leaf() -> SplitNode {
        SplitNode::Leaf {
            profile_id: Some("default".into()),
            title: "shell".into(),
        }
    }

    fn sample_workgroup(id: &str) -> Workgroup {
        Workgroup {
            id: id.into(),
            name: format!("wg-{id}"),
            tabs: vec![TabState {
                title: "tab1".into(),
                root: sample_leaf(),
            }],
        }
    }

    #[test]
    fn add_and_list_workgroups() {
        let mut store = WorkgroupStore::new(Vec::new());
        assert!(store.list().is_empty());
        store.add(sample_workgroup("a"));
        store.add(sample_workgroup("b"));
        let listed = store.list();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, "a");
        assert_eq!(listed[1].id, "b");
    }

    #[test]
    fn remove_returns_true_for_existing() {
        let mut store = WorkgroupStore::new(vec![sample_workgroup("a"), sample_workgroup("b")]);
        assert!(store.remove("a"));
        assert_eq!(store.list().len(), 1);
        assert_eq!(store.list()[0].id, "b");
        // Removing again returns false.
        assert!(!store.remove("a"));
    }

    #[test]
    fn get_returns_none_for_unknown() {
        let store = WorkgroupStore::new(vec![sample_workgroup("a")]);
        assert!(store.get("missing").is_none());
        assert!(store.get("a").is_some());
    }

    #[test]
    fn serde_roundtrip_workgroup_with_nested_splits() {
        let wg = Workgroup {
            id: "main".into(),
            name: "Main Layout".into(),
            tabs: vec![TabState {
                title: "edit".into(),
                root: SplitNode::Container {
                    direction: SplitDir::Horizontal,
                    ratios: vec![0.5, 0.5],
                    children: vec![
                        SplitNode::Leaf {
                            profile_id: Some("editor".into()),
                            title: "vim".into(),
                        },
                        SplitNode::Container {
                            direction: SplitDir::Vertical,
                            ratios: vec![0.7, 0.3],
                            children: vec![
                                SplitNode::Leaf {
                                    profile_id: None,
                                    title: "shell".into(),
                                },
                                SplitNode::Leaf {
                                    profile_id: Some("logs".into()),
                                    title: "logs".into(),
                                },
                            ],
                        },
                    ],
                },
            }],
        };
        let json = serde_json::to_string(&wg).expect("serialize");
        let back: Workgroup = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.id, "main");
        assert_eq!(back.tabs.len(), 1);
        match &back.tabs[0].root {
            SplitNode::Container { direction, children, ratios } => {
                assert_eq!(*direction, SplitDir::Horizontal);
                assert_eq!(children.len(), 2);
                assert_eq!(ratios, &vec![0.5, 0.5]);
            }
            _ => panic!("expected container at root"),
        }
    }

    #[test]
    fn serde_roundtrip_leaf_only() {
        let wg = Workgroup {
            id: "solo".into(),
            name: "Solo".into(),
            tabs: vec![TabState {
                title: "only".into(),
                root: SplitNode::Leaf {
                    profile_id: None,
                    title: "only".into(),
                },
            }],
        };
        let toml = toml::to_string(&wg).expect("serialize toml");
        let back: Workgroup = toml::from_str(&toml).expect("deserialize toml");
        assert_eq!(back.id, "solo");
        match &back.tabs[0].root {
            SplitNode::Leaf { profile_id, title } => {
                assert!(profile_id.is_none());
                assert_eq!(title, "only");
            }
            _ => panic!("expected leaf"),
        }
    }
}

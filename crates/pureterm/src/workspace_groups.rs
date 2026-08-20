//! GAP-26: 工作区分组 —— 左侧栏的树形分组。

use serde::{Deserialize, Serialize};

/// `pureterm.toml` 里的一条 `[[workspace_groups]]` 配置。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceGroup {
    pub name: String,
    #[serde(default)]
    pub workspaces: Vec<String>,
    #[serde(default = "default_expanded")]
    pub expanded: bool,
}

fn default_expanded() -> bool {
    true
}

/// 解析某个工作区名称从属于哪个分组(若存在)。
pub fn group_for_workspace<'a>(
    groups: &'a [WorkspaceGroup],
    workspace_name: &str,
) -> Option<&'a WorkspaceGroup> {
    groups
        .iter()
        .find(|g| g.workspaces.iter().any(|w| w == workspace_name))
}

/// 未列入任何分组的工作区归入 "Ungrouped"。
pub fn ungrouped_workspaces(groups: &[WorkspaceGroup], all_names: &[String]) -> Vec<String> {
    let grouped: std::collections::HashSet<&str> = groups
        .iter()
        .flat_map(|g| g.workspaces.iter().map(String::as_str))
        .collect();
    all_names
        .iter()
        .filter(|n| !grouped.contains(n.as_str()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ungrouped_lists_remainder() {
        let groups = vec![WorkspaceGroup {
            name: "Work".into(),
            workspaces: vec!["dev".into()],
            expanded: true,
        }];
        let all = vec!["dev".into(), "personal".into()];
        assert_eq!(ungrouped_workspaces(&groups, &all), vec!["personal".to_string()]);
    }
}

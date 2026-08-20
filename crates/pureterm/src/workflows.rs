//! GAP-52:工作流 —— 带别名与分类的参数化命令序列。
//!
//! 结构(`[[workflows]]`,位于 `alacritty.toml`):
//!
//! ```toml
//! [[workflows]]
//! name = "deploy"
//! description = "Deploy to env"
//! category = "ops"            # optional, used for palette grouping
//! alias = "d"                 # optional, for `pureterm workflow d`
//! steps = ["ssh {{host}} 'cd /app && git pull'"]
//! params = ["host"]
//! ```
//!
//! 解析规则:
//! - 按名称或别名查找 `Workflow` 时,**第一个**匹配胜出。名称优先于别名
//!   (因此按 `d` 查找时,字面名为 `d` 的工作流更优先)。
//! - `render_steps` 用 `--param key=value` 传入的值替换 `{{key}}` 占位符。
//!   缺失的值保留原位,使未完成的翻译保持可读。
//! - `missing_params` 报告哪些已声明的 `params` 未给出值,以便 CLI 提示
//!   用户,而不是运行残缺的步骤。

use serde::{Deserialize, Serialize};

/// `[[workflows]]` 条目。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workflow {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub steps: Vec<String>,
    #[serde(default)]
    pub params: Vec<String>,
    /// 可选的分组(例如 `"ops"`、`"git"`、`"docker"`),用于命令面板分组。
    #[serde(default)]
    pub category: String,
    /// 可选的短别名。`pureterm workflow <alias>` 会在没有工作流把该别名
    /// 作为字面名时解析到这里。
    #[serde(default)]
    pub alias: String,
}

impl Workflow {
    /// 展开每个步骤中的 `{{param}}` 占位符。
    pub fn render_steps(&self, values: &[(String, String)]) -> Vec<String> {
        self.steps
            .iter()
            .map(|step| {
                let mut s = step.clone();
                for (k, v) in values {
                    s = s.replace(&format!("{{{{{k}}}}}"), v);
                }
                s
            })
            .collect()
    }

    /// 已声明但因未在 `values` 中提供而缺失的 `params` 名称。
    /// 输出保持声明顺序。
    pub fn missing_params(&self, values: &[(String, String)]) -> Vec<String> {
        self.params
            .iter()
            .filter(|p| !values.iter().any(|(k, _)| k == *p))
            .cloned()
            .collect()
    }

    /// 该工作流是否有非空别名?
    pub fn has_alias(&self) -> bool {
        !self.alias.is_empty()
    }

    /// 给定 `query` 是否匹配该工作流 —— 按名称或别名?
    /// 名称优先(调用方应迭代找到第一个名称匹配,再回退到别名)。
    pub fn matches(&self, query: &str) -> bool {
        self.name == query || (self.has_alias() && self.alias == query)
    }
}

/// `[workflows]` 分节开关。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowsConfig {
    pub enabled: bool,
}

impl Default for WorkflowsConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// 从原始 TOML 配置字符串加载 `[[workflows]]` 条目。坏的工作流条目被
/// 静默跳过(CLI / 命令面板仍可展示解析出的内容)。
pub fn load_workflows_from_toml(raw: &str) -> Vec<Workflow> {
    #[derive(Deserialize)]
    struct Wrap {
        #[serde(default)]
        workflows: Vec<Workflow>,
    }
    toml::from_str::<Wrap>(raw)
        .map(|w| w.workflows)
        .unwrap_or_default()
}

/// 按名称或别名查找工作流。返回第一个匹配。当两个工作流都可能匹配时
/// (例如一个 `name = "d"`、另一个 `alias = "d"`),名称匹配胜出。
pub fn find<'a>(workflows: &'a [Workflow], query: &str) -> Option<&'a Workflow> {
    workflows.iter().find(|w| w.name == query).or_else(|| {
        workflows.iter().find(|w| w.alias == query)
    })
}

/// 按 `category` 字段对工作流分组。category 为空的工作流归到空字符串下
/// (调用方可以替换为本地化标签,如 "Uncategorized" / "未分类")。
pub fn group_by_category(workflows: &[Workflow]) -> std::collections::BTreeMap<String, Vec<&Workflow>> {
    let mut out: std::collections::BTreeMap<String, Vec<&Workflow>> =
        std::collections::BTreeMap::new();
    for w in workflows {
        out.entry(w.category.clone()).or_default().push(w);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_params() {
        let wf = Workflow {
            name: "deploy".into(),
            description: String::new(),
            steps: vec!["ssh {{host}} 'deploy {{env}}'".into()],
            params: vec!["host".into(), "env".into()],
            category: String::new(),
            alias: String::new(),
        };
        let out = wf.render_steps(&[
            ("host".into(), "prod".into()),
            ("env".into(), "staging".into()),
        ]);
        assert_eq!(out[0], "ssh prod 'deploy staging'");
    }

    #[test]
    fn missing_params_reports_declaration_order() {
        let wf = Workflow {
            name: "x".into(),
            description: String::new(),
            steps: vec!["echo {{a}} {{b}} {{c}}".into()],
            params: vec!["a".into(), "b".into(), "c".into()],
            category: String::new(),
            alias: String::new(),
        };
        let missing = wf.missing_params(&[("b".into(), "B".into())]);
        assert_eq!(missing, vec!["a", "c"]);
    }

    #[test]
    fn matches_by_name_and_alias() {
        let wf = Workflow {
            name: "deploy".into(),
            description: String::new(),
            steps: vec![],
            params: vec![],
            category: String::new(),
            alias: "d".into(),
        };
        assert!(wf.matches("deploy"));
        assert!(wf.matches("d"));
        assert!(!wf.matches("x"));
    }

    #[test]
    fn find_prefers_name_over_alias() {
        let a = Workflow {
            name: "d".into(),
            description: "by-name".into(),
            steps: vec![],
            params: vec![],
            category: String::new(),
            alias: String::new(),
        };
        let b = Workflow {
            name: "deploy".into(),
            description: "by-alias".into(),
            steps: vec![],
            params: vec![],
            category: String::new(),
            alias: "d".into(),
        };
        let v = vec![a, b];
        let resolved = find(&v, "d").unwrap();
        assert_eq!(resolved.description, "by-name");
    }

    #[test]
    fn group_by_category_splits() {
        let a = Workflow {
            name: "a".into(),
            description: String::new(),
            steps: vec![],
            params: vec![],
            category: "ops".into(),
            alias: String::new(),
        };
        let b = Workflow {
            name: "b".into(),
            description: String::new(),
            steps: vec![],
            params: vec![],
            category: "git".into(),
            alias: String::new(),
        };
        let c = Workflow {
            name: "c".into(),
            description: String::new(),
            steps: vec![],
            params: vec![],
            category: "ops".into(),
            alias: String::new(),
        };
        let v = vec![a, b, c];
        let grouped = group_by_category(&v);
        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped.get("ops").unwrap().len(), 2);
        assert_eq!(grouped.get("git").unwrap().len(), 1);
    }

    #[test]
    fn load_skips_malformed_toml_gracefully() {
        // 畸形:`steps` 是字符串而非数组。
        let raw = "[[workflows]]\nname = \"x\"\nsteps = \"not an array\"\n";
        let v = load_workflows_from_toml(raw);
        assert!(v.is_empty());
    }

    #[test]
    fn alias_round_trip_via_toml() {
        let raw = r#"
[[workflows]]
name = "deploy"
alias = "d"
category = "ops"
steps = []
params = []
"#;
        let v = load_workflows_from_toml(raw);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].alias, "d");
        assert_eq!(v[0].category, "ops");
    }
}

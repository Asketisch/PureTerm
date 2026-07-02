//! GAP-52: Workflows — parameterized command sequences with aliases & categories.
//!
//! Schema (`[[workflows]]` in `alacritty.toml`):
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
//! Resolution rules:
//! - When a `Workflow` is looked up by name or alias, the **first** match
//!   wins. Names take priority over aliases (so a workflow literally
//!   named `d` is preferred when looking up `d`).
//! - `render_steps` substitutes `{{key}}` placeholders with the values
//!   passed via `--param key=value`. Missing values are left in place so
//!   unfinished translations remain readable.
//! - `missing_params` reports which declared `params` were not given a
//!   value, so the CLI can prompt the user instead of running broken
//!   steps.

use serde::{Deserialize, Serialize};

/// `[[workflows]]` entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workflow {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub steps: Vec<String>,
    #[serde(default)]
    pub params: Vec<String>,
    /// Optional palette grouping (e.g. `"ops"`, `"git"`, `"docker"`).
    #[serde(default)]
    pub category: String,
    /// Optional short alias. `pureterm workflow <alias>` resolves here
    /// when no workflow has the alias as its literal name.
    #[serde(default)]
    pub alias: String,
}

impl Workflow {
    /// Expand `{{param}}` placeholders in each step.
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

    /// Names of declared `params` that were not supplied in `values`.
    /// The output preserves declaration order.
    pub fn missing_params(&self, values: &[(String, String)]) -> Vec<String> {
        self.params
            .iter()
            .filter(|p| !values.iter().any(|(k, _)| k == *p))
            .cloned()
            .collect()
    }

    /// Does this workflow have a non-empty alias?
    pub fn has_alias(&self) -> bool {
        !self.alias.is_empty()
    }

    /// Does the given `query` match this workflow — by name or alias?
    /// Names take priority (callers should iterate to find the first
    /// name match, then fall back to alias).
    pub fn matches(&self, query: &str) -> bool {
        self.name == query || (self.has_alias() && self.alias == query)
    }
}

/// `[workflows]` section toggle.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowsConfig {
    pub enabled: bool,
}

impl Default for WorkflowsConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

/// Load `[[workflows]]` entries from a raw TOML config string. Bad
/// workflow entries are skipped silently (the CLI / palette can still
/// show whatever parsed).
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

/// Find a workflow by name or alias. Returns the first match. When two
/// workflows could match (e.g. one with `name = "d"` and another with
/// `alias = "d"`), the name match wins.
pub fn find<'a>(workflows: &'a [Workflow], query: &str) -> Option<&'a Workflow> {
    workflows.iter().find(|w| w.name == query).or_else(|| {
        workflows.iter().find(|w| w.alias == query)
    })
}

/// Group workflows by their `category` field. Workflows with an empty
/// category are placed under the empty string (callers can replace it
/// with a localized label like "Uncategorized" / "未分类").
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
        // Malformed: `steps` is a string instead of an array.
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

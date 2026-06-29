//! GAP-52: Workflows — parameterized command sequences.

use serde::{Deserialize, Serialize};

/// `[[workflows]]` entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workflow {
    pub name: String,
    pub description: String,
    pub steps: Vec<String>,
    #[serde(default)]
    pub params: Vec<String>,
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
}

/// `[workflows]` section toggle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowsConfig {
    pub enabled: bool,
}

impl Default for WorkflowsConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

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
        };
        let out = wf.render_steps(&[
            ("host".into(), "prod".into()),
            ("env".into(), "staging".into()),
        ]);
        assert_eq!(out[0], "ssh prod 'deploy staging'");
    }
}

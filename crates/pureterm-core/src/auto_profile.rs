//! 基于工作目录或 SSH 主机的自动配置文件切换。

use crate::profile::Profile;

/// 自动切换配置文件的规则。
#[derive(Debug, Clone)]
pub struct AutoProfileRule {
    /// 要匹配的模式（目录路径前缀或主机名）。
    pub pattern: String,
    /// 要切换到的配置文件 ID。
    pub profile_id: String,
    /// 匹配类型：目录或主机名。
    pub match_type: MatchType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchType {
    /// 与当前工作目录匹配。
    Directory,
    /// 与 SSH 主机名匹配。
    Hostname,
}

/// 基于上下文自动切换配置文件的服务。
pub struct AutoProfileSwitch {
    rules: Vec<AutoProfileRule>,
}

impl AutoProfileSwitch {
    pub fn new() -> Self {
        Self { rules: Vec::new() }
    }

    /// 添加切换规则。
    pub fn add_rule(&mut self, rule: AutoProfileRule) {
        self.rules.push(rule);
    }

    /// 按模式移除规则。
    pub fn remove_rule(&mut self, pattern: &str) {
        self.rules.retain(|r| r.pattern != pattern);
    }

    /// 查找给定目录的匹配配置文件 ID。
    pub fn match_directory<'a>(&self, cwd: &str, profiles: &'a [Profile]) -> Option<&'a Profile> {
        let profile_id = self.rules.iter()
            .filter(|r| r.match_type == MatchType::Directory)
            .find(|r| cwd.starts_with(&r.pattern))?
            .profile_id
            .clone();

        profiles.iter().find(|p| p.id == profile_id)
    }

    /// 查找给定主机名的匹配配置文件 ID。
    pub fn match_hostname<'a>(&self, hostname: &str, profiles: &'a [Profile]) -> Option<&'a Profile> {
        let profile_id = self.rules.iter()
            .filter(|r| r.match_type == MatchType::Hostname)
            .find(|r| hostname == r.pattern)?
            .profile_id
            .clone();

        profiles.iter().find(|p| p.id == profile_id)
    }

    /// 获取所有规则。
    pub fn rules(&self) -> &[AutoProfileRule] {
        &self.rules
    }
}

impl Default for AutoProfileSwitch {
    fn default() -> Self {
        Self::new()
    }
}

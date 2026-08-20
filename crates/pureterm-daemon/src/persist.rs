//! GAP-5.2:守护进程重启后持久化存活隧道的配置。
//!
//! `TunnelManager` 持有的存活状态(`oneshot::Sender`、`AbortHandle`)
//! 不可序列化,因此我们快照出一个可序列化投影——`TunnelConfig`
//! ——写入守护进程自有的 `tunnels.json`。启动时重载该文件,
//! 把每个配置注册为*已停止*的隧道(仅恢复配置;
//! 由用户显式重启——见 GAP-5.2 方案;自动重连延后)。
//!
//! 机密被有意不持久化(镜像 SSH 配置档模式):
//! `TunnelConfig` 没有 `password` 字段,因此密码认证隧道重启
//! 恢复后重新进入 `NeedAuth`,而不是挂在从未存储的机密上。
//!
//! 磁盘形态向前兼容:每个字段都有默认值,以后加字段
//! 不破坏兼容(无需提升 `version`)。仍记录 `version`
//! 以备未来迁移。

use std::path::Path;

use serde::{Deserialize, Serialize};

fn default_port() -> u16 {
    22
}
fn default_remote_dns() -> bool {
    true
}

/// 存活隧道的可序列化投影。**无 `password`/机密字段**——
/// 见模块文档。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TunnelConfig {
    pub host: String,
    #[serde(default)]
    pub user: Option<String>,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default)]
    pub identity_file: Option<String>,
    pub local_port: u16,
    #[serde(default = "default_remote_dns")]
    pub remote_dns: bool,
    #[serde(default)]
    pub forward_agent: bool,
}

/// 磁盘容器。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TunnelsFile {
    pub version: u32,
    pub tunnels: Vec<TunnelConfig>,
}

/// 把 `cfg` 原子写入 `path`。尽力而为:失败仅记录日志(持久化
/// 错误绝不能让守护进程崩溃)。
pub fn save_tunnels(path: &Path, cfg: &TunnelsFile) {
    use std::io::Write;
    let Some(dir) = path.parent() else {
        log::warn!("tunnels: persist path has no parent");
        return;
    };
    if let Err(e) = std::fs::create_dir_all(dir) {
        log::warn!("tunnels: create_dir_all failed: {e}");
        return;
    }
    let json = match serde_json::to_string_pretty(cfg) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("tunnels: serialize failed: {e}");
            return;
        }
    };
    let mut tmp = match tempfile::NamedTempFile::new_in(dir) {
        Ok(t) => t,
        Err(e) => {
            log::warn!("tunnels: temp file failed: {e}");
            return;
        }
    };
    if let Err(e) = tmp.write_all(json.as_bytes()) {
        log::warn!("tunnels: write failed: {e}");
        return;
    }
    if let Err(e) = tmp.persist(path) {
        log::warn!("tunnels: persist failed: {e}");
    }
}

/// 加载 `tunnels.json`。文件缺失或损坏时返回 `None`。
pub fn load_tunnels(path: &Path) -> Option<TunnelsFile> {
    let s = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&s).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> TunnelsFile {
        TunnelsFile {
            version: 1,
            tunnels: vec![TunnelConfig {
                host: "gw.example".into(),
                user: Some("alice".into()),
                port: 2222,
                identity_file: Some("/home/alice/.ssh/id_ed25519".into()),
                local_port: 1080,
                remote_dns: false,
                forward_agent: true,
            }],
        }
    }

    #[test]
    fn round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tunnels.json");
        save_tunnels(&path, &sample());
        let loaded = load_tunnels(&path).expect("load after save");
        assert_eq!(loaded, sample());
    }

    #[test]
    fn default_port_and_remote_dns_when_absent() {
        // JSON 里没有 `port` / `remote_dns` → 默认 22 / true。
        let json = r#"{"host":"gw","local_port":1080}"#;
        let cfg: TunnelConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.port, 22);
        assert!(cfg.remote_dns);
        assert_eq!(cfg.host, "gw");
    }

    #[test]
    fn load_missing_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(load_tunnels(&tmp.path().join("nope.json")).is_none());
    }

    #[test]
    fn load_corrupt_returns_none() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tunnels.json");
        std::fs::write(&path, b"{ this is not json").unwrap();
        assert!(load_tunnels(&path).is_none());
    }

    #[test]
    fn secret_is_not_persisted() {
        // TunnelConfig 不存在 password 字段,因此序列化后的配置
        // 绝不能含 "password"——防止未来误加字段。
        let json = serde_json::to_string(&sample().tunnels[0]).unwrap();
        assert!(
            !json.contains("password"),
            "TunnelConfig must not serialize any secret: {json}"
        );
    }

    #[test]
    fn version_field_present_on_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("tunnels.json");
        save_tunnels(&path, &sample());
        let on_disk = std::fs::read_to_string(&path).unwrap();
        assert!(on_disk.contains("\"version\""), "missing version: {on_disk}");
    }
}

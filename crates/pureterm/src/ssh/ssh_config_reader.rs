use std::collections::HashMap;
use std::path::PathBuf;

/// 从 ~/.ssh/config 解析出的条目
#[derive(Debug, Clone)]
pub struct SshConfigHost {
    pub host_name: String,
    pub user: Option<String>,
    pub port: Option<u16>,
    pub identity_file: Option<PathBuf>,
}

/// 解析 ~/.ssh/config 并返回 Host 别名 → SshConfigHost 的映射
pub fn parse_ssh_config() -> std::io::Result<HashMap<String, SshConfigHost>> {
    let config_path = match std::env::var("HOME") {
        Ok(h) => std::path::PathBuf::from(h).join(".ssh/config"),
        Err(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "No home dir",
            ))
        },
    };

    if !config_path.exists() {
        return Ok(HashMap::new());
    }

    let content = std::fs::read_to_string(&config_path)?;
    let mut hosts = HashMap::new();
    let mut current_host: Option<(String, SshConfigHost)> = None;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with("Host ") {
            if let Some((alias, host)) = current_host.take() {
                hosts.insert(alias, host);
            }
            let alias = line[5..].trim().to_string();
            current_host = Some((
                alias.clone(),
                SshConfigHost {
                    host_name: alias,
                    user: None,
                    port: None,
                    identity_file: None,
                },
            ));
        } else if let Some((_, ref mut host)) = current_host {
            if let Some(val) = line.strip_prefix("User ") {
                host.user = Some(val.trim().to_string());
            } else if let Some(val) = line.strip_prefix("Port ") {
                host.port = val.trim().parse().ok();
            } else if let Some(val) = line.strip_prefix("IdentityFile ") {
                let path =
                    val.trim().replace('~', &std::env::var("HOME").unwrap_or_default());
                host.identity_file = Some(PathBuf::from(path));
            }
        }
    }

    if let Some((alias, host)) = current_host {
        hosts.insert(alias, host);
    }

    Ok(hosts)
}

#[cfg(test)]
mod tests {
    use super::*;

    // 测试使用唯一的基目录,避免并行运行时相互冲突
    fn setup_test_home(content: &str, suffix: &str) -> std::path::PathBuf {
        let base = std::path::PathBuf::from(format!("/tmp/pureterm_ssh_test_{}", suffix));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).expect("failed to create test base dir");

        let ssh_dir = base.join(".ssh");
        std::fs::create_dir(&ssh_dir).expect("failed to create .ssh dir");

        let config_path = ssh_dir.join("config");
        std::fs::write(&config_path, content).expect("failed to write config");

        base
    }

    fn cleanup_test_home(base: &std::path::PathBuf) {
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn test_parse_empty_ssh_config() {
        let test_home = setup_test_home("", "empty");
        let original_home = std::env::var("HOME").ok();
        // SAFETY:仅测试代码——HOME 必须指向我们写入配置的目录
        unsafe { std::env::set_var("HOME", test_home.to_string_lossy().as_ref()) };

        let result = parse_ssh_config();
        // SAFETY:仅测试代码——恢复 HOME
        unsafe { std::env::remove_var("HOME") };
        if let Some(home) = original_home {
            // SAFETY:仅测试代码
            unsafe { std::env::set_var("HOME", &home) };
        }

        cleanup_test_home(&test_home);

        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_parse_ssh_config_with_hosts() {
        let test_home = setup_test_home(
            r#"# This is a comment
Host webserver
    HostName 192.168.1.10
    User admin
    Port 2222
    IdentityFile ~/.ssh/id_rsa

Host database
    HostName db.example.com
    User postgres
    Port 5432

Host *
    ServerAliveInterval 60
"#,
            "hosts",
        );
        let original_home = std::env::var("HOME").ok();
        // SAFETY:仅测试代码——HOME 必须指向写入配置的 test_home
        unsafe { std::env::set_var("HOME", test_home.to_string_lossy().as_ref()) };

        let result = parse_ssh_config();
        // SAFETY:仅测试代码——恢复 HOME
        unsafe { std::env::remove_var("HOME") };
        if let Some(home) = original_home {
            // SAFETY:仅测试代码
            unsafe { std::env::set_var("HOME", &home) };
        }

        cleanup_test_home(&test_home);

        assert!(result.is_ok());
        let hosts = result.unwrap();
        // 注意:Host * 也会被解析为主机条目,因此共得到 3 个
        assert_eq!(hosts.len(), 3);

        let web = hosts.get("webserver").expect("webserver not found");
        assert_eq!(web.host_name, "webserver");
        assert_eq!(web.user, Some("admin".to_string()));
        assert_eq!(web.port, Some(2222));

        let db = hosts.get("database").expect("database not found");
        assert_eq!(db.host_name, "database");
        assert_eq!(db.user, Some("postgres".to_string()));
        assert_eq!(db.port, Some(5432));
    }
}

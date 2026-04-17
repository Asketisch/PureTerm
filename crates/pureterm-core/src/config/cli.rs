//! PureTerm 的命令行参数解析
//!
//! 支持长选项（--option=value）和位置参数。

use std::collections::HashMap;
use std::env;
use std::path::PathBuf;

/// 从命令行解析的 CLI 参数
#[derive(Debug, Clone, Default)]
pub struct CliArgs {
    /// 位置参数（通常是 shell/要运行的命令）
    pub positional: Vec<String>,
    /// 来自 --option=value 的键值对
    pub options: HashMap<String, String>,
    /// 配置文件路径（来自 -c/--config）
    pub config_file: Option<PathBuf>,
    /// 显示帮助并退出
    pub help: bool,
    /// 显示版本并退出
    pub version: bool,
}

impl CliArgs {
    /// 解析命令行参数
    pub fn parse() -> Self {
        Self::parse_from(env::args().skip(1).collect())
    }

    /// 从参数向量解析（对测试有用）
    pub fn parse_from(args: Vec<String>) -> Self {
        let mut cli = Self::default();
        let mut args = args.into_iter().peekable();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                // 特殊标志
                "-h" | "--help" => {
                    cli.help = true;
                }
                "-v" | "--version" => {
                    cli.version = true;
                }
                // 配置文件选项
                "-c" | "--config" => {
                    if let Some(path) = args.next() {
                        cli.config_file = Some(PathBuf::from(path));
                    }
                }
                // 带 = 语法的长选项
                s if s.starts_with("--") => {
                    if let Some(eq_pos) = s.find('=') {
                        let key = s[2..eq_pos].to_string();
                        let value = s[eq_pos + 1..].to_string();
                        cli.options.insert(key, value);
                    } else {
                        // 检查下一个参数是否是值（不是标志）
                        let key = s[2..].to_string();
                        if let Some(next) = args.peek() {
                            if !next.starts_with('-') {
                                cli.options.insert(key, args.next().unwrap());
                            }
                        }
                    }
                }
                // 短选项（单破折号，单字符）
                s if s.starts_with('-') && s.len() == 2 => {
                    let key = s[1..].to_string();
                    if let Some(next) = args.peek() {
                        if !next.starts_with('-') {
                            cli.options.insert(key, args.next().unwrap());
                        }
                    }
                }
                // 位置参数
                _ => {
                    cli.positional.push(arg);
                }
            }
        }

        cli
    }

    /// 按键获取选项值
    pub fn get(&self, key: &str) -> Option<&str> {
        self.options.get(key).map(|s| s.as_str())
    }

    /// 检查是否提供了选项
    pub fn contains(&self, key: &str) -> bool {
        self.options.contains_key(key)
    }

    /// 获取第一个位置参数
    pub fn command(&self) -> Option<&str> {
        self.positional.first().map(|s| s.as_str())
    }

    /// 获取剩余的位置参数（在命令之后）
    pub fn args(&self) -> &[String] {
        &self.positional
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_long_option() {
        let cli = CliArgs::parse_from(vec![
            "--font-size=16".to_string(),
            "--title=MyTerm".to_string(),
        ]);
        assert_eq!(cli.get("font-size"), Some("16"));
        assert_eq!(cli.get("title"), Some("MyTerm"));
    }

    #[test]
    fn test_parse_positional() {
        let cli = CliArgs::parse_from(vec![
            "bash".to_string(),
            "script.sh".to_string(),
        ]);
        assert_eq!(cli.command(), Some("bash"));
        assert_eq!(cli.args(), &["bash", "script.sh"]);
    }

    #[test]
    fn test_parse_config_file() {
        let cli = CliArgs::parse_from(vec![
            "-c".to_string(),
            "/path/to/config.toml".to_string(),
        ]);
        assert_eq!(cli.config_file, Some(PathBuf::from("/path/to/config.toml")));
    }

    #[test]
    fn test_parse_mixed() {
        let cli = CliArgs::parse_from(vec![
            "--font-size=14".to_string(),
            "bash".to_string(),
            "-l".to_string(),
        ]);
        assert_eq!(cli.get("font-size"), Some("14"));
        assert_eq!(cli.command(), Some("bash"));
    }
}

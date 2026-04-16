//! Command-line argument parsing for PureTerm
//!
//! Supports both long options (--option=value) and positional arguments.

use std::collections::HashMap;
use std::env;
use std::path::PathBuf;

/// CLI arguments parsed from command line
#[derive(Debug, Clone, Default)]
pub struct CliArgs {
    /// Positional arguments (usually the shell/command to run)
    pub positional: Vec<String>,
    /// Key-value pairs from --option=value
    pub options: HashMap<String, String>,
    /// Config file path (from -c/--config)
    pub config_file: Option<PathBuf>,
    /// Display help and exit
    pub help: bool,
    /// Display version and exit
    pub version: bool,
}

impl CliArgs {
    /// Parse command line arguments
    pub fn parse() -> Self {
        Self::parse_from(env::args().skip(1).collect())
    }

    /// Parse from a vector of arguments (useful for testing)
    pub fn parse_from(args: Vec<String>) -> Self {
        let mut cli = Self::default();
        let mut args = args.into_iter().peekable();

        while let Some(arg) = args.next() {
            match arg.as_str() {
                // Special flags
                "-h" | "--help" => {
                    cli.help = true;
                }
                "-v" | "--version" => {
                    cli.version = true;
                }
                // Config file option
                "-c" | "--config" => {
                    if let Some(path) = args.next() {
                        cli.config_file = Some(PathBuf::from(path));
                    }
                }
                // Long options with = syntax
                s if s.starts_with("--") => {
                    if let Some(eq_pos) = s.find('=') {
                        let key = s[2..eq_pos].to_string();
                        let value = s[eq_pos + 1..].to_string();
                        cli.options.insert(key, value);
                    } else {
                        // Check if next arg is a value (not a flag)
                        let key = s[2..].to_string();
                        if let Some(next) = args.peek() {
                            if !next.starts_with('-') {
                                cli.options.insert(key, args.next().unwrap());
                            }
                        }
                    }
                }
                // Short options (single dash, single char)
                s if s.starts_with('-') && s.len() == 2 => {
                    let key = s[1..].to_string();
                    if let Some(next) = args.peek() {
                        if !next.starts_with('-') {
                            cli.options.insert(key, args.next().unwrap());
                        }
                    }
                }
                // Positional arguments
                _ => {
                    cli.positional.push(arg);
                }
            }
        }

        cli
    }

    /// Get an option value by key
    pub fn get(&self, key: &str) -> Option<&str> {
        self.options.get(key).map(|s| s.as_str())
    }

    /// Check if an option was provided
    pub fn contains(&self, key: &str) -> bool {
        self.options.contains_key(key)
    }

    /// Get the first positional argument
    pub fn command(&self) -> Option<&str> {
        self.positional.first().map(|s| s.as_str())
    }

    /// Get remaining positional arguments (after command)
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

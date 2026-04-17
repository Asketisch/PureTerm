use pureterm_core::error::{CoreError, Result};
use pureterm_core::profile::{Profile, ProfileOptions, ProfileProvider, ProfileType, TabParams};

pub struct TelnetProfileProvider;

impl ProfileProvider for TelnetProfileProvider {
    fn id(&self) -> &'static str {
        "telnet"
    }

    fn name(&self) -> &'static str {
        "Telnet"
    }

    fn create_tab_params(&self, profile: &Profile) -> Result<TabParams> {
        let host = profile
            .options
            .telnet_host
            .as_deref()
            .ok_or_else(|| CoreError::Config("Telnet profile missing host".into()))?;
        let port = profile.options.telnet_port.unwrap_or(23);

        let title = format!("{}:{}", host, port);

        Ok(TabParams {
            profile_type: ProfileType::Telnet,
            title,
            cwd: None,
            env: None,
        })
    }

    fn get_description(&self, profile: &Profile) -> String {
        let host = profile
            .options
            .telnet_host
            .as_deref()
            .unwrap_or("host");
        let port = profile.options.telnet_port.unwrap_or(23);
        if port == 23 {
            host.to_string()
        } else {
            format!("{}:{}", host, port)
        }
    }

    fn quick_connect(&self, query: &str) -> Result<Profile> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Err(CoreError::Config(
                "Telnet quick connect requires host[:port]".into(),
            ));
        }

        let (host, port) = parse_telnet_query(trimmed)?;

        Ok(Profile {
            id: format!("telnet-{}", trimmed),
            name: host.clone(),
            profile_type: ProfileType::Telnet,
            group: None,
            options: ProfileOptions {
                telnet_host: Some(host),
                telnet_port: Some(port),
                ..Default::default()
            },
            color: None,
            icon: None,
        })
    }
}

fn parse_telnet_query(query: &str) -> Result<(String, u16)> {
    // 处理 IPv6 地址,如 [::1]:23
    if query.starts_with('[') {
        if let Some(bracket_end) = query.find(']') {
            let host = query[1..bracket_end].to_string();
            let rest = &query[bracket_end + 1..];
            let port = if rest.starts_with(':') {
                rest[1..]
                    .parse()
                    .map_err(|_| CoreError::Config(format!("Invalid port in: {}", query)))?
            } else {
                23
            };
            return Ok((host, port));
        }
    }

    // 常规的 host:port 或仅 host
    let (host, port) = if let Some((h, p)) = query.rsplit_once(':') {
        let port: u16 = p
            .parse()
            .map_err(|_| CoreError::Config(format!("Invalid port in: {}", query)))?;
        (h.to_string(), port)
    } else {
        (query.to_string(), 23)
    };

    if host.is_empty() {
        return Err(CoreError::Config("Host must not be empty".into()));
    }

    Ok((host, port))
}

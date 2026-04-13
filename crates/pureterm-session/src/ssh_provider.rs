use pureterm_core::error::{CoreError, Result};
use pureterm_core::profile::{Profile, ProfileOptions, ProfileProvider, ProfileType, TabParams};

pub struct SshProfileProvider;

impl ProfileProvider for SshProfileProvider {
    fn id(&self) -> &'static str {
        "ssh"
    }

    fn name(&self) -> &'static str {
        "SSH"
    }

    fn create_tab_params(&self, profile: &Profile) -> Result<TabParams> {
        let host = profile
            .options
            .host
            .as_deref()
            .ok_or_else(|| CoreError::Config("SSH profile missing host".into()))?;
        let user = profile
            .options
            .user
            .as_deref()
            .ok_or_else(|| CoreError::Config("SSH profile missing user".into()))?;
        let port = profile.options.port.unwrap_or(22);

        let title = format!("{}@{}:{}", user, host, port);

        Ok(TabParams {
            profile_type: ProfileType::Ssh,
            title,
            cwd: profile.options.cwd.clone(),
            env: profile.options.env.clone(),
        })
    }

    fn get_description(&self, profile: &Profile) -> String {
        let user = profile.options.user.as_deref().unwrap_or("user");
        let host = profile.options.host.as_deref().unwrap_or("host");
        let port = profile.options.port.unwrap_or(22);
        if port == 22 {
            format!("{}@{}", user, host)
        } else {
            format!("{}@{}:{}", user, host, port)
        }
    }

    fn quick_connect(&self, query: &str) -> Result<Profile> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Err(CoreError::Config(
                "SSH quick connect requires user@host or user@host:port".into(),
            ));
        }

        let (user, host, port) = parse_ssh_query(trimmed)?;

        Ok(Profile {
            id: format!("ssh-{}", trimmed),
            name: format!("{}@{}", user, host),
            profile_type: ProfileType::Ssh,
            group: None,
            options: ProfileOptions {
                host: Some(host),
                port: Some(port),
                user: Some(user),
                auth_method: Some("prompt".to_string()),
                ..Default::default()
            },
            color: None,
            icon: None,
        })
    }
}

fn parse_ssh_query(query: &str) -> Result<(String, String, u16)> {
    let (user, rest) = query
        .split_once('@')
        .ok_or_else(|| CoreError::Config("Expected user@host format".into()))?;

    let (host, port) = if let Some((h, p)) = rest.rsplit_once(':') {
        let port: u16 = p
            .parse()
            .map_err(|_| CoreError::Config(format!("Invalid port: {}", p)))?;
        (h.to_string(), port)
    } else {
        (rest.to_string(), 22)
    };

    if user.is_empty() || host.is_empty() {
        return Err(CoreError::Config(
            "User and host must not be empty".into(),
        ));
    }

    Ok((user.to_string(), host, port))
}

use pureterm_core::error::Result;
use pureterm_core::profile::{Profile, ProfileOptions, ProfileProvider, ProfileType, TabParams};

pub struct LocalProfileProvider;

impl ProfileProvider for LocalProfileProvider {
    fn id(&self) -> &'static str {
        "local"
    }

    fn name(&self) -> &'static str {
        "Local Shell"
    }

    fn create_tab_params(&self, profile: &Profile) -> Result<TabParams> {
        Ok(TabParams {
            profile_type: ProfileType::Local,
            title: profile.name.clone(),
            cwd: profile.options.cwd.clone(),
            env: profile.options.env.clone(),
        })
    }

    fn get_description(&self, profile: &Profile) -> String {
        let shell = profile
            .options
            .shell
            .as_deref()
            .unwrap_or("default shell");
        match &profile.options.cwd {
            Some(cwd) => format!("{} in {}", shell, cwd),
            None => shell.to_string(),
        }
    }

    fn quick_connect(&self, query: &str) -> Result<Profile> {
        let trimmed = query.trim();
        let (shell, cwd) = if trimmed.is_empty() {
            (None, None)
        } else if trimmed.contains('/') || trimmed.starts_with('~') {
            // Looks like a path
            (None, Some(trimmed.to_string()))
        } else {
            // Treat as shell name
            (Some(trimmed.to_string()), None)
        };

        Ok(Profile {
            id: uuid_like_id("local", trimmed),
            name: if trimmed.is_empty() {
                "Quick Local".to_string()
            } else {
                trimmed.to_string()
            },
            profile_type: ProfileType::Local,
            group: None,
            options: ProfileOptions {
                shell,
                cwd,
                ..Default::default()
            },
            color: None,
            icon: None,
        })
    }
}

fn uuid_like_id(prefix: &str, seed: &str) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    seed.hash(&mut hasher);
    format!("{}-{:016x}", prefix, hasher.finish())
}

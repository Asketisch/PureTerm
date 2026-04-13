use pureterm_core::error::{CoreError, Result};
use pureterm_core::profile::{Profile, ProfileOptions, ProfileProvider, ProfileType, TabParams};

pub struct SerialProfileProvider;

impl ProfileProvider for SerialProfileProvider {
    fn id(&self) -> &'static str {
        "serial"
    }

    fn name(&self) -> &'static str {
        "Serial"
    }

    fn create_tab_params(&self, profile: &Profile) -> Result<TabParams> {
        let path = profile
            .options
            .path
            .as_deref()
            .ok_or_else(|| CoreError::Config("Serial profile missing path".into()))?;
        let baud_rate = profile.options.baud_rate.unwrap_or(9600);

        let title = format!("{} @ {} baud", path, baud_rate);

        Ok(TabParams {
            profile_type: ProfileType::Serial,
            title,
            cwd: None,
            env: None,
        })
    }

    fn get_description(&self, profile: &Profile) -> String {
        let path = profile.options.path.as_deref().unwrap_or("/dev/ttyUSB0");
        let baud = profile.options.baud_rate.unwrap_or(9600);
        format!("{} @ {} baud", path, baud)
    }

    fn quick_connect(&self, query: &str) -> Result<Profile> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Err(CoreError::Config(
                "Serial quick connect requires path[:baud]".into(),
            ));
        }

        let (path, baud_rate) = if let Some((p, b)) = trimmed.rsplit_once(':') {
            let baud: u32 = b
                .parse()
                .map_err(|_| CoreError::Config(format!("Invalid baud rate: {}", b)))?;
            (p.to_string(), baud)
        } else {
            (trimmed.to_string(), 9600)
        };

        Ok(Profile {
            id: format!("serial-{}", trimmed),
            name: path.clone(),
            profile_type: ProfileType::Serial,
            group: None,
            options: ProfileOptions {
                path: Some(path),
                baud_rate: Some(baud_rate),
                data_bits: Some(8),
                stop_bits: Some(1),
                parity: Some("none".to_string()),
                flow_control: Some("none".to_string()),
                ..Default::default()
            },
            color: None,
            icon: None,
        })
    }
}

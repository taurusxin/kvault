use super::model::AutoLockSettings;
use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    pub version: u32,
    pub theme: Theme,
    pub auto_lock: AutoLockSettings,
    /// Non-secret references only. They never substitute for OS authentication.
    pub system_unlock: Option<String>,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: 1,
            theme: Theme::System,
            auto_lock: AutoLockSettings::default(),
            system_unlock: None,
        }
    }
}
impl Preferences {
    pub fn validate(&self) -> Result<()> {
        self.auto_lock.validate()?;
        if self.version != 1
            || self
                .system_unlock
                .as_ref()
                .is_some_and(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(AppError::invalid(
                "本机 config.json 格式无效，请修正或移除后重启",
            ));
        }
        Ok(())
    }
}

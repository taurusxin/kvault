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
pub struct CloseSettings {
    pub minimize_to_tray: bool,
    pub ask_before_close: bool,
}
impl Default for CloseSettings {
    fn default() -> Self {
        Self {
            minimize_to_tray: true,
            ask_before_close: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Preferences {
    pub version: u32,
    pub theme: Theme,
    pub auto_lock: AutoLockSettings,
    /// Non-secret references only. They never substitute for OS authentication.
    pub system_unlock: Option<String>,
    #[serde(default)]
    pub close: CloseSettings,
}
impl Default for Preferences {
    fn default() -> Self {
        Self {
            version: 1,
            theme: Theme::System,
            auto_lock: AutoLockSettings::default(),
            system_unlock: None,
            close: CloseSettings::default(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn existing_preferences_default_to_asking_before_hiding() {
        let json = r#"{"version":1,"theme":"dark","autoLock":{"enabled":true,"timeoutMinutes":5},"systemUnlock":null}"#;
        let prefs: Preferences = serde_json::from_str(json).unwrap();
        prefs.validate().unwrap();
        assert_eq!(prefs.close, CloseSettings::default());
        assert!(
            serde_json::from_str::<CloseSettings>(
                r#"{"minimizeToTray":true,"askBeforeClose":false,"password":"invalid"}"#
            )
            .is_err()
        );
    }

    #[test]
    fn close_settings_survive_restart_without_a_vault() {
        let dir = tempfile::tempdir().unwrap();
        let storage = crate::vault::storage::Storage::new(dir.path().into()).unwrap();
        let expected = CloseSettings {
            minimize_to_tray: false,
            ask_before_close: false,
        };
        storage
            .update_preferences(|p| p.close = expected.clone())
            .unwrap();
        assert!(!storage.exists());
        let reopened = crate::vault::storage::Storage::new(dir.path().into()).unwrap();
        assert_eq!(reopened.preferences().unwrap().close, expected);
    }
}

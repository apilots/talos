use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FsConfig {
    #[serde(default)]
    pub directory_picker: DirectoryPickerConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DirectoryPickerConfig {
    #[serde(default = "default_directory_picker_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub root: DirectoryPickerRoot,
    #[serde(default = "default_show_hidden")]
    pub show_hidden: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DirectoryPickerRoot {
    Home,
}

impl Default for FsConfig {
    fn default() -> Self {
        Self {
            directory_picker: DirectoryPickerConfig::default(),
        }
    }
}

impl Default for DirectoryPickerConfig {
    fn default() -> Self {
        Self {
            enabled: default_directory_picker_enabled(),
            root: DirectoryPickerRoot::default(),
            show_hidden: default_show_hidden(),
        }
    }
}

impl Default for DirectoryPickerRoot {
    fn default() -> Self {
        Self::Home
    }
}

fn default_directory_picker_enabled() -> bool {
    true
}

fn default_show_hidden() -> bool {
    true
}

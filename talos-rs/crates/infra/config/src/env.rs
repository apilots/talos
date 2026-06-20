use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use talos_error::{TalosError, TalosResult};
use talos_fs::WriteOptions;

pub const TALOS_ENV_PREFIX: &str = "TALOS_";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeEnv {
    values: BTreeMap<String, String>,
}

impl RuntimeEnv {
    pub fn load(env_file: &Path) -> TalosResult<Self> {
        Self::load_from_iter(env_file, std::env::vars())
    }

    pub fn load_from_iter<I, K, V>(env_file: &Path, process_env: I) -> TalosResult<Self>
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let mut values = EnvSecretStore::new(env_file.to_path_buf()).read_filtered()?;
        for (key, value) in process_env {
            let key = key.into();
            if is_talos_env_key(&key) {
                values.insert(key, value.into());
            }
        }
        Ok(Self { values })
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }
}

#[derive(Debug, Clone)]
pub struct EnvSecretStore {
    path: PathBuf,
}

impl EnvSecretStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn read_filtered(&self) -> TalosResult<BTreeMap<String, String>> {
        match std::fs::read_to_string(&self.path) {
            Ok(content) => Ok(parse_dotenv_filtered(&content)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(err) => Err(TalosError::ConfigError(format!(
                "failed to read env file {}: {}",
                self.path.display(),
                err
            ))),
        }
    }

    pub fn ensure_file(&self) -> TalosResult<()> {
        if self.path.exists() {
            return Ok(());
        }
        match talos_fs::write_string_with(&self.path, "", WriteOptions::create_new()) {
            Ok(()) | Err(TalosError::Conflict(_)) => Ok(()),
            Err(err) => Err(TalosError::ConfigError(format!(
                "failed to create env file {}: {}",
                self.path.display(),
                err
            ))),
        }
    }

    pub fn ensure_random(&self, key: &str) -> TalosResult<()> {
        let mut values = self.read_filtered()?;
        if values
            .get(key)
            .is_some_and(|value| !value.trim().is_empty())
        {
            return Ok(());
        }
        values.insert(key.to_string(), format!("talos-{}", uuid::Uuid::new_v4()));
        self.write_filtered(&values)
    }

    pub fn set(&self, key: &str, value: &str) -> TalosResult<()> {
        if !is_talos_env_key(key) || !is_valid_env_name(key) {
            return Err(TalosError::ConfigError(format!(
                "invalid talos env key '{}'",
                key
            )));
        }
        let mut values = self.read_filtered()?;
        values.insert(key.to_string(), value.to_string());
        self.write_filtered(&values)
    }

    pub fn remove(&self, key: &str) -> TalosResult<()> {
        let mut values = self.read_filtered()?;
        values.remove(key);
        self.write_filtered(&values)
    }

    pub fn write_filtered(&self, values: &BTreeMap<String, String>) -> TalosResult<()> {
        let mut output = String::new();
        for (key, value) in values {
            if !is_talos_env_key(key) {
                continue;
            }
            output.push_str(key);
            output.push('=');
            output.push_str(&quote_env_value(value));
            output.push('\n');
        }
        talos_fs::write_string(&self.path, &output).map_err(|err| {
            TalosError::ConfigError(format!(
                "failed to write env file {}: {}",
                self.path.display(),
                err
            ))
        })
    }
}

pub fn is_talos_env_key(key: &str) -> bool {
    key.starts_with(TALOS_ENV_PREFIX)
}

pub fn parse_dotenv_filtered(content: &str) -> BTreeMap<String, String> {
    let mut env = BTreeMap::new();
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let normalized = line.strip_prefix("export ").unwrap_or(line).trim();
        let Some((key, value)) = normalized.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if !is_valid_env_name(key) || !is_talos_env_key(key) {
            continue;
        }
        env.insert(key.to_string(), unquote_env_value(value.trim()));
    }
    env
}

fn is_valid_env_name(key: &str) -> bool {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
}

fn unquote_env_value(value: &str) -> String {
    if ((value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\'')))
        && value.len() >= 2
    {
        return value[1..value.len() - 1].to_string();
    }
    let comment_index = value.find(" #");
    comment_index
        .map(|index| value[..index].trim_end().to_string())
        .unwrap_or_else(|| value.to_string())
}

fn quote_env_value(value: &str) -> String {
    if value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':' | '/'))
    {
        return value.to_string();
    }
    format!("\"{}\"", value.replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_dotenv_filters_non_talos_keys() {
        let parsed = parse_dotenv_filtered(
            r#"
TALOS_CLOUD_API_TOKEN=secret
ANTHROPIC_API_KEY=ignored
export TALOS_LLM_ANTHROPIC_API_KEY="sk-test"
INVALID-NAME=ignored
"#,
        );

        assert_eq!(
            parsed.get("TALOS_CLOUD_API_TOKEN").map(String::as_str),
            Some("secret")
        );
        assert_eq!(
            parsed
                .get("TALOS_LLM_ANTHROPIC_API_KEY")
                .map(String::as_str),
            Some("sk-test")
        );
        assert!(!parsed.contains_key("ANTHROPIC_API_KEY"));
    }

    #[test]
    fn process_env_overrides_dotenv() {
        let dir = tempfile::tempdir().expect("temp dir should be created");
        let env_file = dir.path().join(".env");
        std::fs::write(&env_file, "TALOS_CLOUD_API_TOKEN=from-file\n")
            .expect("env file should write");

        let env =
            RuntimeEnv::load_from_iter(&env_file, [("TALOS_CLOUD_API_TOKEN", "from-process")])
                .expect("runtime env should load");

        assert_eq!(env.get("TALOS_CLOUD_API_TOKEN"), Some("from-process"));
    }
}

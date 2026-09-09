//! Bearer credential loading from environment variables or a protected file.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileCredentials {
    token: Option<String>,
    peer_token: Option<String>,
}

#[derive(Default)]
pub struct Credentials {
    pub token: Option<String>,
    pub peer_token: Option<String>,
}

impl Credentials {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let from_file = match path {
            Some(path) => read_credentials_file(path)?,
            None => FileCredentials::default(),
        };

        let token = match nonempty(from_file.token) {
            Some(token) => Some(token),
            None => match env_token("MNEMOS_TOKEN")? {
                Some(token) => Some(token),
                None => env_token("MNEMOS_API_KEY")?,
            },
        };
        let peer_token = match nonempty(from_file.peer_token) {
            Some(token) => Some(token),
            None => env_token("MNEMOS_PEER_TOKEN")?,
        };

        Ok(Self { token, peer_token })
    }
}

fn env_token(name: &str) -> Result<Option<String>> {
    match std::env::var(name) {
        Ok(value) => Ok(nonempty(Some(value))),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            anyhow::bail!("{name} contains non-UTF-8 data")
        }
    }
}

fn nonempty(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn read_credentials_file(path: &Path) -> Result<FileCredentials> {
    let mut file = File::open(path)
        .with_context(|| format!("failed to open credentials file {}", path.display()))?;
    let metadata = file
        .metadata()
        .with_context(|| format!("failed to inspect credentials file {}", path.display()))?;
    if !metadata.is_file() {
        anyhow::bail!("credentials path {} is not a regular file", path.display());
    }
    require_private_mode(path, &metadata)?;

    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .with_context(|| format!("failed to read credentials file {}", path.display()))?;
    let parsed: FileCredentials = toml::from_str(&contents)
        .map_err(|_| anyhow::anyhow!("credentials file {} is not valid TOML", path.display()))?;
    if nonempty(parsed.token.clone()).is_none() && nonempty(parsed.peer_token.clone()).is_none() {
        anyhow::bail!(
            "credentials file {} contains no non-empty token or peer_token",
            path.display()
        );
    }
    Ok(parsed)
}

#[cfg(unix)]
fn require_private_mode(path: &Path, metadata: &std::fs::Metadata) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mode = metadata.permissions().mode() & 0o777;
    if mode != 0o600 {
        anyhow::bail!(
            "credentials file {} must have mode 0600 (found {mode:04o})",
            path.display()
        );
    }
    Ok(())
}

#[cfg(not(unix))]
fn require_private_mode(path: &Path, _: &std::fs::Metadata) -> Result<()> {
    anyhow::bail!(
        "credentials file {} cannot be used because this platform cannot enforce mode 0600",
        path.display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::io::Write;

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    fn credentials_file(contents: &str) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().expect("temp file");
        file.write_all(contents.as_bytes())
            .expect("write credentials");
        file.flush().expect("flush credentials");
        file
    }

    #[test]
    fn mode_0600_toml_loads_both_tokens() {
        let file = credentials_file("token = \"server-secret\"\npeer_token = \"peer-secret\"\n");
        let loaded = read_credentials_file(file.path()).expect("credentials load");
        assert_eq!(loaded.token.as_deref(), Some("server-secret"));
        assert_eq!(loaded.peer_token.as_deref(), Some("peer-secret"));
    }

    #[cfg(unix)]
    #[test]
    fn credentials_file_rejects_any_mode_other_than_0600() {
        let file = credentials_file("token = \"secret\"\n");
        std::fs::set_permissions(file.path(), std::fs::Permissions::from_mode(0o640))
            .expect("chmod credentials");
        let error = match read_credentials_file(file.path()) {
            Ok(_) => panic!("mode must be rejected"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("must have mode 0600"));
    }

    #[test]
    fn invalid_toml_error_does_not_echo_secret_contents() {
        let file = credentials_file("token = super-secret-without-quotes\n");
        let error = match read_credentials_file(file.path()) {
            Ok(_) => panic!("invalid TOML must be rejected"),
            Err(error) => error,
        };
        assert!(!error.to_string().contains("super-secret"));
    }
}

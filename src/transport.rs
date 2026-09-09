//! Transport policy for requests carrying bearer credentials.

use std::net::IpAddr;

use anyhow::{Context, Result};

pub fn require_secure_for_credentials(
    url: &str,
    credentials_present: bool,
    allow_insecure_http: bool,
) -> Result<()> {
    if !credentials_present {
        return Ok(());
    }

    let parsed = reqwest::Url::parse(url).context("invalid URL for credentialed request")?;
    match parsed.scheme() {
        "https" => Ok(()),
        "http" => {
            let host = parsed
                .host_str()
                .context("credentialed request URL must include a host")?;
            if is_loopback(host) || allow_insecure_http {
                return Ok(());
            }
            anyhow::bail!(
                "refusing to send bearer credentials over plaintext HTTP to non-loopback host \
                 '{host}'; use HTTPS or explicitly pass --allow-insecure-http"
            )
        }
        scheme => anyhow::bail!(
            "refusing to send bearer credentials using unsupported URL scheme '{scheme}'; use HTTPS"
        ),
    }
}

fn is_loopback(host: &str) -> bool {
    let host = host.trim_matches(|character| character == '[' || character == ']');
    if host.trim_end_matches('.').eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<IpAddr>()
        .is_ok_and(|address| address.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credentialed_https_is_allowed() {
        assert!(require_secure_for_credentials("https://mnemos.example", true, false).is_ok());
    }

    #[test]
    fn credentialed_http_is_allowed_only_for_loopback_by_default() {
        for url in [
            "http://localhost:5002",
            "http://LOCALHOST.:5002",
            "http://127.0.0.1:5002",
            "http://127.42.0.9:5002",
            "http://[::1]:5002",
        ] {
            assert!(
                require_secure_for_credentials(url, true, false).is_ok(),
                "loopback URL was rejected: {url}"
            );
        }
        assert!(require_secure_for_credentials("http://192.0.2.10:5002", true, false).is_err());
        assert!(
            require_secure_for_credentials("http://localhost.example:5002", true, false).is_err()
        );
    }

    #[test]
    fn explicit_insecure_http_opt_in_allows_remote_http() {
        assert!(require_secure_for_credentials("http://192.0.2.10:5002", true, true).is_ok());
    }

    #[test]
    fn anonymous_remote_http_is_not_blocked_by_the_bearer_policy() {
        assert!(require_secure_for_credentials("http://192.0.2.10:5002", false, false).is_ok());
    }
}

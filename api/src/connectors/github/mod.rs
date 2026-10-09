pub mod authorization;

use reqwest::Url;
use std::{env, error::Error};

pub(crate) fn configured_base_url() -> Result<String, env::VarError> {
    match env::var("GITHUB_BASE_URL") {
        Ok(base) => Ok(base),
        Err(env::VarError::NotPresent) => Ok("https://api.github.com".into()),
        Err(error) => Err(error),
    }
}

/// Normalize the API prefix so relative endpoint URLs preserve `/api/v3/`.
pub(crate) fn api_base_url(base: &str) -> Result<Url, Box<dyn Error>> {
    let mut url = Url::parse(base)?;
    let loopback = url.scheme() == "http"
        && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if !(url.scheme() == "https" || loopback)
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "/" | "/api/v3" | "/api/v3/")
    {
        return Err("GITHUB_BASE_URL must be an HTTPS API origin, optionally ending in /api/v3 (HTTP allowed on loopback), without credentials, query or fragment".into());
    }
    if url.path() == "/api/v3" {
        url.set_path("/api/v3/");
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_cloud_and_enterprise_endpoint_paths() {
        for (base, expected) in [
            ("https://api.github.com", "https://api.github.com/user"),
            (
                "https://github.example/api/v3",
                "https://github.example/api/v3/user",
            ),
            (
                "https://github.example/api/v3/",
                "https://github.example/api/v3/user",
            ),
        ] {
            assert_eq!(
                api_base_url(base).unwrap().join("user").unwrap().as_str(),
                expected
            );
        }
        for base in [
            "http://github.example/api/v3",
            "https://user:secret@github.example",
            "https://github.example/api/v3?x=y",
            "https://github.example/#fragment",
            "https://github.example/other",
        ] {
            assert!(api_base_url(base).is_err());
        }
    }
}

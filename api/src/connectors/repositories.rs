use super::credentials::TokenCipher;
use reqwest::{Client, Url};
use serde::{Deserialize, Serialize};
use std::{env, error::Error, time::Duration};

#[derive(Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    Github,
    Gitlab,
}

impl Provider {
    pub fn name(self) -> &'static str {
        match self {
            Self::Github => "github",
            Self::Gitlab => "gitlab",
        }
    }
}

#[derive(Serialize)]
pub struct Repository {
    pub id: i64,
    pub full_name: String,
    pub web_url: String,
}

#[derive(Serialize)]
pub struct RepositoryList {
    pub repositories: Vec<Repository>,
    pub has_more: bool,
}

pub struct RepositoryClient {
    client: Client,
    gitlab_url: Url,
    github_url: Url,
    cipher: TokenCipher,
}

#[derive(Debug, PartialEq)]
pub enum RepositoryError {
    InvalidCredential,
    Rejected,
    Unavailable,
}

#[derive(Deserialize)]
struct GithubRepository {
    id: i64,
    full_name: String,
    html_url: String,
}
#[derive(Deserialize)]
struct GitlabProject {
    id: i64,
    path_with_namespace: String,
    web_url: String,
}
#[derive(Deserialize)]
struct GitlabAssociations {
    projects: Vec<GitlabProject>,
}

#[derive(Serialize)]
pub struct Branch {
    pub name: String,
    pub commit_sha: String,
}

#[derive(Serialize)]
pub struct BranchList {
    pub branches: Vec<Branch>,
    pub has_more: bool,
}

#[derive(Deserialize)]
struct ProviderBranch {
    name: String,
    commit: BranchCommit,
}

#[derive(Deserialize)]
struct BranchCommit {
    #[serde(alias = "id")]
    sha: String,
}

impl RepositoryClient {
    pub fn from_env() -> Result<Option<Self>, Box<dyn Error>> {
        let key = match env::var("CONNECTOR_TOKEN_KEY") {
            Ok(key) => key,
            Err(env::VarError::NotPresent) => return Ok(None),
            Err(error) => return Err(error.into()),
        };
        Self::new(
            &env::var("GITLAB_BASE_URL").unwrap_or_else(|_| "https://gitlab.com".into()),
            &super::github::configured_base_url()?,
            &key,
        )
        .map(Some)
    }

    fn new(gitlab: &str, github: &str, key: &str) -> Result<Self, Box<dyn Error>> {
        let gitlab_url = Url::parse(gitlab)?;
        let github_url = super::github::api_base_url(github)?;
        for url in [&gitlab_url] {
            let loopback = url.scheme() == "http"
                && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
            if !(url.scheme() == "https" || loopback)
                || url.host_str().is_none()
                || url.path() != "/"
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query().is_some()
                || url.fragment().is_some()
            {
                return Err(
                    "Repository API URLs must be HTTPS origins (HTTP allowed on loopback)".into(),
                );
            }
        }
        Ok(Self {
            client: Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(15))
                .user_agent("federated-learning")
                .build()?,
            gitlab_url,
            github_url,
            cipher: TokenCipher::new(key)?,
        })
    }

    /// Resolve one repository's HTTPS clone URL without trusting client URLs.
    pub(crate) async fn import_source(
        &self,
        provider: Provider,
        user_id: i32,
        encrypted: &str,
        repository: &str,
    ) -> Result<(Url, String), RepositoryError> {
        let token = self
            .cipher
            .decrypt(provider.name(), user_id, encrypted)
            .map_err(|_| RepositoryError::InvalidCredential)?;
        let base = match provider {
            Provider::Github => &self.github_url,
            Provider::Gitlab => &self.gitlab_url,
        };
        let mut url = base.clone();
        match provider {
            Provider::Github => {
                let parts = repository.split('/').collect::<Vec<_>>();
                if parts.len() != 2
                    || parts
                        .iter()
                        .any(|part| part.is_empty() || *part == "." || *part == "..")
                {
                    return Err(RepositoryError::Rejected);
                }
                url.path_segments_mut()
                    .map_err(|_| RepositoryError::Unavailable)?
                    .pop_if_empty()
                    .extend(["repos", parts[0], parts[1]]);
            }
            Provider::Gitlab => {
                if repository.parse::<i64>().ok().is_none_or(|id| id <= 0) {
                    return Err(RepositoryError::Rejected);
                }
                url = base
                    .join(&format!("api/v4/projects/{repository}"))
                    .map_err(|_| RepositoryError::Unavailable)?;
            }
        }
        let request = self.client.get(url);
        let request = match provider {
            Provider::Github => request
                .bearer_auth(&token)
                .header("Accept", "application/vnd.github+json"),
            Provider::Gitlab => request.header("PRIVATE-TOKEN", &token),
        };
        let response = request
            .send()
            .await
            .map_err(|_| RepositoryError::Unavailable)?;
        if matches!(response.status().as_u16(), 401 | 403 | 404) {
            return Err(RepositoryError::Rejected);
        }
        if !response.status().is_success() {
            return Err(RepositoryError::Unavailable);
        }
        let metadata: serde_json::Value = response
            .json()
            .await
            .map_err(|_| RepositoryError::Unavailable)?;
        let key = match provider {
            Provider::Github => "clone_url",
            Provider::Gitlab => "http_url_to_repo",
        };
        let clone = metadata
            .get(key)
            .and_then(serde_json::Value::as_str)
            .ok_or(RepositoryError::Unavailable)?;
        let clone = Url::parse(clone).map_err(|_| RepositoryError::Unavailable)?;
        let cloud_host = provider == Provider::Github
            && base.host_str() == Some("api.github.com")
            && clone.host_str() == Some("github.com")
            && clone.port_or_known_default() == Some(443);
        if !(clone.origin() == base.origin() || cloud_host)
            || clone.scheme() != base.scheme()
            || !clone.username().is_empty()
            || clone.password().is_some()
            || clone.query().is_some()
            || clone.fragment().is_some()
        {
            return Err(RepositoryError::Rejected);
        }
        Ok((clone, token))
    }

    pub async fn branches(
        &self,
        provider: Provider,
        user_id: i32,
        encrypted_token: &str,
        repository: &str,
        page: u32,
    ) -> Result<BranchList, RepositoryError> {
        let token = self
            .cipher
            .decrypt(provider.name(), user_id, encrypted_token)
            .map_err(|_| RepositoryError::InvalidCredential)?;
        let mut url = match provider {
            Provider::Github => {
                let parts: Vec<_> = repository.split('/').collect();
                if parts.len() != 2
                    || parts
                        .iter()
                        .any(|part| part.is_empty() || *part == "." || *part == "..")
                {
                    return Err(RepositoryError::Rejected);
                }
                let mut url = self.github_url.clone();
                url.path_segments_mut()
                    .map_err(|_| RepositoryError::Unavailable)?
                    .pop_if_empty()
                    .extend(["repos", parts[0], parts[1], "branches"]);
                url
            }
            Provider::Gitlab => {
                if repository.parse::<i64>().ok().is_none_or(|id| id <= 0) {
                    return Err(RepositoryError::Rejected);
                }
                self.gitlab_url
                    .join(&format!("api/v4/projects/{repository}/repository/branches"))
                    .map_err(|_| RepositoryError::Unavailable)?
            }
        };
        url.query_pairs_mut()
            .append_pair("per_page", "100")
            .append_pair("page", &page.to_string());
        let request = self.client.get(url);
        let request = match provider {
            Provider::Github => request
                .bearer_auth(token)
                .header("Accept", "application/vnd.github+json"),
            Provider::Gitlab => request.header("PRIVATE-TOKEN", token),
        };
        let response = request
            .send()
            .await
            .map_err(|_| RepositoryError::Unavailable)?;
        if matches!(response.status().as_u16(), 401 | 403 | 404) {
            return Err(RepositoryError::Rejected);
        }
        if !response.status().is_success() {
            return Err(RepositoryError::Unavailable);
        }
        let branches: Vec<Branch> = response
            .json::<Vec<ProviderBranch>>()
            .await
            .map_err(|_| RepositoryError::Unavailable)?
            .into_iter()
            .map(|branch| Branch {
                name: branch.name,
                commit_sha: branch.commit.sha,
            })
            .collect();
        let has_more = branches.len() >= 100;
        Ok(BranchList { branches, has_more })
    }

    pub async fn list(
        &self,
        provider: Provider,
        user_id: i32,
        encrypted_token: &str,
        page: u32,
    ) -> Result<RepositoryList, RepositoryError> {
        let token = self
            .cipher
            .decrypt(provider.name(), user_id, encrypted_token)
            .map_err(|_| RepositoryError::InvalidCredential)?;
        let (base, path) = match provider {
            Provider::Github => (&self.github_url, "user/repos"),
            Provider::Gitlab => (
                &self.gitlab_url,
                "api/v4/personal_access_tokens/self/associations",
            ),
        };
        // Construct pagination URLs ourselves; never follow provider links carrying credentials.
        let mut url = base.join(path).map_err(|_| RepositoryError::Unavailable)?;
        url.query_pairs_mut()
            .append_pair("per_page", "100")
            .append_pair("page", &page.to_string());
        if provider == Provider::Gitlab {
            // Reporter is the standard minimum role for reading repository code.
            // Unfiltered associations include public access and can time out on
            // GitLab.com even when the user belongs to only one project.
            url.query_pairs_mut().append_pair("min_access_level", "20");
        }
        let mut request = self.client.get(url);
        request = match provider {
            Provider::Github => request
                .bearer_auth(token)
                .header("Accept", "application/vnd.github+json"),
            Provider::Gitlab => request.header("PRIVATE-TOKEN", token),
        };
        let response = request
            .send()
            .await
            .map_err(|_| RepositoryError::Unavailable)?;
        if matches!(response.status().as_u16(), 401 | 403) {
            return Err(RepositoryError::Rejected);
        }
        if !response.status().is_success() {
            return Err(RepositoryError::Unavailable);
        }
        let repositories: Vec<Repository> = match provider {
            Provider::Github => response
                .json::<Vec<GithubRepository>>()
                .await
                .map_err(|_| RepositoryError::Unavailable)?
                .into_iter()
                .map(|repo| Repository {
                    id: repo.id,
                    full_name: repo.full_name,
                    web_url: repo.html_url,
                })
                .collect(),
            Provider::Gitlab => response
                .json::<GitlabAssociations>()
                .await
                .map_err(|_| RepositoryError::Unavailable)?
                .projects
                .into_iter()
                .map(|repo| Repository {
                    id: repo.id,
                    full_name: repo.path_with_namespace,
                    web_url: repo.web_url,
                })
                .collect(),
        };
        // A full final page may cause one harmless extra request, avoiding reliance
        // on Link headers (GitLab associations paginate groups and projects together).
        let has_more = repositories.len() >= 100;
        Ok(RepositoryList {
            repositories,
            has_more,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        extract::Query,
        http::{HeaderMap, StatusCode},
        routing::get,
    };
    use base64::{Engine, engine::general_purpose::STANDARD};
    use std::collections::HashMap;

    #[tokio::test]
    async fn import_urls_use_enterprise_api_paths_and_cannot_forward_tokens_to_other_origins() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let clone_base = base.clone();
        let app = Router::new().route(
            "/api/v3/repos/Org/{repo}",
            get(
                move |axum::extract::Path(repo): axum::extract::Path<String>,
                      headers: HeaderMap| {
                    let base = clone_base.clone();
                    async move {
                        assert_eq!(headers["authorization"], "Bearer saved-token");
                        let url = match repo.as_str() {
                            "Valid" => format!("{base}/Org/Valid.git"),
                            "Other" => "https://attacker.invalid/Org/Other.git".into(),
                            "Credentials" => {
                                base.replace("http://", "http://secret@") + "/repo.git"
                            }
                            "Query" => format!("{base}/repo.git?secret=true"),
                            _ => "file:///etc/passwd".into(),
                        };
                        Json(serde_json::json!({"clone_url":url}))
                    }
                },
            ),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client =
            RepositoryClient::new(&base, &format!("{base}/api/v3"), &STANDARD.encode([7; 32]))
                .unwrap();
        let encrypted = client.cipher.encrypt("github", 12, "saved-token").unwrap();
        let (url, token) = client
            .import_source(Provider::Github, 12, &encrypted, "Org/Valid")
            .await
            .unwrap();
        assert_eq!(url.as_str(), format!("{base}/Org/Valid.git"));
        assert_eq!(token, "saved-token");
        for repo in ["Other", "Credentials", "Query", "File"] {
            assert_eq!(
                client
                    .import_source(Provider::Github, 12, &encrypted, &format!("Org/{repo}"))
                    .await
                    .err(),
                Some(RepositoryError::Rejected)
            );
        }
        assert_eq!(
            client
                .import_source(Provider::Github, 13, &encrypted, "Org/Valid")
                .await
                .err(),
            Some(RepositoryError::InvalidCredential)
        );
        server.abort();
    }

    #[tokio::test]
    async fn lists_both_providers_using_read_requests_and_safe_pagination() {
        let app = Router::new()
            .route("/api/v3/user/repos", get(|headers: HeaderMap, Query(query): Query<HashMap<String, String>>| async move {
                assert_eq!(headers["authorization"], "Bearer saved-github-token");
                assert_eq!(headers["user-agent"], "federated-learning");
                assert_eq!(query["page"], "2");
                assert_eq!(query["per_page"], "100");
                Json(serde_json::json!([{"id": 1, "full_name": "Org/Github", "html_url": "https://github.com/Org/Github"}]))
            }))
            .route("/api/v4/personal_access_tokens/self/associations", get(|headers: HeaderMap| async move {
                assert_eq!(headers["private-token"], "saved-gitlab-token");
                Json(serde_json::json!({"groups": [], "projects": [{"id": 2, "path_with_namespace": "Lab/Gitlab", "web_url": "https://gitlab.com/Lab/Gitlab"}]}))
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let key = STANDARD.encode([7; 32]);
        let client = RepositoryClient::new(&base, &format!("{base}/api/v3"), &key).unwrap();
        for (provider, name) in [
            (Provider::Github, "Org/Github"),
            (Provider::Gitlab, "Lab/Gitlab"),
        ] {
            let token = client
                .cipher
                .encrypt(
                    provider.name(),
                    12,
                    &format!("saved-{}-token", provider.name()),
                )
                .unwrap();
            let page = client.list(provider, 12, &token, 2).await.unwrap();
            assert_eq!(page.repositories[0].full_name, name);
            assert!(!page.has_more);
            assert!(!serde_json::to_string(&page).unwrap().contains("saved-"));
            assert_eq!(
                client.list(provider, 13, &token, 2).await.err(),
                Some(RepositoryError::InvalidCredential)
            );
        }
        task.abort();
    }

    #[tokio::test]
    async fn loads_branch_heads_for_gitlab_and_github_enterprise_and_paginates() {
        let app = Router::new()
            .route("/api/v3/repos/Org/Repo/branches", get(|headers: HeaderMap, Query(query): Query<HashMap<String, String>>| async move {
                assert_eq!(headers["authorization"], "Bearer branch-token");
                assert_eq!(query["per_page"], "100");
                assert_eq!(query["page"], "2");
                Json(vec![serde_json::json!({"name": "feature/topic", "commit": {"sha": "abc123"}}); 100])
            }))
            .route("/api/v4/projects/42/repository/branches", get(|headers: HeaderMap, Query(query): Query<HashMap<String, String>>| async move {
                assert_eq!(headers["private-token"], "branch-token");
                assert_eq!(query["page"], "2");
                Json(serde_json::json!([{"name": "main", "commit": {"id": "def456"}}]))
            }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client =
            RepositoryClient::new(&base, &format!("{base}/api/v3"), &STANDARD.encode([7; 32]))
                .unwrap();
        for (provider, repository, head, has_more) in [
            (Provider::Github, "Org/Repo", "abc123", true),
            (Provider::Gitlab, "42", "def456", false),
        ] {
            let token = client
                .cipher
                .encrypt(provider.name(), 12, "branch-token")
                .unwrap();
            let result = client
                .branches(provider, 12, &token, repository, 2)
                .await
                .unwrap();
            assert_eq!(result.branches[0].commit_sha, head);
            assert_eq!(result.has_more, has_more);
            assert!(
                !serde_json::to_string(&result)
                    .unwrap()
                    .contains("branch-token")
            );
            assert_eq!(
                client
                    .branches(provider, 13, &token, repository, 2)
                    .await
                    .err(),
                Some(RepositoryError::InvalidCredential)
            );
        }
        task.abort();
    }

    #[tokio::test]
    async fn gitlab_discovery_limits_associations_to_repository_read_access() {
        // Unfiltered discovery can scan public associations and time out even
        // for an account with a single repository. Model that provider failure.
        let app = Router::new().route(
            "/api/v4/personal_access_tokens/self/associations",
            get(|headers: HeaderMap, Query(query): Query<HashMap<String, String>>| async move {
                assert_eq!(headers["private-token"], "saved-token");
                assert_eq!(query["page"], "1");
                assert_eq!(query["per_page"], "100");
                if query.get("min_access_level").map(String::as_str) != Some("20") {
                    return (StatusCode::GATEWAY_TIMEOUT, Json(serde_json::json!({"error": "unfiltered discovery timed out"})));
                }
                (StatusCode::OK, Json(serde_json::json!({"groups": [], "projects": [{"id": 42, "path_with_namespace": "Lab/OnlyRepo", "web_url": "https://gitlab.com/Lab/OnlyRepo"}]})))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let client = RepositoryClient::new(&base, &base, &STANDARD.encode([7; 32])).unwrap();
        let token = client.cipher.encrypt("gitlab", 12, "saved-token").unwrap();
        let result = client.list(Provider::Gitlab, 12, &token, 1).await;
        task.abort();
        let page =
            result.expect("single repository should load without unfiltered discovery timing out");
        assert_eq!(page.repositories.len(), 1);
        assert_eq!(page.repositories[0].full_name, "Lab/OnlyRepo");
        assert!(!page.has_more);
    }

    #[tokio::test]
    async fn rejects_revoked_tokens_provider_errors_and_redirects() {
        for (status, expected) in [
            (StatusCode::UNAUTHORIZED, RepositoryError::Rejected),
            (StatusCode::FORBIDDEN, RepositoryError::Rejected),
            (StatusCode::TOO_MANY_REQUESTS, RepositoryError::Unavailable),
            (StatusCode::FOUND, RepositoryError::Unavailable),
        ] {
            let app = Router::new().route(
                "/user/repos",
                get(move || async move {
                    (
                        status,
                        [("Location", "http://example.com/steal-token")],
                        "unavailable",
                    )
                }),
            );
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let base = format!("http://{}", listener.local_addr().unwrap());
            let task = tokio::spawn(async move {
                axum::serve(listener, app).await.unwrap();
            });
            let client = RepositoryClient::new(&base, &base, &STANDARD.encode([7; 32])).unwrap();
            let token = client.cipher.encrypt("github", 12, "token").unwrap();
            assert_eq!(
                client.list(Provider::Github, 12, &token, 1).await.err(),
                Some(expected)
            );
            task.abort();
        }
    }
}

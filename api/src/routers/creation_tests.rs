use super::*;
use crate::{db::users::Claims, state::AppState};
use axum::Router;
use jsonwebtoken::{DecodingKey, EncodingKey, Header, encode};
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{
    net::SocketAddr,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

const SECRET: &[u8] = b"creation-test-secret-at-least-thirty-two-bytes";

async fn serve(pool: PgPool) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    serve_with_git(pool, crate::git::GitClient::test_config()).await
}

async fn serve_with_git(
    pool: PgPool,
    git: crate::git::GitClient,
) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    let app = Router::new()
        .merge(organizations_router())
        .merge(projects_router())
        .merge(users_router())
        .with_state(AppState {
            pool,
            encoding_key: EncodingKey::from_secret(SECRET),
            decoding_key: DecodingKey::from_secret(SECRET),
            git,
        });
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (address, server)
}

async fn request(
    address: SocketAddr,
    method: &str,
    path: &str,
    body: Value,
    token: Option<&str>,
) -> (u16, String) {
    let body = body.to_string();
    let authorization = token
        .map(|token| format!("Authorization: Bearer {token}\r\n"))
        .unwrap_or_default();
    let mut connection = TcpStream::connect(address).await.unwrap();
    connection.write_all(format!(
        "{method} {path} HTTP/1.1\r\nHost: localhost\r\n{authorization}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len(),
    ).as_bytes()).await.unwrap();
    let mut response = String::new();
    connection.read_to_string(&mut response).await.unwrap();
    let (headers, body) = response.split_once("\r\n\r\n").unwrap();
    let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
    (status, body.to_owned())
}

fn token(user_id: i32) -> String {
    let claims = Claims {
        sub: user_id.to_string(),
        exp: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600,
        jti: uuid::Uuid::new_v4().to_string(),
    };
    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(SECRET),
    )
    .unwrap()
}

#[tokio::test]
async fn creation_routes_reject_missing_and_forged_credentials() {
    let (address, server) =
        serve(PgPool::connect_lazy("postgres://localhost/unused").unwrap()).await;
    for path in ["/organizations", "/organizations/1/projects"] {
        for token in [None, Some("forged-token")] {
            let (status, _) =
                request(address, "POST", path, json!({"name": "Example"}), token).await;
            assert_eq!(status, 401, "{path}");
        }
    }
    server.abort();
}

#[tokio::test]
async fn project_members_route_rejects_missing_and_forged_credentials() {
    let (address, server) =
        serve(PgPool::connect_lazy("postgres://localhost/unused").unwrap()).await;
    for token in [None, Some("forged-token")] {
        assert_eq!(
            request(address, "GET", "/projects/1/members", Value::Null, token)
                .await
                .0,
            401
        );
    }
    server.abort();
}

#[tokio::test]
async fn snapshot_metadata_route_rejects_missing_and_forged_credentials() {
    let (address, server) =
        serve(PgPool::connect_lazy("postgres://localhost/unused").unwrap()).await;
    for token in [None, Some("forged-token")] {
        for path in [
            "/projects/42/snapshots",
            "/projects/42/snapshots/7",
            "/projects/42/snapshots/7/tree",
            "/projects/42/snapshots/7/file?path=train.py",
        ] {
            assert_eq!(
                request(address, "GET", path, Value::Null, token).await.0,
                401
            );
        }
    }
    server.abort();
}

#[tokio::test]
async fn snapshot_save_route_rejects_missing_and_forged_credentials() {
    let (address, server) =
        serve(PgPool::connect_lazy("postgres://localhost/unused").unwrap()).await;
    for token in [None, Some("forged-token")] {
        for body in [json!({"files": []}), Value::Null] {
            assert_eq!(
                request(address, "POST", "/projects/42/snapshots", body, token)
                    .await
                    .0,
                401
            );
        }
    }
    server.abort();
}

#[tokio::test]
#[ignore = "requires TEST_DATABASE_URL and permission to create a temporary schema"]
async fn creation_endpoints_persist_membership_and_enforce_project_ownership() {
    let url = std::env::var("TEST_DATABASE_URL").unwrap();
    let admin = PgPool::connect(&url).await.unwrap();
    let schema = format!("fl_creation_{}", uuid::Uuid::new_v4().simple());
    sqlx::raw_sql(&format!("CREATE SCHEMA {schema}"))
        .execute(&admin)
        .await
        .unwrap();
    let search_path = format!("SET search_path TO {schema}");
    let pool = PgPoolOptions::new()
        .after_connect(move |connection, _| {
            let search_path = search_path.clone();
            Box::pin(async move {
                sqlx::query(&search_path).execute(connection).await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    let schema_sql = include_str!("../../../db/schema.sql")
        .replace(
            "\\ir deployment_rules.sql",
            include_str!("../../../db/deployment_rules.sql"),
        )
        .lines()
        .filter(|line| !line.starts_with('\\'))
        .collect::<Vec<_>>()
        .join("\n");
    sqlx::raw_sql(&schema_sql).execute(&pool).await.unwrap();
    let storage = std::env::temp_dir().join(format!("fl-creation-{}", uuid::Uuid::new_v4()));
    let git = crate::git::GitClient::configured(storage.clone()).unwrap();
    let (address, server) = serve_with_git(pool.clone(), git.clone()).await;
    let check_pool = pool.clone();
    // Run assertions in a task so a failed assertion still allows schema cleanup.
    let result = tokio::spawn(async move {
        check_creation(address, &check_pool, &git).await;
        check_project_members(address, &check_pool).await;
        check_snapshots(address, &check_pool, &git).await;
        check_save_snapshots(address, &check_pool, &git).await;
    })
    .await;
    server.abort();
    pool.close().await;
    std::fs::remove_dir_all(storage).unwrap();
    sqlx::raw_sql(&format!("DROP SCHEMA {schema} CASCADE"))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    result.unwrap();
}

async fn check_project_members(address: SocketAddr, pool: &PgPool) {
    let mut users = Vec::new();
    for username in [
        "project-owner",
        "project-editor",
        "project-participant",
        "project-unassigned",
        "project-outsider",
    ] {
        users.push(
            sqlx::query_scalar::<_, i32>(
                "INSERT INTO users (username, password_hash) VALUES ($1, 'unused') RETURNING id",
            )
            .bind(username)
            .fetch_one(pool)
            .await
            .unwrap(),
        );
    }
    let owner = users[0];
    let organization =
        crate::db::organizations::create_organization(pool, owner, "Members fixture")
            .await
            .unwrap();
    let project =
        crate::db::projects::create_project(pool, organization.id, owner, "Members fixture")
            .await
            .unwrap()
            .unwrap();
    let other_project =
        crate::db::projects::create_project(pool, organization.id, owner, "Other project")
            .await
            .unwrap()
            .unwrap();
    let role = sqlx::query_scalar::<_, i32>(
        "INSERT INTO roles (org_id, name) VALUES ($1, 'Editor') RETURNING id",
    )
    .bind(organization.id)
    .fetch_one(pool)
    .await
    .unwrap();
    let participant_role = sqlx::query_scalar::<_, i32>(
        "INSERT INTO roles (org_id, name) VALUES ($1, 'Participant') RETURNING id",
    )
    .bind(organization.id)
    .fetch_one(pool)
    .await
    .unwrap();
    let unassigned_role = sqlx::query_scalar::<_, i32>(
        "INSERT INTO roles (org_id, name) VALUES ($1, 'Other project editor') RETURNING id",
    )
    .bind(organization.id)
    .fetch_one(pool)
    .await
    .unwrap();
    for (user, role) in [
        (users[1], role),
        (users[2], participant_role),
        (users[3], unassigned_role),
    ] {
        sqlx::query("INSERT INTO user_organization (org_id, user_id, role_id) VALUES ($1, $2, $3)")
            .bind(organization.id)
            .bind(user)
            .bind(role)
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query("INSERT INTO role_project_permission (org_id, role_id, project_id, perm_id)
                 SELECT $1, $2, $3, id FROM permissions WHERE name IN ('edit_project', 'start_deployment')")
        .bind(organization.id).bind(role).bind(project.id).execute(pool).await.unwrap();
    sqlx::query(
        "INSERT INTO role_project_permission (org_id, role_id, project_id, perm_id)
                 SELECT $1, $2, $3, id FROM permissions WHERE name = 'participate_in_deployment'",
    )
    .bind(organization.id)
    .bind(participant_role)
    .bind(project.id)
    .execute(pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO role_project_permission (org_id, role_id, project_id, perm_id)
                 SELECT $1, $2, $3, id FROM permissions WHERE name = 'edit_project'",
    )
    .bind(organization.id)
    .bind(unassigned_role)
    .bind(other_project.id)
    .execute(pool)
    .await
    .unwrap();
    let path = format!("/projects/{}/members", project.id);
    for reader in [owner, users[1], users[3]] {
        let (status, body) =
            request(address, "GET", &path, Value::Null, Some(&token(reader))).await;
        assert_eq!(status, 200, "{body}");
        let body: Value = serde_json::from_str(&body).unwrap();
        let members = body["members"].as_array().unwrap();
        assert_eq!(members.len(), 3);
        assert_eq!(
            members
                .iter()
                .map(|member| member["username"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["project-editor", "project-owner", "project-participant"]
        );
        assert_eq!(members[0]["role_name"], "Editor");
        assert!(members[1]["role_id"].is_null());
        assert_eq!(members[2]["role_name"], "Participant");
    }
    for (path, reader) in [
        (path.clone(), users[4]),
        ("/projects/2147483647/members".into(), owner),
    ] {
        let (status, body) =
            request(address, "GET", &path, Value::Null, Some(&token(reader))).await;
        assert_eq!(status, 200);
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap(),
            json!({"members": []})
        );
    }
    // Revoking the project's grant removes a current member from the list.
    sqlx::query("DELETE FROM role_project_permission WHERE role_id = $1 AND project_id = $2")
        .bind(participant_role)
        .bind(project.id)
        .execute(pool)
        .await
        .unwrap();
    let (_, body) = request(address, "GET", &path, Value::Null, Some(&token(owner))).await;
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["members"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

async fn check_creation(address: SocketAddr, pool: &PgPool, git: &crate::git::GitClient) {
    let owner = sqlx::query_scalar::<_, i32>(
        "INSERT INTO users (username, password_hash) VALUES ('owner', 'unused') RETURNING id",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let other = sqlx::query_scalar::<_, i32>(
        "INSERT INTO users (username, password_hash) VALUES ('other', 'unused') RETURNING id",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    let owner_token = token(owner);
    let other_token = token(other);

    let (status, body) = request(
        address,
        "POST",
        "/organizations",
        json!({"name": "  O'Reilly lab  "}),
        Some(&owner_token),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let organization: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(organization["name"], "O'Reilly lab");
    assert_eq!(organization["owner_user_id"], owner);
    let org_id = organization["id"].as_i64().unwrap() as i32;
    let membership = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS (SELECT 1 FROM user_organization WHERE org_id = $1 AND user_id = $2)",
    )
    .bind(org_id)
    .bind(owner)
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(membership);
    let (status, body) = request(
        address,
        "GET",
        "/users/organizations",
        Value::Null,
        Some(&owner_token),
    )
    .await;
    assert_eq!(status, 200);
    let listed: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(listed["organizations"][0], organization);

    let project_path = format!("/organizations/{org_id}/projects");
    let (status, body) = request(
        address,
        "POST",
        &project_path,
        json!({"name": "  Training  "}),
        Some(&owner_token),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let project: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(project["name"], "Training");
    assert_eq!(project["org_id"], org_id);
    assert_eq!(project["created_by_user_id"], owner);
    let project_id = project["id"].as_i64().unwrap() as i32;
    assert!(
        git.repository_root()
            .join(format!("{project_id}.git/HEAD"))
            .is_file()
    );
    assert!(git.list_snapshot_refs(project_id).await.unwrap().is_empty());
    let mounts = sqlx::query_as::<_, (String, String)>(
        "SELECT input_mount_destination, output_mount_destination FROM projects WHERE id = $1",
    )
    .bind(project["id"].as_i64().unwrap() as i32)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(mounts, ("/data/input".into(), "/data/output".into()));
    let (status, body) = request(
        address,
        "GET",
        &project_path,
        Value::Null,
        Some(&owner_token),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["projects"][0],
        project
    );

    // An outsider, ordinary member, and role manager cannot create projects.
    assert_eq!(
        request(
            address,
            "POST",
            &project_path,
            json!({"name": "Denied"}),
            Some(&other_token)
        )
        .await
        .0,
        403
    );
    sqlx::query("INSERT INTO user_organization (org_id, user_id) VALUES ($1, $2)")
        .bind(org_id)
        .bind(other)
        .execute(pool)
        .await
        .unwrap();
    assert_eq!(
        request(
            address,
            "POST",
            &project_path,
            json!({"name": "Denied"}),
            Some(&other_token)
        )
        .await
        .0,
        403
    );
    let role = sqlx::query_scalar::<_, i32>(
        "INSERT INTO roles (org_id, name) VALUES ($1, 'Manager') RETURNING id",
    )
    .bind(org_id)
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO role_permission (role_id, perm_id) SELECT $1, id FROM permissions WHERE name = 'edit_roles'")
        .bind(role).execute(pool).await.unwrap();
    sqlx::query("UPDATE user_organization SET role_id = $1 WHERE org_id = $2 AND user_id = $3")
        .bind(role)
        .bind(org_id)
        .bind(other)
        .execute(pool)
        .await
        .unwrap();
    assert_eq!(
        request(
            address,
            "POST",
            &project_path,
            json!({"name": "Denied"}),
            Some(&other_token)
        )
        .await
        .0,
        403
    );
    assert_eq!(
        request(
            address,
            "POST",
            "/organizations/2147483647/projects",
            json!({"name": "Denied"}),
            Some(&owner_token)
        )
        .await
        .0,
        403
    );

    for path in ["/organizations", project_path.as_str()] {
        assert_eq!(
            request(
                address,
                "POST",
                path,
                json!({"name": "   "}),
                Some(&owner_token)
            )
            .await
            .0,
            400
        );
        assert_eq!(
            request(
                address,
                "POST",
                path,
                json!({"name": "a".repeat(256)}),
                Some(&owner_token)
            )
            .await
            .0,
            400
        );
        assert_eq!(
            request(
                address,
                "POST",
                path,
                json!({"name": "Spoof", "owner_user_id": other}),
                Some(&owner_token)
            )
            .await
            .0,
            422
        );
        assert_eq!(
            request(address, "POST", path, json!({}), Some(&owner_token))
                .await
                .0,
            422
        );
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM organizations")
            .fetch_one(pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM projects")
            .fetch_one(pool)
            .await
            .unwrap(),
        1
    );
    // Fail at COMMIT, after Git initialization has succeeded.
    sqlx::raw_sql(
        "CREATE FUNCTION reject_project_commit() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN RAISE EXCEPTION 'test commit rejection'; END $$;
         CREATE CONSTRAINT TRIGGER reject_project_commit
         AFTER INSERT ON projects DEFERRABLE INITIALLY DEFERRED
         FOR EACH ROW EXECUTE FUNCTION reject_project_commit();",
    )
    .execute(pool)
    .await
    .unwrap();
    let (status, _) = request(
        address,
        "POST",
        &project_path,
        json!({"name": "Commit failure"}),
        Some(&owner_token),
    )
    .await;
    assert_eq!(status, 500);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM projects")
            .fetch_one(pool)
            .await
            .unwrap(),
        1
    );
    assert_eq!(std::fs::read_dir(git.repository_root()).unwrap().count(), 1);
    sqlx::raw_sql(
        "DROP TRIGGER reject_project_commit ON projects; DROP FUNCTION reject_project_commit();",
    )
    .execute(pool)
    .await
    .unwrap();

    // Existing storage must neither be adopted nor deleted on creation failure.
    let next_id = sqlx::query_scalar::<_, i32>(
        "SELECT nextval(pg_get_serial_sequence('projects', 'id'))::integer",
    )
    .fetch_one(pool)
    .await
    .unwrap()
        + 1;
    let existing = git.repository_root().join(format!("{next_id}.git"));
    std::fs::create_dir(&existing).unwrap();
    std::fs::write(existing.join("sentinel"), b"keep").unwrap();
    let (status, _) = request(
        address,
        "POST",
        &project_path,
        json!({"name": "Repository failure"}),
        Some(&owner_token),
    )
    .await;
    assert_eq!(status, 500);
    assert_eq!(std::fs::read(existing.join("sentinel")).unwrap(), b"keep");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM projects")
            .fetch_one(pool)
            .await
            .unwrap(),
        1
    );

    sqlx::query("INSERT INTO revoked_tokens (jti, expires_at) VALUES ($1, CURRENT_TIMESTAMP + interval '1 hour')")
        .bind(crate::db::users::verify_user_token(&owner_token, &AppState {
            pool: pool.clone(), encoding_key: EncodingKey::from_secret(SECRET), decoding_key: DecodingKey::from_secret(SECRET),
            git: crate::git::GitClient::test_config(),
        }).unwrap().jti).execute(pool).await.unwrap();
    for path in ["/organizations", project_path.as_str()] {
        assert_eq!(
            request(
                address,
                "POST",
                path,
                json!({"name": "Revoked"}),
                Some(&owner_token)
            )
            .await
            .0,
            401
        );
    }
}

async fn check_save_snapshots(address: SocketAddr, pool: &PgPool, git: &crate::git::GitClient) {
    let mut users = Vec::new();
    for username in ["save-owner", "save-editor", "save-reader", "save-outsider"] {
        users.push(
            sqlx::query_scalar::<_, i32>(
                "INSERT INTO users (username, password_hash) VALUES ($1, 'unused') RETURNING id",
            )
            .bind(username)
            .fetch_one(pool)
            .await
            .unwrap(),
        );
    }
    let owner = users[0];
    let editor = users[1];
    let organization = crate::db::organizations::create_organization(pool, owner, "Save fixture")
        .await
        .unwrap();
    let mut roles = Vec::new();
    for (user, name) in [(editor, "Snapshot editor"), (users[2], "Snapshot reader")] {
        let role = sqlx::query_scalar::<_, i32>(
            "INSERT INTO roles (org_id, name) VALUES ($1, $2) RETURNING id",
        )
        .bind(organization.id)
        .bind(name)
        .fetch_one(pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO user_organization (org_id, user_id, role_id) VALUES ($1, $2, $3)")
            .bind(organization.id)
            .bind(user)
            .bind(role)
            .execute(pool)
            .await
            .unwrap();
        roles.push(role);
    }
    let (status, body) = request(
        address,
        "POST",
        &format!("/organizations/{}/projects", organization.id),
        json!({"name":"Upload target"}),
        Some(&token(owner)),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let project = serde_json::from_str::<Value>(&body).unwrap()["id"]
        .as_i64()
        .unwrap() as i32;
    for (role, permission) in [
        (roles[0], "edit_project"),
        (roles[1], "participate_in_deployment"),
    ] {
        sqlx::query(
            "INSERT INTO role_project_permission (org_id, role_id, project_id, perm_id)
            SELECT $1, $2, $3, id FROM permissions WHERE name = $4",
        )
        .bind(organization.id)
        .bind(role)
        .bind(project)
        .bind(permission)
        .execute(pool)
        .await
        .unwrap();
    }
    let path = format!("/projects/{project}/snapshots");
    let upload = json!({"files": [
        {"path":"train.py", "content":"print('uploaded')\r\n"},
        {"path":"src/space name.py", "content":"nested\n"},
        {"path":"binary.bin", "content_base64":"AP8B"},
        {"path":"run.sh", "content":"#!/bin/sh\necho ok\n", "executable":true}
    ]});
    for user in [users[2], users[3]] {
        assert_eq!(
            request(address, "POST", &path, upload.clone(), Some(&token(user)))
                .await
                .0,
            404
        );
    }
    assert_eq!(
        request(
            address,
            "POST",
            "/projects/2147483647/snapshots",
            upload.clone(),
            Some(&token(owner))
        )
        .await
        .0,
        404
    );
    assert!(git.list_snapshot_refs(project).await.unwrap().is_empty());
    let (status, body) = request(address, "POST", &path, upload.clone(), Some(&token(owner))).await;
    assert_eq!(status, 201, "{body}");
    let first = serde_json::from_str::<Value>(&body).unwrap();
    let first_id = first["id"].as_i64().unwrap();
    assert_eq!(first["project_id"], project);
    assert_eq!(first["created_by_user_id"], owner);
    assert_eq!(first["source"], "local");
    assert!(first["source_branch"].is_null());
    assert_eq!(first["git_commit_sha"].as_str().unwrap().len(), 40);
    let refs = git.list_snapshot_refs(project).await.unwrap();
    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].name, format!("refs/snapshots/{first_id}"));
    assert_eq!(refs[0].commit_sha, first["git_commit_sha"]);
    let (_, metadata) = request(
        address,
        "GET",
        &format!("{path}/{first_id}"),
        Value::Null,
        Some(&token(users[2])),
    )
    .await;
    assert_eq!(serde_json::from_str::<Value>(&metadata).unwrap(), first);
    let (_, file) = request(
        address,
        "GET",
        &format!("{path}/{first_id}/file?path=train.py"),
        Value::Null,
        Some(&token(owner)),
    )
    .await;
    assert_eq!(
        serde_json::from_str::<Value>(&file).unwrap()["content"],
        "print('uploaded')\r\n"
    );
    assert_eq!(
        request(
            address,
            "GET",
            &format!("{path}/{first_id}/file?path=binary.bin"),
            Value::Null,
            Some(&token(owner))
        )
        .await
        .0,
        415
    );

    // Each upload replaces the complete file tree for the new version only.
    let (status, body) = request(
        address,
        "POST",
        &path,
        json!({"files":[{"path":"new.py", "content":"new"}]}),
        Some(&token(editor)),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let second = serde_json::from_str::<Value>(&body).unwrap();
    assert_eq!(second["created_by_user_id"], editor);
    let second_id = second["id"].as_i64().unwrap();
    assert_eq!(
        request(
            address,
            "GET",
            &format!("{path}/{second_id}/file?path=train.py"),
            Value::Null,
            Some(&token(owner))
        )
        .await
        .0,
        404
    );
    assert_eq!(
        request(
            address,
            "GET",
            &format!("{path}/{first_id}/file?path=train.py"),
            Value::Null,
            Some(&token(owner))
        )
        .await
        .0,
        200
    );

    let before = git.list_snapshot_refs(project).await.unwrap();
    for (payload, expected) in [
        (json!({"files":[{"path":"../secret", "content":"x"}]}), 400),
        (
            json!({"files":[{"path":".git/config", "content":"x"}]}),
            400,
        ),
        (
            json!({"files":[{"path":"a", "content":"x"},{"path":"a", "content":"y"}]}),
            400,
        ),
        (
            json!({"files":[{"path":"a", "content":"x"},{"path":"a/file", "content":"y"}]}),
            400,
        ),
        (json!({"files":[{"path":"a", "content_base64":"%%%"}]}), 400),
        (
            json!({"files":[{"path":"a", "content":"x", "content_base64":"eA=="}]}),
            400,
        ),
        (json!({"files":[{"path":"a"}]}), 400),
        (json!({"files":[], "created_by_user_id":users[3]}), 422),
        (json!({"files":"invalid"}), 422),
        (
            json!({"files":vec![json!({"path":"a", "content":""}); crate::snapshots::types::MAX_SNAPSHOT_FILES + 1]}),
            413,
        ),
    ] {
        assert_eq!(
            request(address, "POST", &path, payload, Some(&token(owner)))
                .await
                .0,
            expected
        );
        assert_eq!(git.list_snapshot_refs(project).await.unwrap(), before);
    }
    // An editor cannot upload to another project or publish after losing editing rights.
    let other =
        crate::db::projects::create_project(pool, organization.id, owner, "Unshared project")
            .await
            .unwrap()
            .unwrap();
    assert_eq!(
        request(
            address,
            "POST",
            &format!("/projects/{}/snapshots", other.id),
            upload.clone(),
            Some(&token(editor))
        )
        .await
        .0,
        404
    );
    assert!(
        crate::db::snapshots::can_save_snapshot(pool, project, editor)
            .await
            .unwrap()
    );
    sqlx::query("DELETE FROM role_project_permission WHERE role_id = $1 AND project_id = $2")
        .bind(roles[0])
        .bind(project)
        .execute(pool)
        .await
        .unwrap();
    assert!(
        crate::db::snapshots::create_snapshot(
            pool,
            project,
            editor,
            first["git_commit_sha"].as_str().unwrap()
        )
        .await
        .unwrap()
        .is_none()
    );
    assert_eq!(
        request(address, "POST", &path, upload.clone(), Some(&token(editor)))
            .await
            .0,
        404
    );
    let revoked = token(owner);
    let claims = jsonwebtoken::decode::<Claims>(
        &revoked,
        &DecodingKey::from_secret(SECRET),
        &jsonwebtoken::Validation::default(),
    )
    .unwrap()
    .claims;
    sqlx::query(
        "INSERT INTO revoked_tokens (jti, expires_at) VALUES ($1, to_timestamp($2::double precision))",
    )
    .bind(claims.jti)
    .bind(claims.exp as f64)
    .execute(pool)
    .await
    .unwrap();
    assert_eq!(
        request(address, "POST", &path, upload.clone(), Some(&revoked))
            .await
            .0,
        401
    );
    assert_eq!(git.list_snapshot_refs(project).await.unwrap(), before);

    // Files larger than the preview limit are still saved, including requests over Axum's default 2 MiB.
    let large_size = crate::git::MAX_PREVIEW_BYTES + 1;
    let (status, body) = request(
        address,
        "POST",
        &path,
        json!({"files":[{"path":"large.py", "content":"x".repeat(large_size)}]}),
        Some(&token(owner)),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let large = serde_json::from_str::<Value>(&body).unwrap()["id"]
        .as_i64()
        .unwrap();
    let (status, body) = request(
        address,
        "GET",
        &format!("{path}/{large}/file?path=large.py"),
        Value::Null,
        Some(&token(owner)),
    )
    .await;
    assert_eq!(status, 413);
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["size_bytes"],
        large_size
    );

    // Git failures roll back metadata and never overwrite a retained reference.
    let collision = sqlx::query_scalar::<_, i64>("SELECT last_value + 1 FROM snapshots_id_seq")
        .fetch_one(pool)
        .await
        .unwrap();
    git.retain_snapshot(
        project,
        &collision.to_string(),
        first["git_commit_sha"].as_str().unwrap(),
    )
    .await
    .unwrap();
    let count =
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM snapshots WHERE project_id = $1")
            .bind(project)
            .fetch_one(pool)
            .await
            .unwrap();
    let refs = git.list_snapshot_refs(project).await.unwrap();
    assert_eq!(
        request(address, "POST", &path, upload.clone(), Some(&token(owner)))
            .await
            .0,
        500
    );
    assert_eq!(git.list_snapshot_refs(project).await.unwrap(), refs);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM snapshots WHERE project_id = $1")
            .bind(project)
            .fetch_one(pool)
            .await
            .unwrap(),
        count
    );
    assert_eq!(
        request(
            address,
            "POST",
            &format!("/projects/{}/snapshots", other.id),
            upload.clone(),
            Some(&token(owner))
        )
        .await
        .0,
        500
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM snapshots WHERE project_id = $1")
            .bind(other.id)
            .fetch_one(pool)
            .await
            .unwrap(),
        0
    );

    // A deferred database failure occurs after Git retention; do not report success
    // or remove existing snapshots. The unlisted reference remains for reconciliation.
    sqlx::raw_sql(
        "CREATE FUNCTION reject_snapshot_save() RETURNS trigger LANGUAGE plpgsql AS $$
        BEGIN RAISE EXCEPTION 'Test commit failure'; END; $$;
        CREATE CONSTRAINT TRIGGER reject_snapshot_save AFTER INSERT ON snapshots
        DEFERRABLE INITIALLY DEFERRED FOR EACH ROW EXECUTE FUNCTION reject_snapshot_save();",
    )
    .execute(pool)
    .await
    .unwrap();
    let (status, body) = request(address, "POST", &path, upload.clone(), Some(&token(owner))).await;
    assert_eq!(status, 500, "{body}");
    assert!(!body.contains("Test commit failure"));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM snapshots WHERE project_id = $1")
            .bind(project)
            .fetch_one(pool)
            .await
            .unwrap(),
        count
    );
    assert_eq!(
        request(
            address,
            "GET",
            &format!("{path}/{first_id}/file?path=train.py"),
            Value::Null,
            Some(&token(owner))
        )
        .await
        .0,
        200
    );
    sqlx::raw_sql(
        "DROP TRIGGER reject_snapshot_save ON snapshots; DROP FUNCTION reject_snapshot_save();",
    )
    .execute(pool)
    .await
    .unwrap();

    let make_upload = |number| json!({"files":[{"path":"concurrent.py", "content":format!("version {number}")} ]});
    let owner_token = token(owner);
    let results = tokio::join!(
        request(address, "POST", &path, make_upload(1), Some(&owner_token)),
        request(address, "POST", &path, make_upload(2), Some(&owner_token)),
        request(address, "POST", &path, make_upload(3), Some(&owner_token)),
        request(address, "POST", &path, make_upload(4), Some(&owner_token)),
        request(address, "POST", &path, make_upload(5), Some(&owner_token)),
    );
    let mut saved_ids = std::collections::HashSet::new();
    let refs = git.list_snapshot_refs(project).await.unwrap();
    for (status, body) in [results.0, results.1, results.2, results.3, results.4] {
        assert_eq!(status, 201, "{body}");
        let saved = serde_json::from_str::<Value>(&body).unwrap();
        let id = saved["id"].as_i64().unwrap();
        assert!(saved_ids.insert(id));
        assert!(
            refs.iter()
                .any(|reference| reference.name == format!("refs/snapshots/{id}")
                    && reference.commit_sha == saved["git_commit_sha"].as_str().unwrap())
        );
    }
    let (_, body) = request(address, "GET", &path, Value::Null, Some(&token(owner))).await;
    let listed = serde_json::from_str::<Value>(&body).unwrap();
    assert_eq!(
        listed["snapshots"][0]["id"].as_i64().unwrap(),
        *saved_ids.iter().max().unwrap()
    );
    let (status, body) = request(
        address,
        "POST",
        &path,
        json!({"files":[]}),
        Some(&token(owner)),
    )
    .await;
    assert_eq!(status, 201, "{body}");
    let empty_id = serde_json::from_str::<Value>(&body).unwrap()["id"]
        .as_i64()
        .unwrap();
    let (_, body) = request(
        address,
        "GET",
        &format!("{path}/{empty_id}/tree"),
        Value::Null,
        Some(&token(owner)),
    )
    .await;
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap()["entries"],
        json!([])
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM deployment_runs WHERE project_id = $1")
            .bind(project)
            .fetch_one(pool)
            .await
            .unwrap(),
        0
    );
    assert!(
        std::fs::read_dir(git.repository_root().parent().unwrap())
            .unwrap()
            .all(|entry| !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".capture-"))
    );
}

async fn check_snapshots(address: SocketAddr, pool: &PgPool, git: &crate::git::GitClient) {
    let mut users = Vec::new();
    for username in [
        "snapshot-owner",
        "snapshot-reader",
        "snapshot-unassigned",
        "snapshot-outsider",
    ] {
        users.push(
            sqlx::query_scalar::<_, i32>(
                "INSERT INTO users (username, password_hash) VALUES ($1, 'unused') RETURNING id",
            )
            .bind(username)
            .fetch_one(pool)
            .await
            .unwrap(),
        );
    }
    let owner = users[0];
    let reader = users[1];
    let organization =
        crate::db::organizations::create_organization(pool, owner, "Snapshot fixture")
            .await
            .unwrap();
    let project = crate::db::projects::create_project(pool, organization.id, owner, "Snapshots")
        .await
        .unwrap()
        .unwrap();
    let other =
        crate::db::projects::create_project(pool, organization.id, owner, "Other snapshots")
            .await
            .unwrap()
            .unwrap();
    let role = sqlx::query_scalar::<_, i32>(
        "INSERT INTO roles (org_id, name) VALUES ($1, 'Snapshot participant') RETURNING id",
    )
    .bind(organization.id)
    .fetch_one(pool)
    .await
    .unwrap();
    for user in [reader, users[2]] {
        sqlx::query("INSERT INTO user_organization (org_id, user_id, role_id) VALUES ($1, $2, $3)")
            .bind(organization.id)
            .bind(user)
            .bind(if user == reader { Some(role) } else { None })
            .execute(pool)
            .await
            .unwrap();
    }
    sqlx::query(
        "INSERT INTO role_project_permission (org_id, role_id, project_id, perm_id)
                 SELECT $1, $2, $3, id FROM permissions WHERE name='participate_in_deployment'",
    )
    .bind(organization.id)
    .bind(role)
    .bind(project.id)
    .execute(pool)
    .await
    .unwrap();
    let source = git
        .repository_root()
        .parent()
        .unwrap()
        .join("snapshot-source");
    std::fs::create_dir_all(source.join("src")).unwrap();
    std::fs::write(source.join("train.py"), "print('snapshot')\n").unwrap();
    std::fs::write(source.join("src/space name.txt"), "nested contents\n").unwrap();
    std::fs::write(source.join("binary.bin"), [0, 1]).unwrap();
    std::fs::write(
        source.join("large.txt"),
        vec![b'x'; crate::git::MAX_PREVIEW_BYTES + 1],
    )
    .unwrap();
    for args in [
        vec!["init", "--quiet"],
        vec!["add", "--all"],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@localhost",
            "commit",
            "--quiet",
            "-m",
            "Snapshot",
        ],
    ] {
        assert!(
            tokio::process::Command::new("git")
                .arg("-C")
                .arg(&source)
                .args(args)
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .output()
                .await
                .unwrap()
                .status
                .success()
        );
    }
    let head = tokio::process::Command::new("git")
        .arg("-C")
        .arg(&source)
        .args(["rev-parse", "HEAD"])
        .output()
        .await
        .unwrap();
    let sha = String::from_utf8(head.stdout).unwrap().trim().to_owned();
    let source_sha = "b".repeat(40);
    let snapshot_id = sqlx::query_scalar::<_, i32>(
        "INSERT INTO snapshots (project_id, created_by_user_id, source, source_branch,
                                source_commit_sha, git_commit_sha, created_at)
         VALUES ($1, $2, 'github', 'main', $3, $4, '2026-10-01 00:00:00+00') RETURNING id",
    )
    .bind(project.id)
    .bind(owner)
    .bind(&source_sha)
    .bind(&sha)
    .fetch_one(pool)
    .await
    .unwrap();
    git.create_project_repository(project.id).await.unwrap();
    git.publish_snapshot(project.id, &snapshot_id.to_string(), &source, &sha)
        .await
        .unwrap();
    let path = format!("/projects/{}/snapshots/{snapshot_id}", project.id);
    for user in [owner, reader] {
        let (status, body) = request(address, "GET", &path, Value::Null, Some(&token(user))).await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap(),
            json!({
                "id": snapshot_id, "project_id": project.id, "created_by_user_id": owner,
                "source": "github", "source_branch": "main", "source_commit_sha": source_sha,
                "git_commit_sha": sha, "created_at": 1790812800000.0,
            })
        );
    }
    for (path, user) in [
        (path.clone(), users[2]),
        (path.clone(), users[3]),
        (
            format!("/projects/{}/snapshots/{snapshot_id}", other.id),
            owner,
        ),
        (
            format!("/projects/{}/snapshots/2147483647", project.id),
            owner,
        ),
        (
            format!("/projects/2147483647/snapshots/{snapshot_id}"),
            owner,
        ),
    ] {
        let (status, body) = request(address, "GET", &path, Value::Null, Some(&token(user))).await;
        assert_eq!(status, 404, "{body}");
        assert_eq!(body, "Snapshot not found.");
    }
    // Optional provenance is null for local snapshots.
    let local_id = sqlx::query_scalar::<_, i32>(
        "INSERT INTO snapshots (project_id, created_by_user_id, source, git_commit_sha)
         VALUES ($1, $2, 'local', $3) RETURNING id",
    )
    .bind(project.id)
    .bind(owner)
    .bind(&sha)
    .fetch_one(pool)
    .await
    .unwrap();
    let (status, body) = request(
        address,
        "GET",
        &format!("/projects/{}/snapshots/{local_id}", project.id),
        Value::Null,
        Some(&token(owner)),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let body: Value = serde_json::from_str(&body).unwrap();
    assert!(body["source_branch"].is_null());
    assert!(body["source_commit_sha"].is_null());
    for user in [owner, reader] {
        let (status, body) = request(
            address,
            "GET",
            &format!("{path}/tree"),
            Value::Null,
            Some(&token(user)),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        let tree: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(tree["path"], "");
        assert!(
            tree["entries"]
                .as_array()
                .unwrap()
                .iter()
                .any(|entry| entry["path"] == "src" && entry["kind"] == "directory")
        );
        let (status, body) = request(
            address,
            "GET",
            &format!("{path}/tree?path=src"),
            Value::Null,
            Some(&token(user)),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap()["entries"][0]["path"],
            "src/space name.txt"
        );
        let (status, body) = request(
            address,
            "GET",
            &format!("{path}/file?path=src%2Fspace%20name.txt"),
            Value::Null,
            Some(&token(user)),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        assert_eq!(
            serde_json::from_str::<Value>(&body).unwrap(),
            json!({"path":"src/space name.txt", "content":"nested contents\n"})
        );
    }
    for (suffix, expected) in [
        ("/tree?path=..", 400),
        ("/file?path=%2Fetc%2Fpasswd", 400),
        ("/tree?path=missing", 404),
        ("/file?path=missing", 404),
        ("/file?path=binary.bin", 415),
        ("/file?path=src", 415),
    ] {
        assert_eq!(
            request(
                address,
                "GET",
                &format!("{path}{suffix}"),
                Value::Null,
                Some(&token(owner))
            )
            .await
            .0,
            expected
        );
    }
    let (status, body) = request(
        address,
        "GET",
        &format!("{path}/file?path=large.txt"),
        Value::Null,
        Some(&token(owner)),
    )
    .await;
    assert_eq!(status, 413, "{body}");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap(),
        json!({
            "error": "preview_too_large",
            "size_bytes": crate::git::MAX_PREVIEW_BYTES + 1,
            "max_preview_bytes": crate::git::MAX_PREVIEW_BYTES,
        })
    );
    for suffix in ["/tree", "/file?path=train.py"] {
        for user in [users[2], users[3]] {
            assert_eq!(
                request(
                    address,
                    "GET",
                    &format!("{path}{suffix}"),
                    Value::Null,
                    Some(&token(user))
                )
                .await
                .0,
                404
            );
        }
        let wrong_project = format!("/projects/{}/snapshots/{snapshot_id}{suffix}", other.id);
        assert_eq!(
            request(
                address,
                "GET",
                &wrong_project,
                Value::Null,
                Some(&token(owner))
            )
            .await
            .0,
            404
        );
    }
    let backdated_id = sqlx::query_scalar::<_, i32>(
        "INSERT INTO snapshots (project_id, created_by_user_id, source, git_commit_sha, created_at)
         VALUES ($1, $2, 'local', $3, '2026-09-01 00:00:00+00') RETURNING id",
    )
    .bind(project.id)
    .bind(owner)
    .bind(&sha)
    .fetch_one(pool)
    .await
    .unwrap();
    let tied_id = sqlx::query_scalar::<_, i32>(
        "INSERT INTO snapshots (project_id, created_by_user_id, source, git_commit_sha, created_at)
         SELECT $1, $2, 'local', $3, created_at FROM snapshots WHERE id=$4 RETURNING id",
    )
    .bind(project.id)
    .bind(owner)
    .bind(&sha)
    .bind(local_id)
    .fetch_one(pool)
    .await
    .unwrap();
    let list_path = format!("/projects/{}/snapshots", project.id);
    for user in [owner, reader] {
        let (status, body) =
            request(address, "GET", &list_path, Value::Null, Some(&token(user))).await;
        assert_eq!(status, 200, "{body}");
        let list: Value = serde_json::from_str(&body).unwrap();
        let ids: Vec<_> = list["snapshots"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| row["id"].as_i64().unwrap())
            .collect();
        assert_eq!(
            ids,
            vec![
                i64::from(tied_id),
                i64::from(local_id),
                i64::from(snapshot_id),
                i64::from(backdated_id)
            ]
        );
        assert_eq!(list["has_more"], false);
    }
    for (query, expected_id, has_more) in [
        ("?limit=1", Some(tied_id), true),
        ("?limit=1&offset=1", Some(local_id), true),
        ("?offset=100", None, false),
    ] {
        let (status, body) = request(
            address,
            "GET",
            &format!("{list_path}{query}"),
            Value::Null,
            Some(&token(owner)),
        )
        .await;
        assert_eq!(status, 200, "{body}");
        let list: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(list["has_more"], has_more);
        if let Some(id) = expected_id {
            assert_eq!(list["snapshots"][0]["id"], id);
        } else {
            assert_eq!(list["snapshots"], json!([]));
        }
    }
    for limit in [0, 101] {
        assert_eq!(
            request(
                address,
                "GET",
                &format!("{list_path}?limit={limit}"),
                Value::Null,
                Some(&token(owner))
            )
            .await
            .0,
            400
        );
    }
    let (status, body) = request(
        address,
        "GET",
        &format!("/projects/{}/snapshots", other.id),
        Value::Null,
        Some(&token(owner)),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        serde_json::from_str::<Value>(&body).unwrap(),
        json!({"snapshots": [], "has_more": false})
    );
    for (project_id, user) in [
        (project.id, users[2]),
        (project.id, users[3]),
        (2147483647, owner),
    ] {
        assert_eq!(
            request(
                address,
                "GET",
                &format!("/projects/{project_id}/snapshots"),
                Value::Null,
                Some(&token(user))
            )
            .await
            .0,
            404
        );
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM deployment_runs WHERE project_id=$1")
            .bind(project.id)
            .fetch_one(pool)
            .await
            .unwrap(),
        0
    );

    // Access follows current grants, not who originally created the snapshot.
    sqlx::query("DELETE FROM role_project_permission WHERE role_id=$1 AND project_id=$2")
        .bind(role)
        .bind(project.id)
        .execute(pool)
        .await
        .unwrap();
    assert_eq!(
        request(address, "GET", &path, Value::Null, Some(&token(reader)))
            .await
            .0,
        404
    );
    for suffix in ["/tree", "/file?path=train.py"] {
        assert_eq!(
            request(
                address,
                "GET",
                &format!("{path}{suffix}"),
                Value::Null,
                Some(&token(reader))
            )
            .await
            .0,
            404
        );
    }
    assert_eq!(
        request(
            address,
            "GET",
            &list_path,
            Value::Null,
            Some(&token(reader))
        )
        .await
        .0,
        404
    );
    let revoked = token(owner);
    let state = AppState {
        pool: pool.clone(),
        encoding_key: EncodingKey::from_secret(SECRET),
        decoding_key: DecodingKey::from_secret(SECRET),
        git: crate::git::GitClient::test_config(),
    };
    let claims = crate::db::users::verify_user_token(&revoked, &state).unwrap();
    sqlx::query("INSERT INTO revoked_tokens (jti, expires_at) VALUES ($1, CURRENT_TIMESTAMP + interval '1 hour')")
        .bind(claims.jti).execute(pool).await.unwrap();
    assert_eq!(
        request(address, "GET", &list_path, Value::Null, Some(&revoked))
            .await
            .0,
        401
    );
    assert_eq!(
        request(address, "GET", &path, Value::Null, Some(&revoked))
            .await
            .0,
        401
    );
}

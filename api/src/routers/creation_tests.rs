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
    let app = Router::new()
        .merge(organizations_router())
        .merge(projects_router())
        .merge(users_router())
        .with_state(AppState {
            pool,
            encoding_key: EncodingKey::from_secret(SECRET),
            decoding_key: DecodingKey::from_secret(SECRET),
            git: crate::git::GitClient::test_config(),
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
    let (address, server) = serve(pool.clone()).await;
    let check_pool = pool.clone();
    // Run assertions in a task so a failed assertion still allows schema cleanup.
    let result = tokio::spawn(async move {
        check_creation(address, &check_pool).await;
        check_project_members(address, &check_pool).await;
    })
    .await;
    server.abort();
    pool.close().await;
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

async fn check_creation(address: SocketAddr, pool: &PgPool) {
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

use super::types::{Member, MemberList, Organization, Project, ProjectList};
use sqlx::PgPool;

pub async fn create_organization(
    pool: &PgPool,
    user_id: i32,
    name: &str,
) -> Result<Organization, sqlx::Error> {
    // Ownership requires membership; the deferred FK is checked at commit.
    let mut transaction = pool.begin().await?;
    let (id, name, owner_user_id) = sqlx::query_as::<_, (i32, String, i32)>(
        "INSERT INTO organizations (name, owner_user_id)
         VALUES ($1, $2) RETURNING id, name, owner_user_id",
    )
    .bind(name)
    .bind(user_id)
    .fetch_one(&mut *transaction)
    .await?;
    sqlx::query("INSERT INTO user_organization (user_id, org_id) VALUES ($1, $2)")
        .bind(user_id)
        .bind(id)
        .execute(&mut *transaction)
        .await?;
    transaction.commit().await?;
    Ok(Organization {
        id,
        name,
        owner_user_id,
    })
}

pub async fn get_organization_members(
    pool: &PgPool,
    org_id: i32,
    user_id: i32,
) -> Result<MemberList, sqlx::Error> {
    // Verify the caller's membership within the same statement that reads members.
    let rows = sqlx::query_as::<_, (i32, String, Option<i32>, Option<String>)>(
        "SELECT u.id, u.username, m.role_id, r.name
         FROM user_organization AS caller
         JOIN user_organization AS m ON m.org_id = caller.org_id
         JOIN users AS u ON u.id = m.user_id
         LEFT JOIN roles AS r ON r.org_id = m.org_id AND r.id = m.role_id
         WHERE caller.org_id = $1 AND caller.user_id = $2
         ORDER BY u.username, u.id",
    )
    .bind(org_id)
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(MemberList {
        members: rows
            .into_iter()
            .map(|(id, username, role_id, role_name)| Member {
                id,
                username,
                role_id,
                role_name,
            })
            .collect(),
    })
}

pub async fn get_organization_projects(
    pool: &PgPool,
    org_id: i32,
    user_id: i32,
) -> Result<ProjectList, sqlx::Error> {
    // Check membership and read projects from the same database snapshot.
    let rows = sqlx::query_as::<_, (i32, i32, i32, String)>(
        "SELECT p.id, p.org_id, p.created_by_user_id, p.name
         FROM user_organization AS membership
         JOIN projects AS p ON p.org_id = membership.org_id
         WHERE membership.org_id = $1 AND membership.user_id = $2
         ORDER BY p.name, p.id",
    )
    .bind(org_id)
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    let projects = rows
        .into_iter()
        .map(|(id, org_id, created_by_user_id, name)| Project {
            id,
            org_id,
            created_by_user_id,
            name,
        })
        .collect();
    Ok(ProjectList { projects })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL with demo fixtures"]
    async fn lists_projects_only_for_members_and_handles_empty_organizations() {
        let pool = PgPool::connect(&std::env::var("TEST_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let (user_id, org_id) = sqlx::query_as::<_, (i32, i32)>(
            "SELECT u.id, m.org_id FROM users u JOIN user_organization m ON m.user_id = u.id
             WHERE u.username = 'demo' ORDER BY m.org_id LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let result = get_organization_projects(&pool, org_id, user_id)
            .await
            .unwrap();
        let expected = sqlx::query_as::<_, (i32, String)>(
            "SELECT id, name FROM projects WHERE org_id = $1 ORDER BY name, id",
        )
        .bind(org_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        let actual: Vec<_> = result
            .projects
            .into_iter()
            .map(|p| (p.id, p.name))
            .collect();
        assert_eq!(actual, expected);
        let outsider =
            sqlx::query_scalar::<_, i32>("SELECT id FROM users WHERE username = 'no-memberships'")
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(
            get_organization_projects(&pool, org_id, outsider)
                .await
                .unwrap()
                .projects
                .is_empty()
        );
        assert!(
            get_organization_projects(&pool, -1, user_id)
                .await
                .unwrap()
                .projects
                .is_empty()
        );
        pool.close().await;
    }
}

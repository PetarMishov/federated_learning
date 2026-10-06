use super::types::{DBError, Project, ProjectList};
use sqlx::PgPool;

pub async fn get_organization_projects(
    pool: &PgPool,
    org_id: i32,
    user_id: i32,
) -> Result<Option<ProjectList>, DBError> {
    // Check membership and read projects from the same database snapshot.
    // The left join distinguishes an empty organization from inaccessible projects.
    let rows = sqlx::query_as::<_, (Option<i32>, Option<i32>, Option<i32>, Option<String>)>(
        "SELECT p.id, p.org_id, p.created_by_user_id, p.name
         FROM user_organization AS membership
         LEFT JOIN projects AS p ON p.org_id = membership.org_id
         WHERE membership.org_id = $1 AND membership.user_id = $2
         ORDER BY p.name, p.id",
    )
    .bind(org_id)
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    if rows.is_empty() {
        return Ok(None);
    }
    let projects = rows
        .into_iter()
        .filter_map(|(id, org_id, created_by_user_id, name)| {
            Some(Project {
                id: id?,
                org_id: org_id?,
                created_by_user_id: created_by_user_id?,
                name: name?,
            })
        })
        .collect();
    Ok(Some(ProjectList { projects }))
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
            .unwrap()
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
                .is_none()
        );
        assert!(
            get_organization_projects(&pool, -1, user_id)
                .await
                .unwrap()
                .is_none()
        );
        pool.close().await;
    }
}

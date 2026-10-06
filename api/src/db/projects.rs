use super::types::{DBError, Deployment, DeploymentList};
use sqlx::PgPool;

pub async fn get_project_deployments(
    pool: &PgPool,
    proj_id: i32,
    user_id: i32,
) -> Result<DeploymentList, DBError> {
    // Read deployments and check current organization membership in one statement.
    let deployments = sqlx::query_as::<_, Deployment>(
        "SELECT d.id, d.org_id, d.project_id, d.snapshot_id, d.name,
                d.status::text AS status, d.created_by_user_id,
                (EXTRACT(EPOCH FROM d.created_at) * 1000)::double precision AS created_at,
                (EXTRACT(EPOCH FROM d.started_at) * 1000)::double precision AS started_at,
                (EXTRACT(EPOCH FROM d.ended_at) * 1000)::double precision AS ended_at
         FROM deployment_runs AS d
         JOIN user_organization AS membership ON membership.org_id = d.org_id
         WHERE d.project_id = $1 AND membership.user_id = $2
         ORDER BY d.created_at DESC, d.id DESC",
    )
    .bind(proj_id)
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(DeploymentList { deployments })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL with demo fixtures"]
    async fn lists_only_requested_project_deployments_for_organization_members() {
        let pool = PgPool::connect(&std::env::var("TEST_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let (project_id, org_id, user_id) = sqlx::query_as::<_, (i32, i32, i32)>(
            "SELECT p.id, p.org_id, m.user_id FROM projects p
             JOIN user_organization m ON m.org_id = p.org_id
             JOIN users u ON u.id = m.user_id
             WHERE u.username = 'demo'
               AND EXISTS (SELECT 1 FROM deployment_runs d WHERE d.project_id = p.id)
             ORDER BY p.id LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let result = get_project_deployments(&pool, project_id, user_id)
            .await
            .unwrap();
        let expected = sqlx::query_as::<_, (i32, String)>(
            "SELECT id, status::text FROM deployment_runs
             WHERE project_id = $1 ORDER BY created_at DESC, id DESC",
        )
        .bind(project_id)
        .fetch_all(&pool)
        .await
        .unwrap();
        assert!(!expected.is_empty());
        assert!(
            result
                .deployments
                .iter()
                .all(|d| d.project_id == project_id)
        );
        let actual: Vec<_> = result
            .deployments
            .into_iter()
            .map(|d| (d.id, d.status))
            .collect();
        assert_eq!(actual, expected);

        let outsider = sqlx::query_scalar::<_, i32>(
            "SELECT id FROM users u WHERE NOT EXISTS (
                SELECT 1 FROM user_organization m WHERE m.org_id = $1 AND m.user_id = u.id
             ) ORDER BY id LIMIT 1",
        )
        .bind(org_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        for (project, user) in [(project_id, outsider), (-1, user_id)] {
            assert!(
                get_project_deployments(&pool, project, user)
                    .await
                    .unwrap()
                    .deployments
                    .is_empty()
            );
        }
        pool.close().await;
    }
}

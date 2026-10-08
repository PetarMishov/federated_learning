use super::types::{DBError, Snapshot};
use sqlx::PgPool;

pub async fn get_snapshot(
    pool: &PgPool,
    project_id: i32,
    snapshot_id: i32,
    user_id: i32,
) -> Result<Option<Snapshot>, DBError> {
    // Check snapshot ownership and current project access in the same query.
    // Organization membership alone does not grant access to project files.
    Ok(sqlx::query_as::<_, Snapshot>(
        "SELECT s.id, s.project_id, s.created_by_user_id, s.source::text AS source,
                s.source_branch, s.source_commit_sha, s.git_commit_sha,
                (EXTRACT(EPOCH FROM s.created_at) * 1000)::double precision AS created_at
         FROM snapshots s
         JOIN projects p ON p.id = s.project_id
         JOIN organizations o ON o.id = p.org_id
         JOIN user_organization caller ON caller.org_id = p.org_id AND caller.user_id = $3
         WHERE p.id = $1 AND s.id = $2 AND (
             o.owner_user_id = $3 OR EXISTS (
                 SELECT 1 FROM role_project_permission g
                 WHERE g.org_id = p.org_id AND g.project_id = p.id AND g.role_id = caller.role_id
             )
         )",
    )
    .bind(project_id)
    .bind(snapshot_id)
    .bind(user_id)
    .fetch_optional(pool)
    .await?)
}

use super::types::{Snapshot, SnapshotList};
use sqlx::{FromRow, PgPool, Row};

pub async fn can_save_snapshot(
    pool: &PgPool,
    project_id: i32,
    user_id: i32,
) -> Result<bool, sqlx::Error> {
    Ok(
        sqlx::query_scalar("SELECT has_project_permission($1, $2, 'edit_project')")
            .bind(user_id)
            .bind(project_id)
            .fetch_one(pool)
            .await?,
    )
}

pub async fn create_snapshot<'a>(
    executor: impl sqlx::PgExecutor<'a>,
    project_id: i32,
    user_id: i32,
    git_commit_sha: &str,
) -> Result<Option<Snapshot>, sqlx::Error> {
    // Recheck editing rights at publication, retaining the authorization rows
    // until commit so membership, ownership, and grants cannot change mid-save.
    Ok(sqlx::query_as::<_, Snapshot>(
        "WITH authorized_project AS (
             SELECT p.id FROM projects p
             JOIN organizations o ON o.id = p.org_id
             JOIN user_organization caller ON caller.org_id = p.org_id AND caller.user_id = $2
             WHERE p.id = $1 AND (o.owner_user_id = $2 OR EXISTS (
                 SELECT 1 FROM role_project_permission g
                 JOIN permissions perm ON perm.id = g.perm_id
                 WHERE g.org_id = p.org_id AND g.project_id = p.id
                   AND g.role_id = caller.role_id AND perm.name = 'edit_project'
                 FOR SHARE OF g
             ))
             FOR SHARE OF p, o, caller
         )
         INSERT INTO snapshots (project_id, created_by_user_id, source, git_commit_sha, created_at)
         SELECT id, $2, 'local', $3, clock_timestamp() FROM authorized_project
         RETURNING id, project_id, created_by_user_id, source::text AS source,
                   source_branch, source_commit_sha, git_commit_sha,
                   (EXTRACT(EPOCH FROM created_at) * 1000)::double precision AS created_at",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(git_commit_sha)
    .fetch_optional(executor)
    .await?)
}

pub async fn snapshot_was_saved<'a>(
    executor: impl sqlx::PgExecutor<'a>,
    project_id: i32,
    snapshot_id: i32,
    git_commit_sha: &str,
) -> Result<bool, sqlx::Error> {
    // Internal reconciliation only; HTTP reads must use get_snapshot instead.
    Ok(sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM snapshots WHERE project_id = $1 AND id = $2 AND git_commit_sha = $3)",
    )
    .bind(project_id)
    .bind(snapshot_id)
    .bind(git_commit_sha)
    .fetch_one(executor)
    .await?)
}

pub async fn get_snapshots(
    pool: &PgPool,
    project_id: i32,
    user_id: i32,
    limit: u32,
    offset: u32,
) -> Result<Option<SnapshotList>, sqlx::Error> {
    // The left join distinguishes an authorized empty project from no access,
    // while keeping authorization and snapshot selection in one statement.
    let rows = sqlx::query(
        "WITH authorized_project AS (
             SELECT p.id FROM projects p
             JOIN organizations o ON o.id = p.org_id
             JOIN user_organization caller ON caller.org_id = p.org_id AND caller.user_id = $2
             WHERE p.id = $1 AND (o.owner_user_id = $2 OR EXISTS (
                 SELECT 1 FROM role_project_permission g
                 WHERE g.org_id = p.org_id AND g.project_id = p.id AND g.role_id = caller.role_id
             ))
         )
         SELECT s.id, s.project_id, s.created_by_user_id, s.source::text AS source,
                s.source_branch, s.source_commit_sha, s.git_commit_sha,
                (EXTRACT(EPOCH FROM s.created_at) * 1000)::double precision AS created_at
         FROM authorized_project p
         LEFT JOIN LATERAL (
             SELECT * FROM snapshots WHERE project_id = p.id
             ORDER BY created_at DESC, id DESC LIMIT $3 OFFSET $4
         ) s ON true
         ORDER BY s.created_at DESC, s.id DESC",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(i64::from(limit) + 1)
    .bind(i64::from(offset))
    .fetch_all(pool)
    .await?;
    if rows.is_empty() {
        return Ok(None);
    }
    let mut snapshots = Vec::new();
    for row in rows {
        if row.try_get::<Option<i32>, _>("id")?.is_some() {
            snapshots.push(Snapshot::from_row(&row)?);
        }
    }
    let has_more = snapshots.len() > limit as usize;
    snapshots.truncate(limit as usize);
    Ok(Some(SnapshotList {
        snapshots,
        has_more,
    }))
}

pub async fn get_snapshot(
    pool: &PgPool,
    project_id: i32,
    snapshot_id: i32,
    user_id: i32,
) -> Result<Option<Snapshot>, sqlx::Error> {
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

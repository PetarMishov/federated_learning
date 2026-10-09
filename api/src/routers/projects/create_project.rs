use crate::{
    auth::verify_user_credentials,
    db::{projects, types::Project},
    routers::types::CreateNameRequest,
    state::AppState,
};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};

// Separate namespace from other application advisory locks.
const PROJECT_CREATION_LOCK: i32 = 0x464c5052;

pub async fn create_project_request(
    State(state): State<AppState>,
    Path(org_id): Path<i32>,
    headers: HeaderMap,
    Json(request): Json<CreateNameRequest>,
) -> Result<(StatusCode, Json<Project>), (StatusCode, &'static str)> {
    let user_id = verify_user_credentials(headers, &state).await?;
    let name = request.validated_name()?;
    let creation = create_project_with_repository(&state, org_id, user_id, name).await;
    let project = creation
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Could not create project.",
            )
        })?
        .ok_or((
            StatusCode::FORBIDDEN,
            "Only the organization owner can create projects.",
        ))?;
    Ok((StatusCode::CREATED, Json(project)))
}

async fn create_project_with_repository(
    state: &AppState,
    org_id: i32,
    user_id: i32,
    name: &str,
) -> Result<Option<Project>, Box<dyn std::error::Error + Send + Sync>> {
    let mut transaction = state.pool.begin().await?;
    let Some(project) = projects::create_project(&mut *transaction, org_id, user_id, name).await?
    else {
        transaction.rollback().await?;
        return Ok(None); // basically someone who isnt allow to make the project tried to make it
    };
    lock_project(&mut transaction, project.id).await?;
    state.git.create_new_project_repository(project.id).await?;

    match transaction.commit().await {
        Ok(()) => Ok(Some(project)),
        Err(commit_error) => {
            let mut check = state.pool.begin().await?;
            lock_project(&mut check, project.id).await?;
            let exists = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS (SELECT 1 FROM projects WHERE id = $1)",
            )
            .bind(project.id)
            .fetch_one(&mut *check)
            .await?;
            if exists {
                Ok(Some(project))
            } else {
                state.git.remove_project_repository(project.id)?;
                Err(commit_error.into())
            }
            // If checking fails, retain storage for later reconciliation.
            // Never delete a repository on an unknown commit outcome.
        }
    }
}

async fn lock_project(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    project_id: i32,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT pg_advisory_xact_lock($1, $2)")
        .bind(PROJECT_CREATION_LOCK)
        .bind(project_id)
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

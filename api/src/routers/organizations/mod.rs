mod get_organization_members;
mod get_organization_projects;

use axum::{Router, routing::get};

use crate::state::AppState;

pub fn organizations_router() -> Router<AppState> {
    Router::new()
        .route(
            "/{org_id}/projects",
            get(get_organization_projects::get_organization_projects_request),
        )
        .route(
            "/{org_id}/members",
            get(get_organization_members::get_organization_members_request),
        )
}

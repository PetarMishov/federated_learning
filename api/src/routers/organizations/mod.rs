mod create_organization;
mod get_organization_members;
mod get_organization_projects;

use axum::{
    Router,
    routing::{get, post},
};

use crate::state::AppState;

pub fn organizations_router() -> Router<AppState> {
    Router::new()
        .route(
            "/organizations",
            post(create_organization::create_organization_request),
        )
        .route(
            "/organizations/{org_id}/projects",
            get(get_organization_projects::get_organization_projects_request),
        )
        .route(
            "/organizations/{org_id}/members",
            get(get_organization_members::get_organization_members_request),
        )
}

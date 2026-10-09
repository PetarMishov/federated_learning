use serde::Serialize;

#[derive(Serialize)]
pub struct OrganizationList {
    pub organizations: Vec<Organization>,
}

#[derive(Serialize)]
pub struct Organization {
    pub id: i32,
    pub name: String,
    pub owner_user_id: i32,
}

#[derive(Serialize)]
pub struct NotificationList {
    pub notifications: Vec<Notification>,
}

#[derive(Serialize)]
pub struct Notification {
    pub id: i32,
    pub title: String,
    pub message: String,
    pub is_read: bool,
    pub created_at: f64,
}

#[derive(Serialize)]
pub struct ProjectList {
    pub projects: Vec<Project>,
}

#[derive(Serialize)]
pub struct Project {
    pub id: i32,
    pub org_id: i32,
    pub created_by_user_id: i32,
    pub name: String,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct Snapshot {
    pub id: i32,
    pub project_id: i32,
    pub created_by_user_id: i32,
    pub source: String,
    pub source_branch: Option<String>,
    pub source_commit_sha: Option<String>,
    pub git_commit_sha: String,
    pub created_at: f64,
}

#[derive(Serialize)]
pub struct SnapshotList {
    pub snapshots: Vec<Snapshot>,
    pub has_more: bool,
}

#[derive(Serialize)]
pub struct MemberList {
    pub members: Vec<Member>,
}

#[derive(Serialize)]
pub struct Member {
    pub id: i32,
    pub username: String,
    pub role_id: Option<i32>,
    pub role_name: Option<String>,
}

#[derive(Serialize)]
pub struct DeploymentList {
    pub deployments: Vec<Deployment>,
}

#[derive(Serialize, sqlx::FromRow)]
pub struct Deployment {
    pub id: i32,
    pub org_id: i32,
    pub project_id: i32,
    pub snapshot_id: i32,
    pub name: String,
    pub status: String,
    pub created_by_user_id: i32,
    pub created_at: f64,
    pub started_at: Option<f64>,
    pub ended_at: Option<f64>,
}

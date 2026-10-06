use serde::Serialize;

pub type DBError = Box<dyn std::error::Error + Send + Sync>;

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
    pub title: String, //NOTE: title is content
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

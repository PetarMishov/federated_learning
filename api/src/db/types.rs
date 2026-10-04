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
}

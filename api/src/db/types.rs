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

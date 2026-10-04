use super::types::{DBError, Organization, OrganizationList};
use sqlx::PgPool;

pub async fn get_user_organizations(
    pool: &PgPool,
    user_id: i32,
) -> Result<OrganizationList, DBError> {
    let rows = sqlx::query_as::<_, (i32, String, i32)>(
        "SELECT o.id, o.name, o.owner_user_id
         FROM organizations AS o
         JOIN user_organization AS membership ON membership.org_id = o.id
         WHERE membership.user_id = $1
         ORDER BY o.name, o.id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(OrganizationList {
        organizations: rows
            .into_iter()
            .map(|(id, name, owner_user_id)| Organization {
                id,
                name,
                owner_user_id,
            })
            .collect(),
    })
}

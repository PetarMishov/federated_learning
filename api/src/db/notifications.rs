use sqlx::PgPool;

use crate::db::types::{DBError, Notification, NotificationList};

pub async fn get_user_notifications(
    pool: &PgPool,
    user_id: i32,
) -> Result<NotificationList, DBError> {
    let rows = sqlx::query_as::<_, (i32, String, bool, f64)>(
        "SELECT n.id, n.title, n.read_at IS NOT NULL AS is_read,
                (EXTRACT(EPOCH FROM n.created_at) * 1000)::double precision AS created_at
         FROM notifications AS n
         WHERE n.user_id = $1
         ORDER BY n.title, n.id",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;

    Ok(NotificationList {
        notifications: rows
            .into_iter()
            .map(|(id, title, is_read, created_at)| Notification {
                id,
                title,
                is_read,
                created_at,
            })
            .collect(),
    })
}

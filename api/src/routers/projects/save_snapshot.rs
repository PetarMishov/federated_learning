use axum::http::StatusCode;

pub async fn save_snapshot_request() -> (StatusCode, &'static str) {
    // TODO: Authenticate, authorize project access, and implement this snapshot endpoint.
    (
        StatusCode::NOT_IMPLEMENTED,
        "Snapshot endpoint is not implemented yet.",
    )
}

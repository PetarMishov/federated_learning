#[derive(Default, serde::Deserialize)]
pub struct SnapshotPath {
    #[serde(default)]
    pub path: String,
}

#[derive(serde::Deserialize)]
pub struct SnapshotPage {
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default)]
    pub offset: u32,
}

fn default_limit() -> u32 {
    50
}

#[derive(serde::Serialize)]
pub struct SnapshotPreviewTooLarge {
    pub error: &'static str,
    pub size_bytes: usize,
    pub max_preview_bytes: usize,
}

/// HTTP request limit, independent of the editor's per-file preview limit.
pub const MAX_SNAPSHOT_REQUEST_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_SNAPSHOT_FILES: usize = 10_000;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SaveSnapshotRequest {
    pub base_snapshot_id: Option<i32>,
    pub files: Vec<SnapshotUploadFile>,
    #[serde(default)]
    pub operations: Vec<crate::git::GitSnapshotOperation>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotUploadFile {
    pub path: String,
    pub content: Option<String>,
    pub content_base64: Option<String>,
    #[serde(default)]
    pub executable: bool,
}

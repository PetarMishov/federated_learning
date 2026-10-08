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

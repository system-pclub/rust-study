#[derive(Debug, Default, Deserialize)]
pub struct PackageConfig {
    pub version: Option<String>,
    pub git: Option<String>,
    pub path: Option<String>,
}

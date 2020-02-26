use std::collections::BTreeMap;

mod general;
pub(crate) mod file;
mod package;
mod user;

#[derive(Debug, Default, Deserialize)]
pub struct Config {
    pub general: general::GeneralConfig,
    #[serde(default)]
    pub packages: BTreeMap<String, package::PackageConfig>,
    #[serde(default)]
    pub files: Vec<file::FileConfig>,
    #[serde(default)]
    pub users: BTreeMap<String, user::UserConfig>,
}

pub use self::dependency::Dependency;
pub use self::features::{
    enable_nightly_features, maybe_allow_nightly_features, nightly_features_allowed,
};
pub use self::features::{CliUnstable, Edition, Feature, Features};
pub use self::interning::InternedString;
pub use self::manifest::{EitherManifest, VirtualManifest};
pub use self::manifest::{LibKind, Manifest, Target, TargetKind};
pub use self::package::{Package, PackageSet};
pub use self::package_id::PackageId;
pub use self::package_id_spec::PackageIdSpec;
pub use self::registry::Registry;
pub use self::resolver::{Resolve, ResolveVersion};
pub use self::shell::{Shell, Verbosity};
pub use self::source::{GitReference, Source, SourceId, SourceMap};
pub use self::summary::{FeatureMap, FeatureValue, Summary};
pub use self::workspace::{Members, Workspace, WorkspaceConfig, WorkspaceRootConfig};

pub mod compiler;
pub mod dependency;
pub mod features;
mod interning;
pub mod manifest;
pub mod package;
pub mod package_id;
mod package_id_spec;
pub mod profiles;
pub mod registry;
pub mod resolver;
pub mod shell;
pub mod source;
pub mod summary;
mod workspace;

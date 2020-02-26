use super::Scripter;
use super::Tarballer;
use crate::util::*;
use crate::Result;
use failure::{format_err, bail, ResultExt};
use std::io::Write;
use std::path::Path;

actor! {
    #[derive(Debug)]
    pub struct Generator {
        /// The name of the product, for display
        product_name: String = "Product",

        /// The name of the component, distinct from other installed components
        component_name: String = "component",

        /// The name of the package, tarball
        package_name: String = "package",

        /// The directory under lib/ where the manifest lives
        rel_manifest_dir: String = "packagelib",

        /// The string to print after successful installation
        success_message: String = "Installed.",

        /// Places to look for legacy manifests to uninstall
        legacy_manifest_dirs: String = "",

        /// Directory containing files that should not be installed
        non_installed_overlay: String = "",

        /// Path prefixes of directories that should be installed/uninstalled in bulk
        bulk_dirs: String = "",

        /// The directory containing the installation medium
        image_dir: String = "./install_image",

        /// The directory to do temporary work
        work_dir: String = "./workdir",

        /// The location to put the final image and tarball
        output_dir: String = "./dist",
    }
}

impl Generator {
    /// Generates the actual installer tarball
    pub fn run(self) -> Result<()> {
        create_dir_all(&self.work_dir)?;

        let package_dir = Path::new(&self.work_dir).join(&self.package_name);
        if package_dir.exists() {
            remove_dir_all(&package_dir)?;
        }

        // Copy the image and write the manifest
        let component_dir = package_dir.join(&self.component_name);
        create_dir_all(&component_dir)?;
        copy_and_manifest(self.image_dir.as_ref(), &component_dir, &self.bulk_dirs)?;

        // Write the component name
        let components = package_dir.join("components");
        writeln!(create_new_file(components)?, "{}", self.component_name)
            .with_context(|_| "failed to write the component file")?;

        // Write the installer version (only used by combine-installers.sh)
        let version = package_dir.join("rust-installer-version");
        writeln!(
            create_new_file(version)?,
            "{}",
            crate::RUST_INSTALLER_VERSION
        )
        .with_context(|_| "failed to write new installer version")?;

        // Copy the overlay
        if !self.non_installed_overlay.is_empty() {
            copy_recursive(self.non_installed_overlay.as_ref(), &package_dir)?;
        }

        // Generate the install script
        let output_script = package_dir.join("install.sh");
        let mut scripter = Scripter::default();
        scripter
            .product_name(self.product_name)
            .rel_manifest_dir(self.rel_manifest_dir)
            .success_message(self.success_message)
            .legacy_manifest_dirs(self.legacy_manifest_dirs)
            .output_script(path_to_str(&output_script)?);
        scripter.run()?;

        // Make the tarballs
        create_dir_all(&self.output_dir)?;
        let output = Path::new(&self.output_dir).join(&self.package_name);
        let mut tarballer = Tarballer::default();
        tarballer
            .work_dir(self.work_dir)
            .input(self.package_name)
            .output(path_to_str(&output)?);
        tarballer.run()?;

        Ok(())
    }
}

/// Copies the `src` directory recursively to `dst`, writing `manifest.in` too.
fn copy_and_manifest(src: &Path, dst: &Path, bulk_dirs: &str) -> Result<()> {
    let manifest = create_new_file(dst.join("manifest.in"))?;
    let bulk_dirs: Vec<_> = bulk_dirs
        .split(',')
        .filter(|s| !s.is_empty())
        .map(Path::new)
        .collect();

    copy_with_callback(src, dst, |path, file_type| {
        // We need paths to be compatible with both Unix and Windows.
        if path
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .any(|s| s.contains('\\'))
        {
            bail!(
                "rust-installer doesn't support '\\' in path components: {:?}",
                path
            );
        }

        // Normalize to Unix-style path separators.
        let normalized_string;
        let mut string = path.to_str().ok_or_else(|| {
            format_err!(
                "rust-installer doesn't support non-Unicode paths: {:?}",
                path
            )
        })?;
        if string.contains('\\') {
            normalized_string = string.replace('\\', "/");
            string = &normalized_string;
        }

        if file_type.is_dir() {
            // Only manifest directories that are explicitly bulk.
            if bulk_dirs.contains(&path) {
                writeln!(&manifest, "dir:{}", string)?;
            }
        } else {
            // Only manifest files that aren't under bulk directories.
            if !bulk_dirs.iter().any(|d| path.starts_with(d)) {
                writeln!(&manifest, "file:{}", string)?;
            }
        }
        Ok(())
    })
}

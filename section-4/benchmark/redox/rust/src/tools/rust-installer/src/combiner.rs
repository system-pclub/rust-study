use super::Scripter;
use super::Tarballer;
use crate::util::*;
use crate::Result;
use failure::{bail, ResultExt};
use flate2::read::GzDecoder;
use std::io::{Read, Write};
use std::path::Path;
use tar::Archive;

actor! {
    #[derive(Debug)]
    pub struct Combiner {
        /// The name of the product, for display.
        product_name: String = "Product",

        /// The name of the package  tarball.
        package_name: String = "package",

        /// The directory under lib/ where the manifest lives.
        rel_manifest_dir: String = "packagelib",

        /// The string to print after successful installation.
        success_message: String = "Installed.",

        /// Places to look for legacy manifests to uninstall.
        legacy_manifest_dirs: String = "",

        /// Installers to combine.
        input_tarballs: String = "",

        /// Directory containing files that should not be installed.
        non_installed_overlay: String = "",

        /// The directory to do temporary work.
        work_dir: String = "./workdir",

        /// The location to put the final image and tarball.
        output_dir: String = "./dist",
    }
}

impl Combiner {
    /// Combines the installer tarballs.
    pub fn run(self) -> Result<()> {
        create_dir_all(&self.work_dir)?;

        let package_dir = Path::new(&self.work_dir).join(&self.package_name);
        if package_dir.exists() {
            remove_dir_all(&package_dir)?;
        }
        create_dir_all(&package_dir)?;

        // Merge each installer into the work directory of the new installer.
        let components = create_new_file(package_dir.join("components"))?;
        for input_tarball in self
            .input_tarballs
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            // Extract the input tarballs
            let tar = GzDecoder::new(open_file(&input_tarball)?);
            Archive::new(tar).unpack(&self.work_dir).with_context(|_| {
                format!(
                    "unable to extract '{}' into '{}'",
                    &input_tarball, self.work_dir
                )
            })?;

            let pkg_name = input_tarball.trim_end_matches(".tar.gz");
            let pkg_name = Path::new(pkg_name).file_name().unwrap();
            let pkg_dir = Path::new(&self.work_dir).join(&pkg_name);

            // Verify the version number.
            let mut version = String::new();
            open_file(pkg_dir.join("rust-installer-version"))
                .and_then(|mut file| Ok(file.read_to_string(&mut version)?))
                .with_context(|_| format!("failed to read version in '{}'", input_tarball))?;
            if version.trim().parse() != Ok(crate::RUST_INSTALLER_VERSION) {
                bail!("incorrect installer version in {}", input_tarball);
            }

            // Copy components to the new combined installer.
            let mut pkg_components = String::new();
            open_file(pkg_dir.join("components"))
                .and_then(|mut file| Ok(file.read_to_string(&mut pkg_components)?))
                .with_context(|_| format!("failed to read components in '{}'", input_tarball))?;
            for component in pkg_components.split_whitespace() {
                // All we need to do is copy the component directory. We could
                // move it, but rustbuild wants to reuse the unpacked package
                // dir for OS-specific installers on macOS and Windows.
                let component_dir = package_dir.join(&component);
                create_dir(&component_dir)?;
                copy_recursive(&pkg_dir.join(&component), &component_dir)?;

                // Merge the component name.
                writeln!(&components, "{}", component)
                    .with_context(|_| "failed to write new components")?;
            }
        }
        drop(components);

        // Write the installer version.
        let version = package_dir.join("rust-installer-version");
        writeln!(
            create_new_file(version)?,
            "{}",
            crate::RUST_INSTALLER_VERSION
        )
        .with_context(|_| "failed to write new installer version")?;

        // Copy the overlay.
        if !self.non_installed_overlay.is_empty() {
            copy_recursive(self.non_installed_overlay.as_ref(), &package_dir)?;
        }

        // Generate the install script.
        let output_script = package_dir.join("install.sh");
        let mut scripter = Scripter::default();
        scripter
            .product_name(self.product_name)
            .rel_manifest_dir(self.rel_manifest_dir)
            .success_message(self.success_message)
            .legacy_manifest_dirs(self.legacy_manifest_dirs)
            .output_script(path_to_str(&output_script)?);
        scripter.run()?;

        // Make the tarballs.
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

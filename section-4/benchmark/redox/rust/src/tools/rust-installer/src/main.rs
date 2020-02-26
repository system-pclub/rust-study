use clap::{App, ArgMatches};
use failure::ResultExt;
use installer::Result;

fn main() -> Result<()> {
    let yaml = clap::load_yaml!("main.yml");
    let matches = App::from_yaml(yaml).get_matches();

    match matches.subcommand() {
        ("combine", Some(matches)) => combine(matches),
        ("generate", Some(matches)) => generate(matches),
        ("script", Some(matches)) => script(matches),
        ("tarball", Some(matches)) => tarball(matches),
        _ => unreachable!(),
    }
}

/// Parse clap arguements into the type constructor.
macro_rules! parse(
    ($matches:expr => $type:ty { $( $option:tt => $setter:ident, )* }) => {
        {
            let mut command: $type = Default::default();
            $( $matches.value_of($option).map(|s| command.$setter(s)); )*
            command
        }
    }
);

fn combine(matches: &ArgMatches<'_>) -> Result<()> {
    let combiner = parse!(matches => installer::Combiner {
        "product-name" => product_name,
        "package-name" => package_name,
        "rel-manifest-dir" => rel_manifest_dir,
        "success-message" => success_message,
        "legacy-manifest-dirs" => legacy_manifest_dirs,
        "input-tarballs" => input_tarballs,
        "non-installed-overlay" => non_installed_overlay,
        "work-dir" => work_dir,
        "output-dir" => output_dir,
    });

    combiner
        .run()
        .with_context(|_| "failed to combine installers")?;
    Ok(())
}

fn generate(matches: &ArgMatches<'_>) -> Result<()> {
    let generator = parse!(matches => installer::Generator {
        "product-name" => product_name,
        "component-name" => component_name,
        "package-name" => package_name,
        "rel-manifest-dir" => rel_manifest_dir,
        "success-message" => success_message,
        "legacy-manifest-dirs" => legacy_manifest_dirs,
        "non-installed-overlay" => non_installed_overlay,
        "bulk-dirs" => bulk_dirs,
        "image-dir" => image_dir,
        "work-dir" => work_dir,
        "output-dir" => output_dir,
    });

    generator
        .run()
        .with_context(|_| "failed to generate installer")?;
    Ok(())
}

fn script(matches: &ArgMatches<'_>) -> Result<()> {
    let scripter = parse!(matches => installer::Scripter {
        "product-name" => product_name,
        "rel-manifest-dir" => rel_manifest_dir,
        "success-message" => success_message,
        "legacy-manifest-dirs" => legacy_manifest_dirs,
        "output-script" => output_script,
    });

    scripter
        .run()
        .with_context(|_| "failed to generate installation script")?;
    Ok(())
}

fn tarball(matches: &ArgMatches<'_>) -> Result<()> {
    let tarballer = parse!(matches => installer::Tarballer {
        "input" => input,
        "output" => output,
        "work-dir" => work_dir,
    });

    tarballer
        .run()
        .with_context(|_| "failed to generate tarballs")?;
    Ok(())
}

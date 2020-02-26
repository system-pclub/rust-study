use crate::command_prelude::*;

use cargo::ops::{self, OutputMetadataOptions};
use cargo::print_json;

pub fn cli() -> App {
    subcommand("metadata")
        .about(
            "Output the resolved dependencies of a package, \
             the concrete used versions including overrides, \
             in machine-readable format",
        )
        .arg(opt("quiet", "No output printed to stdout").short("q"))
        .arg_features()
        .arg(
            opt(
                "filter-platform",
                "Only include resolve dependencies matching the given target-triple",
            )
            .value_name("TRIPLE"),
        )
        .arg(opt(
            "no-deps",
            "Output information only about the root package \
             and don't fetch dependencies",
        ))
        .arg_manifest_path()
        .arg(
            opt("format-version", "Format version")
                .value_name("VERSION")
                .possible_value("1"),
        )
}

pub fn exec(config: &mut Config, args: &ArgMatches<'_>) -> CliResult {
    let ws = args.workspace(config)?;

    let version = match args.value_of("format-version") {
        None => {
            config.shell().warn(
                "please specify `--format-version` flag explicitly \
                 to avoid compatibility problems",
            )?;
            1
        }
        Some(version) => version.parse().unwrap(),
    };

    let options = OutputMetadataOptions {
        features: values(args, "features"),
        all_features: args.is_present("all-features"),
        no_default_features: args.is_present("no-default-features"),
        no_deps: args.is_present("no-deps"),
        filter_platform: args.value_of("filter-platform").map(|s| s.to_string()),
        version,
    };

    let result = ops::output_metadata(&ws, &options)?;
    print_json(&result);
    Ok(())
}

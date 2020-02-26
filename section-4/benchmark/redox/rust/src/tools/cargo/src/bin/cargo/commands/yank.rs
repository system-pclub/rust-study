use crate::command_prelude::*;

use cargo::ops;

pub fn cli() -> App {
    subcommand("yank")
        .about("Remove a pushed crate from the index")
        .arg(opt("quiet", "No output printed to stdout").short("q"))
        .arg(Arg::with_name("crate"))
        .arg(opt("vers", "The version to yank or un-yank").value_name("VERSION"))
        .arg(opt(
            "undo",
            "Undo a yank, putting a version back into the index",
        ))
        .arg(opt("index", "Registry index to yank from").value_name("INDEX"))
        .arg(opt("token", "API token to use when authenticating").value_name("TOKEN"))
        .arg(opt("registry", "Registry to use").value_name("REGISTRY"))
        .after_help(
            "\
The yank command removes a previously pushed crate's version from the server's
index. This command does not delete any data, and the crate will still be
available for download via the registry's download link.

Note that existing crates locked to a yanked version will still be able to
download the yanked version to use it. Cargo will, however, not allow any new
crates to be locked to any yanked version.
",
        )
}

pub fn exec(config: &mut Config, args: &ArgMatches<'_>) -> CliResult {
    let registry = args.registry(config)?;

    ops::yank(
        config,
        args.value_of("crate").map(|s| s.to_string()),
        args.value_of("vers").map(|s| s.to_string()),
        args.value_of("token").map(|s| s.to_string()),
        args.value_of("index").map(|s| s.to_string()),
        args.is_present("undo"),
        registry,
    )?;
    Ok(())
}

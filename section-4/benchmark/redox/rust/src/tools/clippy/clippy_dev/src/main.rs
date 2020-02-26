extern crate clap;
extern crate clippy_dev;
extern crate regex;

use clap::{App, Arg, SubCommand};
use clippy_dev::*;

mod fmt;
mod stderr_length_check;

#[derive(PartialEq)]
enum UpdateMode {
    Check,
    Change,
}

fn main() {
    let matches = App::new("Clippy developer tooling")
        .subcommand(
            SubCommand::with_name("fmt")
                .about("Run rustfmt on all projects and tests")
                .arg(
                    Arg::with_name("check")
                        .long("check")
                        .help("Use the rustfmt --check option"),
                )
                .arg(
                    Arg::with_name("verbose")
                        .short("v")
                        .long("verbose")
                        .help("Echo commands run"),
                ),
        )
        .subcommand(
            SubCommand::with_name("update_lints")
                .about("Updates lint registration and information from the source code")
                .long_about(
                    "Makes sure that:\n \
                     * the lint count in README.md is correct\n \
                     * the changelog contains markdown link references at the bottom\n \
                     * all lint groups include the correct lints\n \
                     * lint modules in `clippy_lints/*` are visible in `src/lib.rs` via `pub mod`\n \
                     * all lints are registered in the lint store",
                )
                .arg(Arg::with_name("print-only").long("print-only").help(
                    "Print a table of lints to STDOUT. \
                     This does not include deprecated and internal lints. \
                     (Does not modify any files)",
                ))
                .arg(
                    Arg::with_name("check")
                        .long("check")
                        .help("Checks that util/dev update_lints has been run. Used on CI."),
                ),
        )
        .arg(
            Arg::with_name("limit-stderr-length")
                .long("limit-stderr-length")
                .help("Ensures that stderr files do not grow longer than a certain amount of lines."),
        )
        .get_matches();

    if matches.is_present("limit-stderr-length") {
        stderr_length_check::check();
    }

    match matches.subcommand() {
        ("fmt", Some(matches)) => {
            fmt::run(matches.is_present("check"), matches.is_present("verbose"));
        },
        ("update_lints", Some(matches)) => {
            if matches.is_present("print-only") {
                print_lints();
            } else if matches.is_present("check") {
                update_lints(&UpdateMode::Check);
            } else {
                update_lints(&UpdateMode::Change);
            }
        },
        _ => {},
    }
}

fn print_lints() {
    let lint_list = gather_all();
    let usable_lints: Vec<Lint> = Lint::usable_lints(lint_list).collect();
    let lint_count = usable_lints.len();
    let grouped_by_lint_group = Lint::by_lint_group(&usable_lints);

    for (lint_group, mut lints) in grouped_by_lint_group {
        if lint_group == "Deprecated" {
            continue;
        }
        println!("\n## {}", lint_group);

        lints.sort_by_key(|l| l.name.clone());

        for lint in lints {
            println!(
                "* [{}]({}#{}) ({})",
                lint.name,
                clippy_dev::DOCS_LINK.clone(),
                lint.name,
                lint.desc
            );
        }
    }

    println!("there are {} lints", lint_count);
}

#[allow(clippy::too_many_lines)]
fn update_lints(update_mode: &UpdateMode) {
    let lint_list: Vec<Lint> = gather_all().collect();

    let usable_lints: Vec<Lint> = Lint::usable_lints(lint_list.clone().into_iter()).collect();
    let lint_count = usable_lints.len();

    let mut sorted_usable_lints = usable_lints.clone();
    sorted_usable_lints.sort_by_key(|lint| lint.name.clone());

    let mut file_change = replace_region_in_file(
        "../src/lintlist/mod.rs",
        "begin lint list",
        "end lint list",
        false,
        update_mode == &UpdateMode::Change,
        || {
            format!(
                "pub const ALL_LINTS: [Lint; {}] = {:#?};",
                sorted_usable_lints.len(),
                sorted_usable_lints
            )
            .lines()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
        },
    )
    .changed;

    file_change |= replace_region_in_file(
        "../README.md",
        r#"\[There are \d+ lints included in this crate!\]\(https://rust-lang.github.io/rust-clippy/master/index.html\)"#,
        "",
        true,
        update_mode == &UpdateMode::Change,
        || {
            vec![
                format!("[There are {} lints included in this crate!](https://rust-lang.github.io/rust-clippy/master/index.html)", lint_count)
            ]
        }
    ).changed;

    file_change |= replace_region_in_file(
        "../CHANGELOG.md",
        "<!-- begin autogenerated links to lint list -->",
        "<!-- end autogenerated links to lint list -->",
        false,
        update_mode == &UpdateMode::Change,
        || gen_changelog_lint_list(lint_list.clone()),
    )
    .changed;

    file_change |= replace_region_in_file(
        "../clippy_lints/src/lib.rs",
        "begin deprecated lints",
        "end deprecated lints",
        false,
        update_mode == &UpdateMode::Change,
        || gen_deprecated(&lint_list),
    )
    .changed;

    file_change |= replace_region_in_file(
        "../clippy_lints/src/lib.rs",
        "begin register lints",
        "end register lints",
        false,
        update_mode == &UpdateMode::Change,
        || gen_register_lint_list(&lint_list),
    )
    .changed;

    file_change |= replace_region_in_file(
        "../clippy_lints/src/lib.rs",
        "begin lints modules",
        "end lints modules",
        false,
        update_mode == &UpdateMode::Change,
        || gen_modules_list(lint_list.clone()),
    )
    .changed;

    // Generate lists of lints in the clippy::all lint group
    file_change |= replace_region_in_file(
        "../clippy_lints/src/lib.rs",
        r#"store.register_group\(true, "clippy::all""#,
        r#"\]\);"#,
        false,
        update_mode == &UpdateMode::Change,
        || {
            // clippy::all should only include the following lint groups:
            let all_group_lints = usable_lints
                .clone()
                .into_iter()
                .filter(|l| {
                    l.group == "correctness" || l.group == "style" || l.group == "complexity" || l.group == "perf"
                })
                .collect();

            gen_lint_group_list(all_group_lints)
        },
    )
    .changed;

    // Generate the list of lints for all other lint groups
    for (lint_group, lints) in Lint::by_lint_group(&usable_lints) {
        file_change |= replace_region_in_file(
            "../clippy_lints/src/lib.rs",
            &format!("store.register_group\\(true, \"clippy::{}\"", lint_group),
            r#"\]\);"#,
            false,
            update_mode == &UpdateMode::Change,
            || gen_lint_group_list(lints.clone()),
        )
        .changed;
    }

    if update_mode == &UpdateMode::Check && file_change {
        println!(
            "Not all lints defined properly. \
             Please run `util/dev update_lints` to make sure all lints are defined properly."
        );
        std::process::exit(1);
    }
}

# Changelog Update

If you want to help with updating the [changelog][changelog], you're in the right place.

## When to update

Typos and other small fixes/additions are _always_ welcome.

Special care needs to be taken when it comes to updating the changelog for a new
Rust release. For that purpose, the changelog is ideally updated during the week
before an upcoming stable release. You can find the release dates on the [Rust
Forge][forge].

Most of the time we only need to update the changelog for minor Rust releases. It's
been very rare that Clippy changes were included in a patch release.

## How to update

### 1. Finding the relevant Clippy commits

Each Rust release ships with its own version of Clippy. The Clippy submodule can
be found in the [tools][tools] directory of the Rust repository.

To find the Clippy commit hash for a specific Rust release you select the Rust
release tag from the dropdown and then check the commit of the Clippy directory:

![Explanation of how to find the commit hash](https://user-images.githubusercontent.com/2042399/62846160-1f8b0480-bcce-11e9-9da8-7964ca034e7a.png)

When writing the release notes for the upcoming stable release you want to check
out the commit of the current Rust `beta` tag.

### 2. Fetching the PRs between those commits

You'll want to run `util/fetch_prs_between.sh commit1 commit2 > changes.txt`
and open that file in your editor of choice.

* `commit1` is the Clippy commit hash of the previous stable release
* `commit2` is the Clippy commit hash of the release you want to write the changelog for.

When updating the changelog it's also a good idea to make sure that `commit1` is
already correct in the current changelog.

### 3. Authoring the final changelog

The above script should have dumped all the relevant PRs to the file you
specified. It should have filtered out most of the irrelevant PRs
already, but it's a good idea to do a manual cleanup pass where you look for
more irrelevant PRs. If you're not sure about some PRs, just leave them in for
the review and ask for feedback.

With the PRs filtered, you can start to take each PR and move the
`changelog: ` content to `CHANGELOG.md`. Adapt the wording as you see fit but
try to keep it somewhat coherent.

The order should roughly be:

1. New lints
2. Changes that expand what code existing lints cover
3. ICE fixes
4. False positive fixes
5. Suggestion fixes/improvements

Please also be sure to update the Beta/Unreleased sections at the top with the
relevant commit ranges.

[changelog]: https://github.com/rust-lang/rust-clippy/blob/master/CHANGELOG.md
[forge]: https://forge.rust-lang.org/
[tools]: https://github.com/rust-lang/rust/tree/master/src/tools

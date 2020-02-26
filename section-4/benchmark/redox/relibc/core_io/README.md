# core_io

`std::io` with all the parts that don't work in core removed.

## Adding new nightly versions

First, make sure the commit you want to add is fetch in the git tree at
`/your/rust/dir/.git`. Then, import the right source files:

```
$ echo FULL_COMMIT_ID ...|GIT_DIR=/your/rust/dir/.git ./build-src.sh
```

Instead of echoing in the commit IDs, you might pipe in `rustc-commit-db
list-valid`.

The build-src script will prompt you to create patches for new commits. You
will be dropped in a shell prompt with a temporary new, clean, git repository
just for this patch. Make any changes necessary to make it build. **Don't**
commit any changes! When exiting the shell and the script will use the working
tree diff as the patch. The temporary git repository will be deleted. Before
dropping into the shell, the script will show you nearby commits, you can try
to apply `$PATCH_DIR/that_commit.patch` and see if it works for you.

## Publishing

```
$ echo FULL_COMMIT_ID ...|GIT_DIR=/your/rust/dir/.git ./build-src.sh publish
```

Again, instead of echoing in the commit IDs, you might pipe in `rustc-commit-db
list-valid`.

## Editing patches

To edit all patches, again make a checkout of the rust source. Then, run:

```
$ GIT_DIR=/your/rust/dir/.git ./edit-patches.sh
```

The script will prompt you to make changes. You will be dropped in a shell
prompt with a temporary new, clean, git repository just for this patch edit.
The original patch will be the HEAD commit in the repository. Make any changes
you want. **Don't** commit any changes! When exiting the shell and the script
will use the diff between the working tree and the root commit as the patch.
The temporary git repository will be deleted. When editing further commits, the
previous patch changes will already be applied to the working tree (if
succesful).

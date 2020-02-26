use std::path::PathBuf;

pub fn db_location() -> String {
    format!("{}/tests/test_db/", env!("CARGO_MANIFEST_DIR"))
}

pub fn get_db() -> pkgutils::Database {
    let path = db_location();
    pkgutils::Database::open(
        format!("{}/pkg", path),
        pkgutils::PackageDepends::Directory(
            PathBuf::from(format!("{}/etc/pkg.d/pkglist", path))
        )
    )
}

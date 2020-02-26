use ordermap::OrderMap;

mod common;

#[test]
fn test_is_installed() {
    let db = common::get_db();

    // Check if pkg4 is installed, and check if pkg3 is not installed
    assert!(db.is_pkg_installed("pkg4"));
    assert!(!db.is_pkg_installed("pkg3"));
}

#[test]
fn test_get_pkg_depends() {
    let db = common::get_db();

    let pkgs = db.get_pkg_depends("pkg2").unwrap();

    assert_eq!(pkgs, vec!["pkg3", "pkg4"]);
}

#[test]
fn test_calc_depends() {
    let db = common::get_db();

    let mut pkgs = OrderMap::new();

    db.calculate_depends("pkg1", &mut pkgs).unwrap();

    let pkgs_vec: Vec<String> = pkgs.keys().map(|x| x.to_string()).collect();

    assert_eq!(pkgs_vec, vec!["pkg3", "pkg2", "pkg1"]);
}

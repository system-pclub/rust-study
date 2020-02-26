use std::fs::File;
use std::path::{Path, PathBuf};
use std::io;
use std::io::Read;
use std::error;
use std::fmt;

use petgraph;
use petgraph::graphmap::DiGraphMap;

use bidir_map::BidirMap;

use ordermap::OrderMap;

use toml::de;

use crate::PackageMeta;
use crate::Repo;

/// Error type for the `Database`. It's a combination of an `std::io::Error`,
/// `toml::de::Error`, and a cyclic error that can occur during dependency
/// resolution.
#[derive(Debug)]
pub enum DatabaseError {
    Io(io::Error),
    Toml(de::Error),
    Cycle(String),
}

impl fmt::Display for DatabaseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            DatabaseError::Io(ref err) => write!(f, "IO error: {}", err),
            DatabaseError::Toml(ref err) => write!(f, "TOML parsing error: {}", err),
            DatabaseError::Cycle(ref err) => write!(f, "Cyclic dependency: {}", err),
        }
    }
}

impl error::Error for DatabaseError {
    fn description(&self) -> &str {
        match *self {
            DatabaseError::Io(ref err) => err.description(),
            DatabaseError::Toml(ref err) => err.description(),
            DatabaseError::Cycle(_) => "Cyclic dependency",
        }
    }

    fn cause(&self) -> Option<&dyn error::Error> {
        match *self {
            DatabaseError::Io(ref err) => Some(err),
            DatabaseError::Toml(ref err) => Some(err),
            DatabaseError::Cycle(_) => None,
        }
    }
}

impl From<io::Error> for DatabaseError {
    fn from(err: io::Error) -> DatabaseError {
        DatabaseError::Io(err)
    }
}

impl From<de::Error> for DatabaseError {
    fn from(err: de::Error) -> DatabaseError {
        DatabaseError::Toml(err)
    }
}

#[derive(Debug)]
pub enum PackageDepends {
    Directory(PathBuf),
    Repository(Repo),
}

impl PackageDepends {
    /// Retrieves the dependencies of a package that are listed in its manifest
    /// file.
    pub fn get_depends(&self, pkg_name: &str) -> Result<Vec<String>, DatabaseError> {
        match *self {
            PackageDepends::Directory(ref pathbuf) => {
                let path = pathbuf.as_path().join(format!("{}.toml", pkg_name));

                let mut input = String::new();
                File::open(path.as_path().to_str().unwrap()).and_then(|mut f| {
                    f.read_to_string(&mut input)
                })?;

                Ok(PackageMeta::from_toml(&input)?.depends)
            },
            PackageDepends::Repository(ref repo) => {
                Ok(repo.fetch_meta(pkg_name)?.depends)
            }
        }
    }
}

/// The `Database` contains a list of all packages that are available for
/// install, as well as a list of all the packages installed on the system.
/// It is used to calculate the dependencies of a package and for checking if
/// a package is installed.
#[derive(Debug)]
pub struct Database {
    /// The path to the directory that contains the manifests of the packages
    /// installed
    installed_path: PathBuf,

    /// The path to the directory that contains the manifests of the packages
    /// available for install
    pkgdepends: PackageDepends,
}

/// The `Database` contains a list of all packages that are available for
/// install, as well as a list of all the packages installed on the system.
/// It is used to calculate the dependencies of a package and for checking if
/// a package is installed.
impl Database {
    /// Opens a database from the specified path.
    pub fn open<P: AsRef<Path>>(installed_path: P, pkgdepends: PackageDepends) -> Self {
        Database {
            installed_path: installed_path.as_ref().to_path_buf(),
            pkgdepends: pkgdepends,
        }
    }

    /// Checks if a package is installed
    pub fn is_pkg_installed(&self, pkg_name: &str) -> bool {
        let pkg_path_buf = self.installed_path.as_path().join(format!("{}.toml", pkg_name));
        let installed = pkg_path_buf.as_path().exists();
        installed
    }

    /// Retrieves the dependencies of a package that are listed in its manifest
    /// file.
    pub fn get_pkg_depends(&self, pkg_name: &str) -> Result<Vec<String>, DatabaseError> {
        self.pkgdepends.get_depends(pkg_name)
    }

    /// Calculates the dependencies of the specified package, and appends them to
    /// `ordered_dependencies`.
    pub fn calculate_depends(&self, pkg_name: &str, ordered_dependencies: &mut OrderMap<String, ()>) -> Result<(), DatabaseError> {
        let mut graph = DiGraphMap::new();

        // Use bimap to intern strings and use integers for keys in graph because
        // String doesn't implement Copy and graphmap requires Copy
        let mut map = BidirMap::new();

        map.insert(pkg_name.to_string(), 0);

        self.calculate_depends_rec(pkg_name, &mut map, &mut graph)?;

        // Convert integers back to package names and calculate install order
        let dependency_ids = petgraph::algo::toposort(&graph, None).or_else(|err| {
            // There was a cyclic dependency. Since the graph is made up of numbers, the
            // name of the package that caused the cyclic dependency must be retrieved for
            // human readability.
            Err(DatabaseError::Cycle(map.get_by_second(&err.node_id()).unwrap().to_string()))
        })?;

        for i in dependency_ids {
            if !ordered_dependencies.contains_key(map.get_by_second(&i).unwrap()) {
                if let Some((name, _)) = map.remove_by_second(&i) {
                    ordered_dependencies.insert(name, ());
                }
            }
        }

        Ok(())
    }

    /// Helper function to calculate package dependencies.
    fn calculate_depends_rec(&self, pkg_name: &str, map: &mut BidirMap<String, usize>, graph: &mut DiGraphMap<usize, u8>) -> Result<(), DatabaseError> {
        let curr_node = *map.get_by_first(pkg_name).unwrap();

        let mut depends = self.get_pkg_depends(pkg_name)?;

        if depends.len() == 0 {
            return Ok(());
        }

        // Copy all dependencies from vector into map, using the map length as the key
        while !depends.is_empty() {
            let index = depends.len() - 1;
            let dependency = depends.remove(index);

            // Check if package is already installed
            if !self.is_pkg_installed(&dependency) {
                // Check if the package is already in the graph. If it is, its
                // dependencies don't need to be calculated.
                if !map.contains_first_key(&dependency) {
                    let dependency_node = map.len();
                    graph.add_node(dependency_node);
                    map.insert(dependency, dependency_node);

                    graph.add_edge(dependency_node, curr_node, 0);
                    let dependency_name = map.get_mut_by_second(&dependency_node).unwrap().clone();
                    self.calculate_depends_rec(&dependency_name, map, graph)?;
                } else {
                    // Dependencies don't need to be calculated; the package only needs to get
                    // linked to the current node
                    let dependency_node = *map.get_by_first(&dependency).unwrap();
                    graph.add_edge(dependency_node, curr_node, 0);
                }
            }
        }

        Ok(())
    }
}

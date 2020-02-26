use libflate::gzip::Decoder;
use std::fs::File;
use std::path::Path;
use std::path::PathBuf;
use std::io::{self, Error, ErrorKind, Read};
use tar::{Archive, EntryType};
use std::io::BufReader;

use crate::packagemeta::PackageMeta;

pub struct Package {
    archive: Archive<Decoder<BufReader<File>>>,
    path: PathBuf,
    meta: Option<PackageMeta>,
}

impl Package {
    pub fn from_path<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let file = File::open(&path)?;
        let decoder = Decoder::new(BufReader::new(file))?;

        let mut ar = Archive::new(decoder);
        ar.set_preserve_permissions(true);
        Ok(Package{archive: ar, path: path.as_ref().to_path_buf(), meta: None})
    }

    pub fn install<P: AsRef<Path>>(&mut self, dest: P)-> io::Result<()> {
        self.archive.unpack(dest)?;
        Ok(())
    }

    pub fn list(&mut self) -> io::Result<()> {
        for i in self.archive.entries()? {
            println!("{}", i?.path()?.display());
        }
        Ok(())
    }

    pub fn archive(&self) -> &Archive<Decoder<BufReader<File>>> {
        &self.archive
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn meta(&mut self) -> io::Result<&PackageMeta> {
        if self.meta.is_none() {
            let mut toml = None;
            for entry in self.archive.entries()? {
                let mut entry = entry?;
                if entry.header().entry_type() != EntryType::Directory && entry.path()?.starts_with("pkg") {
                    if toml.is_none() {
                        let mut text = String::new();
                        entry.read_to_string(&mut text)?;
                        toml = Some(text);
                    } else {
                        return Err(Error::new(ErrorKind::Other, "Multiple metadata files in package"));
                    }
                }
            }

            if let Some(toml) = toml {
                self.meta = Some(PackageMeta::from_toml(&toml).map_err(|e| Error::new(ErrorKind::Other, e))?);
            } else {
                return Err(Error::new(ErrorKind::NotFound, "Package metadata not found"));
            }
        }

        Ok(self.meta.as_ref().unwrap())
    }
}

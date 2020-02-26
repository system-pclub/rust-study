extern crate arg_parser;
extern crate redox_installer;
extern crate redoxfs;
extern crate serde;
extern crate toml;

use redox_installer::Config;
use redoxfs::{DiskFile, FileSystem};
use std::{fs, io, process, sync, thread, time};
use std::ffi::OsStr;
use std::io::{Read, Write};
use std::ops::DerefMut;
use std::path::Path;
use std::process::Command;

#[cfg(not(target_os = "redox"))]
fn disk_paths(_paths: &mut Vec<(String, u64)>) {}

#[cfg(target_os = "redox")]
fn disk_paths(paths: &mut Vec<(String, u64)>) {
    let mut schemes = Vec::new();
    match fs::read_dir(":") {
        Ok(entries) => for entry_res in entries {
            if let Ok(entry) = entry_res {
                let path = entry.path();
                if let Ok(path_str) = path.into_os_string().into_string() {
                    let scheme = path_str.trim_start_matches(':').trim_matches('/');
                    if scheme.starts_with("disk") {
                        schemes.push(format!("{}:", scheme));
                    }
                }
            }
        },
        Err(err) => {
            eprintln!("installer_tui: failed to list schemes: {}", err);
        }
    }

    for scheme in schemes {
        let is_dir = fs::metadata(&scheme)
            .map(|x| x.is_dir())
            .unwrap_or(false);
        if is_dir {
            match fs::read_dir(&scheme) {
                Ok(entries) => for entry_res in entries {
                    if let Ok(entry) = entry_res {
                        if let Ok(path) = entry.path().into_os_string().into_string() {
                            if let Ok(metadata) = entry.metadata() {
                                let size = metadata.len();
                                if size > 0 {
                                    paths.push((path, size));
                                }
                            }
                        }
                    }
                },
                Err(err) => {
                    eprintln!("installer_tui: failed to list '{}': {}", scheme, err);
                }
            }
        }
    }
}

const KB: u64 = 1024;
const MB: u64 = 1024 * KB;
const GB: u64 = 1024 * MB;
const TB: u64 = 1024 * GB;

fn format_size(size: u64) -> String {
    if size % TB == 0 {
        format!("{} TB", size / TB)
    } else if size % GB == 0 {
        format!("{} GB", size / GB)
    } else if size % MB == 0 {
        format!("{} MB", size / MB)
    } else if size % KB == 0 {
        format!("{} KB", size / KB)
    } else {
        format!("{} B", size)
    }
}

fn with_redoxfs<P, T, F>(disk_path: &P, bootloader: &[u8], callback: F)
    -> T where
        P: AsRef<Path>,
        T: Send + Sync + 'static,
        F: FnMut(&Path) -> T + Send + Sync + 'static
{
    let mount_path = "file/installer_tui";

    let res = {
        let disk = DiskFile::open(disk_path).unwrap();

        if cfg!(not(target_os = "redox")) {
            if ! Path::new(mount_path).exists() {
                fs::create_dir(mount_path).unwrap();
            }
        }

        let ctime = time::SystemTime::now().duration_since(time::UNIX_EPOCH).unwrap();
        let fs = FileSystem::create_reserved(disk, bootloader, ctime.as_secs(), ctime.subsec_nanos()).unwrap();

        let callback_mutex = sync::Arc::new(sync::Mutex::new(callback));
        let join_handle = redoxfs::mount(fs, mount_path, move |real_path| {
            let callback_mutex = callback_mutex.clone();
            let real_path = real_path.to_owned();
            thread::spawn(move || {
                let res = {
                    let mut callback_guard = callback_mutex.lock().unwrap();
                    let callback = callback_guard.deref_mut();
                    callback(&real_path)
                };

                if cfg!(target_os = "redox") {
                    fs::remove_file(format!(":{}", mount_path)).unwrap();
                } else {
                    let status_res = if cfg!(target_os = "linux") {
                        Command::new("fusermount")
                            .arg("-u")
                            .arg(mount_path)
                            .status()
                    } else {
                        Command::new("umount")
                            .arg(mount_path)
                            .status()
                    };

                    let status = status_res.unwrap();
                    if ! status.success() {
                        panic!("umount failed");
                    }
                }

                res
            })
        }).unwrap();

        join_handle.join().unwrap()
    };

    res
}

fn dir_files(dir: &str, files: &mut Vec<String>) -> io::Result<()> {
    for entry_res in fs::read_dir(&format!("file:/{}", dir))? {
        let entry = entry_res?;
        let path = entry.path();
        let path_str = path.into_os_string().into_string().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "Failed to convert Path to &str"
            )
        })?;
        let path_trimmed = path_str.trim_start_matches("file:/");
        let metadata = entry.metadata()?;
        if metadata.is_dir() {
            dir_files(path_trimmed, files);
        } else {
            files.push(path_trimmed.to_string());
        }
    }
    Ok(())
}

fn package_files(config: &mut Config, files: &mut Vec<String>) -> io::Result<()> {
    //TODO: Remove packages from config where all files are located (and have valid shasum?)
    config.packages.clear();

    for entry_res in fs::read_dir("file:/pkg")? {
        let entry = entry_res?;
        let path = entry.path();
        if path.extension() == Some(OsStr::new("sha256sums")) {
            let sha256sums = fs::read_to_string(&path)?;
            for line in sha256sums.lines() {
                //TODO: Support binary format (second space turns into an asterisk)
                let mut parts = line.splitn(2, "  ");
                let _sha256sum = parts.next().ok_or(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Missing checksum in sha256sums"
                ))?;
                let name = parts.next().ok_or(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Missing filename in sha256sums"
                ))?;
                files.push(name.to_string());
            }
        }
    }

    Ok(())
}

fn main() {
    let disk_path = {
        let mut paths = Vec::new();
        disk_paths(&mut paths);
        loop {
            for (i, (path, size)) in paths.iter().enumerate() {
                eprintln!("\x1B[1m{}\x1B[0m: {}: {}", i + 1, path, format_size(*size));
            }

            if paths.is_empty() {
                eprintln!("installer_tui: no drives found");
                process::exit(1);
            } else {
                eprint!("Select a drive from 1 to {}: ", paths.len());

                let mut line = String::new();
                match io::stdin().read_line(&mut line) {
                    Ok(0) => {
                        eprintln!("installer_tui: failed to read line: end of input");
                        process::exit(1);
                    },
                    Ok(_) => (),
                    Err(err) => {
                        eprintln!("installer_tui: failed to read line: {}", err);
                        process::exit(1);
                    }
                }

                match line.trim().parse::<usize>() {
                    Ok(i) => {
                        if i >= 1 && i <= paths.len() {
                            break paths[i - 1].0.clone();
                        } else {
                            eprintln!("{} not from 1 to {}", i, paths.len());
                        }
                    },
                    Err(err) => {
                        eprintln!("invalid input: {}", err);
                    }
                }
            }
        }
    };

    let bootloader = {
        let path = "file:/bootloader";
        match fs::read(path) {
            Ok(ok) => ok,
            Err(err) => {
                eprintln!("installer_tui: {}: failed to read: {}", path, err);
                process::exit(1);
            }
        }
    };

    let res = with_redoxfs(&disk_path, &bootloader, |mount_path| -> Result<(), failure::Error> {
        let mut config = {
            let path = "file:/filesystem.toml";
            match fs::read_to_string(path) {
                Ok(config_data) => {
                    match toml::from_str(&config_data) {
                        Ok(config) => {
                            config
                        },
                        Err(err) => {
                            eprintln!("installer_tui: {}: failed to decode: {}", path, err);
                            return Err(failure::Error::from_boxed_compat(
                                Box::new(err))
                            );
                        }
                    }
                },
                Err(err) => {
                    eprintln!("installer_tui: {}: failed to read: {}", path, err);
                    return Err(failure::Error::from_boxed_compat(
                        Box::new(err))
                    );
                }
            }
        };

        // Copy bootloader, filesystem.toml, and kernel
        let mut files = vec![
            "bootloader".to_string(),
            "filesystem.toml".to_string(),
            "kernel".to_string()
        ];

        // Copy files in /include, /lib, and /pkg
        //TODO: Convert this data into package data
        for dir in ["include", "lib", "pkg"].iter() {
            if let Err(err) = dir_files(dir, &mut files) {
                eprintln!("installer_tui: failed to read files from {}: {}", dir, err);
                return Err(failure::Error::from_boxed_compat(
                    Box::new(err))
                );
            }
        }

        // Copy files from locally installed packages
        if let Err(err) = package_files(&mut config, &mut files) {
            eprintln!("installer_tui: failed to read package files: {}", err);
            return Err(failure::Error::from_boxed_compat(
                Box::new(err))
            );
        }

        let mut buf = vec![0; 4 * 1024 * 1024];
        for (i, name) in files.iter().enumerate() {
            eprintln!("copy {} [{}/{}]", name, i, files.len());

            let src = format!("file:/{}", name);
            let dest = mount_path.join(name);
            if let Some(parent) = dest.parent() {
                match fs::create_dir_all(&parent) {
                    Ok(()) => (),
                    Err(err) => {
                        eprintln!("installer_tui: failed to create directory {}: {}", parent.display(), err);
                        return Err(failure::Error::from_boxed_compat(
                            Box::new(err))
                        );
                    }
                }
            }

            //TODO: match file type to support symlinks

            {
                let mut src_file = match fs::File::open(&src) {
                    Ok(ok) => ok,
                    Err(err) => {
                        eprintln!("installer_tui: failed to open file {}: {}", src, err);
                        return Err(failure::Error::from_boxed_compat(
                            Box::new(err))
                        );
                    }
                };

                let mut dest_file = match fs::File::create(&dest) {
                    Ok(ok) => ok,
                    Err(err) => {
                        eprintln!("installer_tui: failed to create file {}: {}", dest.display(), err);
                        return Err(failure::Error::from_boxed_compat(
                            Box::new(err))
                        );
                    }
                };

                loop {
                    let count = match src_file.read(&mut buf) {
                        Ok(ok) => ok,
                        Err(err) => {
                            eprintln!("installer_tui: failed to read file {}: {}", src, err);
                            return Err(failure::Error::from_boxed_compat(
                                Box::new(err))
                            );
                        }
                    };

                    if count == 0 {
                        break;
                    }

                    match dest_file.write_all(&buf[..count]) {
                        Ok(()) => (),
                        Err(err) => {
                            eprintln!("installer_tui: failed to write file {}: {}", dest.display(), err);
                            return Err(failure::Error::from_boxed_compat(
                                Box::new(err))
                            );
                        }
                    }
                }
            }
        }

        eprintln!("finished copying {} files", files.len());

        let cookbook: Option<&'static str> = None;
        redox_installer::install(config, mount_path, cookbook).map_err(|err| {
            io::Error::new(
                io::ErrorKind::Other,
                err
            )
        })?;

        eprintln!("finished installing, unmounting filesystem");

        Ok(())
    });

    match res {
        Ok(()) => {
            eprintln!("installer_tui: installed successfully");
            process::exit(0);
        },
        Err(err) => {
            eprintln!("installer_tui: failed to install: {:?}", err);
            process::exit(1);
        }
    }
}

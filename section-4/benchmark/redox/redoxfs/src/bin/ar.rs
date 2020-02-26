extern crate redoxfs;
extern crate syscall;
extern crate uuid;

use std::{env, fs, process};
use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};

use redoxfs::{DiskSparse, FileSystem, archive};
use uuid::Uuid;

fn main() {
    let mut args = env::args().skip(1);

    let disk_path = if let Some(path) = args.next() {
        path
    } else {
        println!("redoxfs-ar: no disk image provided");
        println!("redoxfs-ar DISK FOLDER [BOOTLOADER]");
        process::exit(1);
    };

    let folder_path = if let Some(path) = args.next() {
        path
    } else {
        println!("redoxfs-ar: no folder provided");
        println!("redoxfs-ar DISK FOLDER [BOOTLOADER]");
        process::exit(1);
    };

    let bootloader_path_opt = args.next();

    let disk = match DiskSparse::create(&disk_path) {
        Ok(disk) => disk,
        Err(err) => {
            println!("redoxfs-ar: failed to open image {}: {}", disk_path, err);
            process::exit(1);
        }
    };

    let mut bootloader = vec![];
    if let Some(bootloader_path) = bootloader_path_opt {
        match fs::File::open(&bootloader_path) {
            Ok(mut file) => match file.read_to_end(&mut bootloader) {
                Ok(_) => (),
                Err(err) => {
                    println!("redoxfs-ar: failed to read bootloader {}: {}", bootloader_path, err);
                    process::exit(1);
                }
            },
            Err(err) => {
                println!("redoxfs-ar: failed to open bootloader {}: {}", bootloader_path, err);
                process::exit(1);
            }
        }
    };

    let ctime = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    match FileSystem::create_reserved(disk, &bootloader, ctime.as_secs(), ctime.subsec_nanos()) {
        Ok(mut fs) => {
            let size = match archive(&mut fs, &folder_path) {
                Ok(ok) => ok,
                Err(err) => {
                    println!("redoxfs-ar: failed to archive {}: {}", folder_path, err);
                    process::exit(1);
                }
            };

            if let Err(err) = fs.disk.file.set_len(size) {
                println!("redoxfs-ar: failed to truncate {} to {}: {}", disk_path, size, err);
                process::exit(1);
            }

            let uuid = Uuid::from_bytes(&fs.header.1.uuid).unwrap();
            println!(
                "redoxfs-ar: created filesystem on {}, reserved {} blocks, size {} MB, uuid {}",
                disk_path,
                fs.block,
                fs.header.1.size/1000/1000,
                uuid.hyphenated()
            );
        },
        Err(err) => {
            println!("redoxfs-ar: failed to create filesystem on {}: {}", disk_path, err);
            process::exit(1);
        }
    };
}

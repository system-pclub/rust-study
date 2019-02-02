extern crate libc;

/**
 * How to reproduce this issue
 *      - Get redoxfs from https://gitlab.redox-os.org/redox-os/redoxfs
 *      - compile and mount redoxfs: make clean; make; make mount;
 *      - compile this program and copy this binary to the root of your mount point: /path/to/redoxfs/image
 *      - Run this program under your mount point
 *
 * The patch of redoxfs(redoxfs-issue12.patch) is under root directory of this project
 */
fn main() {
    unsafe {
        let mut name_buf : Vec<u8> = Vec::with_capacity(64);
        name_buf.push(255);
        let fd = libc::open(name_buf.as_ptr() as *const i8, libc::O_CREAT|libc::O_WRONLY);
    }
}

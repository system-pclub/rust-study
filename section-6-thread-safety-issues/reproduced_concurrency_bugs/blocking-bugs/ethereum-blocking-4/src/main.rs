use parking_lot::RwLock;
use std::sync::Arc;
use std::thread;
use std::time;

fn main() {
    let foo = Arc::new(RwLock::new(1));
    let foo2 = foo.clone();

    let t = thread::spawn(move || {
        thread::sleep(time::Duration::from_millis(500));
        let mut wl = foo2.write();
        *wl = 2;
        println!("Write lock {}", *wl);
    });
    {
        let rl1 = foo.read();
        println!("read lock1 {}", *rl1);
        thread::sleep(time::Duration::from_millis(1000));
        let rl2 = foo.read();
        println!("read lock2 {}", *rl2);
    }
    t.join().unwrap();
    return ();
}

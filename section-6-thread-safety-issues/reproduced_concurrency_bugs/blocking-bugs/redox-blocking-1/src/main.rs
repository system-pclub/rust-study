use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::collections::BTreeMap;
use std::sync::Arc;

pub struct DiskScheme {
    disks: i32,
    handles: Arc<Mutex<BTreeMap<usize, ()>>>,
    next_id: AtomicUsize,
}

impl DiskScheme {
    fn dup(&self, _id: usize) -> Result<usize, ()> {
        let mut handles = self.handles.lock().unwrap();
        let new_id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.handles.lock().unwrap().insert(new_id, ());
        Ok(new_id)
    }
}

fn main() {
    let d = DiskScheme {
        disks: 1,
        handles: Arc::new(Mutex::new(BTreeMap::default())),
        next_id: AtomicUsize::new(1)
    };
    let _result = d.dup(1);
    println!("Hello World!");
}


use std::sync::RwLock;
use std::thread;
use std::sync::Arc;
use std::time;

struct BlockChain {
    best_block: RwLock<()>,
    best_ancient_block: RwLock<()>,
}

impl BlockChain {
    fn new() -> Self {
        BlockChain {
            best_block: RwLock::new(()),
            best_ancient_block: RwLock::new(()),
        }
    }

    fn chain_info(&self) {
        let best_block = self.best_block.read();
        let best_ancient_block = self.best_ancient_block.read();
    }

    fn commit(&self) {
        let mut best_ancient_block = self.best_ancient_block.write();
        thread::sleep(time::Duration::from_millis(500));
        let mut best_block = self.best_block.write();
    }
}

fn main() {
    let bc = Arc::new(BlockChain::new());
    let cloned_bc = bc.clone();
    thread::spawn(move || {
        cloned_bc.chain_info();
    });
    bc.commit();
    println!("Hello, world!");
}

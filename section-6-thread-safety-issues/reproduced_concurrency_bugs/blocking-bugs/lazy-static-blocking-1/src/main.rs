use std::sync::Once;

static START: Once = Once::new();
fn main() {
    START.call_once(|| {
        START.call_once(|| {
            println!("deadlock!");
        });
    });
}
// This should fail even without validation
// compile-flags: -Zmiri-disable-validation

struct Human;

fn main() {
    let _x: ! = unsafe {
        std::mem::transmute::<Human, !>(Human) //~ ERROR entered unreachable code
    };
}

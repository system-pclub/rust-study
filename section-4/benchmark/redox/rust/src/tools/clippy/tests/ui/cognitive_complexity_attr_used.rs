#![warn(clippy::cognitive_complexity)]
#![warn(unused)]

fn main() {
    kaboom();
}

#[clippy::cognitive_complexity = "0"]
fn kaboom() {
    if 42 == 43 {
        panic!();
    } else if "cake" == "lie" {
        println!("what?");
    }
}

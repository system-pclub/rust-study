// run-rustfix

fn main () {
    (1, (2, 3)).1.1; //~ ERROR unexpected token: `1.1`
}

fn main() {}

trait T {
    fn qux() -> Option<usize> {
        let _ = if true {
        });
//~^ ERROR expected one of `async`
//~| ERROR expected one of `.`, `;`, `?`, `else`, or an operator, found `}`
//~| ERROR expected identifier, found `;`
        Some(4)
    }

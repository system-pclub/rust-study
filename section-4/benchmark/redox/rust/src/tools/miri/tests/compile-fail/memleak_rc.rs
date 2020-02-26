// ignore-windows: We do not check leaks on Windows
// ignore-macos: We do not check leaks on macOS

//error-pattern: the evaluated program leaked memory

use std::rc::Rc;
use std::cell::RefCell;

struct Dummy(Rc<RefCell<Option<Dummy>>>);

fn main() {
    let x = Dummy(Rc::new(RefCell::new(None)));
    let y = Dummy(x.0.clone());
    *x.0.borrow_mut() = Some(y);
}

use std::cell::{BorrowError, BorrowMutError, Ref, RefCell, RefMut};
use std::sync::Arc;
use std::thread;

#[derive(Clone, Debug)]
pub struct TESTRefCell {
    value: RefCell<i32>,
}

unsafe impl Sync for TESTRefCell {}

impl TESTRefCell {
    pub fn new(value: i32) -> TESTRefCell {
        TESTRefCell {
            value: RefCell::new(value),
        }
    }

    pub fn try_borrow(&self) -> Result<Ref<i32>, BorrowError> {
        self.value.try_borrow()
    }

    pub fn try_borrow_mut(&self) -> Result<RefMut<i32>, BorrowMutError> {
        self.value.try_borrow_mut()
    }

    pub fn borrow(&self) -> Ref<i32> {
        self.try_borrow().expect("error in borrow")
    }

    pub fn borrow_mut(&self) -> RefMut<i32> {
        self.try_borrow_mut().expect("error in borrow_mut")
    }
}

fn main() {
    let c = TESTRefCell::new(5);
    let c = Arc::new(c);

    let x = c.borrow();
    {
        let c2 = c.clone();
        thread::spawn(move || {
            {
                let mut v = c2.borrow_mut();
                *v = 100;
            }
            println!("{:?} in child", c2.value);
        })
        .join()
        .unwrap();
    }

    println!("{:?} in main", x);
}

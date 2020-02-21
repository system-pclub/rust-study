#servo-1772 

This is a non-blocking bug.

A mutable borrow held by thread A live while thread B tries to get an immutable borrow, leading to panic inside `RefCell`. 

Result:
```
'<unnamed>' panicked at 'error in borrow_mut: BorrowMutError', src/libcore/result.rs:1187:5
```

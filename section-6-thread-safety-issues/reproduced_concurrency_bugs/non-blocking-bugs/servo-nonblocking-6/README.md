#servo-12639

Rust provides a poisoning mechanism. While a thread holds a mutex and triggers a panic, other threads that are waiting for the mutex will receive an `Err` as return from the `mutex.lock()` function. Therefore, panic information can be propagated across threads. 

`Poisoned.into_inner()` will return the protected shared variable by the mutex. 

For the buggy version of servo, when poison happens, log message will not be sent out, leading to some log is missing.


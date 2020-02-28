# tikv-539

This is a blocking bug related to `Condvar`

A thread can wait for an `Event` object to be set or to be cleared by invoking `Event.inner.1.wait()`. However, if a thread is the last one holding `Event.inner` `Arc`, it will stop the waiting, since no other threads can wake it up. `Arc::strong_count(&Event.inner) == 1` is used to decide whether a thread who is going to call `Event.inner.1.wait()` is the last thread holding `Event.inner` `Arc`. 

When an `Event` is dropped, `Event.inner.1.notify_all()` is called to wake up threads waiting on `Event.inner.1.wait()`, and `drop(Event.inner)` is also called to decrease the value of `Arc::strong_count(&Event.inner)`.

There are two problems for the buggy version. Firstly, library users can directly call `drop(Event.inner)`, without calling `Event.inner.1.notify_all()`. Second, a waiting thread could be waked up and execute `Arc::strong_count(&Event.inner) == 1` before `drop(Event.inner)` is called.


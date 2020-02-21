#parity-ethereum-8977

[parking_lot::RwLock](https://docs.rs/parking_lot/0.7.1/parking_lot/type.RwLock.html)

This lock uses a task-fair locking policy which avoids both reader and writer starvation. This means that readers trying to acquire the lock will block if there are writers waiting to acquire the lock. Therefore, attempts to recursively acquire a read lock from the same thread may result in a deadlock.


# slab_allocator

[![Build Status](https://travis-ci.org/weclaw1/slab_allocator.svg?branch=master)](https://travis-ci.org/weclaw1/slab_allocator)

[Documentation](https://docs.rs/crate/slab_allocator)

## Usage

Create a static allocator in your root module:

```rust
use slab_allocator::LockedHeap;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();
```

Before using this allocator, you need to init it:

```rust
pub fn init_heap() {
    let heap_start = …;
    let heap_end = …;
    let heap_size = heap_end - heap_start;
    unsafe {
        ALLOCATOR.init(heap_start, heap_size);
    }
}
```

## License
This crate is licensed under MIT. See LICENSE for details.

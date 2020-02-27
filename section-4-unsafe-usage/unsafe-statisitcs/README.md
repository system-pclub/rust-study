# Count unsafe regions, functions, and traits

## Studied Benchmark App
1. https://github.com/servo/servo
commit: 3a33f99cad1e80e1f1a3e12dd98cabd9c40aa246
2. https://github.com/tikv/tikv
commit: 2b0296b7c779afc89c8a0dc64e31a6b41d0d035c
3. https://github.com/paritytech/parity-ethereum
commit: 04c686766060d1954ba1069d7634e7458053ec43
4. https://www.redox-os.org/
commit: d68d5890a0079812e49aeacbe76acb49ee5102e1
5. https://github.com/tock/tock
commit: 10723bd0efac798458ee205b04fe8786e8287cf8
6. https://github.com/rust-random/rand
commit: 1eef88c78cdef80c6b1675c3553110a5f2a013d8
7. https://github.com/crossbeam-rs/crossbeam
commit: 0bd562ce1710c58594d57ffa9723298a6df975e9
8. https://github.com/rust-threadpool/rust-threadpool
commit: 07d1a5b1b7aaecad8983cd80623f52cbf310ce23
9. https://github.com/rayon-rs/rayon
commit: 003b5e64735a6606c0aa7735cc22f0a259056c2f
10. https://github.com/rust-lang-nursery/lazy-static.rs
commit: 88994b26744fac2ceb713a9285985f76195c9c89
11. https://github.com/rust-lang/rust
commit: 2975a3c4befa8ad610da2e3c5f5de351d6d70a2b

## Usage:

```./count_unsafe.py APP_REPO_DIR``` 

```./count_LOC.py APP_REPO_DIR```

## Example:

```./count_unsafe.py ../rand```

```./count_LOC.py ../rand```

## Output:

Numbers of unsafe regions, LOC of unsafe regions, unsafe functions, LOC of unsafe functions, and unsafe traits.

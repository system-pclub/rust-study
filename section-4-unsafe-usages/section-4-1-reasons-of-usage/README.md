# Benchmark the performance for unsafe and safe code

## Usage:

1. Install
```
rustup default 1.36.0
```

2. Benchmark memory copy
```
cd mem-copy
cargo bench
```

3. Benchmark memory access
```
cd array-access
cargo bench
```

4. Benchmark ```ptr::offset``` and array access
```
cd array-offset
cargo bench
```

## Output:

Example:
```
test tests::bench_safe   ... bench:       2,007 ns/iter (+/- 42)
test tests::bench_unsafe ... bench:       1,629 ns/iter (+/- 14)
```

Compare numbers of ns/iter between safe and unafe code.

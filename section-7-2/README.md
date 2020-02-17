# Detect Double Lock

## Install:

### 1. Install Rust

[https://www.rust-lang.org/tools/install](https://www.rust-lang.org/tools/install)

### 2. Install llvm 9.0.0

[http://releases.llvm.org/download.html#9.0.0](http://releases.llvm.org/download.html#9.0.0)

Select pre-built binaries according to your OS version. After extracting to your target directory, say, $HOME/Env/llvm, you need to add the following code to your environment file (For my OS, it is $HOME/.bashrc).

```
LLVM_INSTALL_DIR=$HOME/Env/llvm
export PATH=${LLVM_INSTALL_DIR}/bin:$PATH
export LLVM_DIR=${LLVM_INSTALL_DIR}/lib/cmake
export CMAKE_PREFIX_PATH=${LLVM_INSTALL_DIR}/lib/cmake
export LD_LIBRARY_PATH=${LLVM_INSTALL_DIR}/lib:$LD_LIBRARY_PATH
```

## Usage:

### 1. build libPrintManualDrop.so

```
cd DoubleLockDetector
mkdir build
cd build
cmake ..
make
```
libRustDoubleLockDetector.so is in DoubleLockDetector/build/lib/RustDoubleLockDetector/lib

### 2. generate buggy LLVM BC
We will test our tool on an older version of parity-ethereum.

```
git clone git@github.com:parity-ethereum/parity-ethereum.git
cd parity-ethereum
git checkout 93fbbb9aaf161f21471050a2a3257f820c029a73
```

Now we are on a buggy branch of parity-ethereum, next we will generate bc for detection. Find all the Cargo.toml and append the following code to it. If the field [profile.dev] exists, change it to the following code.

```
[profile.dev]
opt-level = 0
debug = true
lto = false
debug-assertions = true
panic = 'unwind'
incremental = false
overflow-checks = true
```

Then, run the following command in each directory where Cargo.toml resides.

```
cargo rustc -- --emit=llvm-bc
```

You can choose ```cargo rustc --lib -- --emit=llvm-bc``` or ```cargo rustc --bin XXX -- --emit=llvm-bc``` if cargo complaints.

Now you can get the bc files in target/debug/deps. Do not use the bc files in incremental!

Then execute the following commands. Change file name and the path accordingly.

```
opt -mem2reg ethcore-XXX.bc > ethcore-XXX.m2r.bc
```

Store all the *.m2r.bc in LLVM_MEM_2_REG_BC_DIR.

### 3. run

```./run.sh LLVM_MEM_2_REG_BC_DIR```

## Output

```
Double Lock Happens! First Lock:
start: /home/boqin/Projects/Rust/double-lock/parity-ethereum ethcore/src/client/client.rs 1947
Second Lock(s):
bb110: /home/boqin/Projects/Rust/double-lock/parity-ethereum ethcore/src/client/client.rs 2039
...
```

The output is in `double_lock.log`, which records the BasicBlock name and source code location of the first and second lock,
followed by a call chain from the second lock to the first one.


## Reported Bugs

https://github.com/paritytech/parity-ethereum/pull/11172 (1 bug)
https://github.com/paritytech/parity-ethereum/pull/11175 (1 bug)
https://github.com/paritytech/parity-ethereum/issues/11176 (4 bugs)
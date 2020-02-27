# UAFDetector
A static use-after-free bug detector for Rust programs. It detects use-after-free bugs
by analyzing Rust's mid-level intermediate representation (MIR).

## Environment Requirements
### 1. Rust (1.38.0-nightly). 
If you are using the VM we provided for artifact evaluation, the
required Rust version should be already installed. If you are using those tools on your own
machine, Please follow the instruction here: https://www.rust-lang.org/tools/install to install
Rust and then update the Rust to 1.38.0-nightly by

```
rustup default nightly-2019-07-10-x86_64-unknown-linux-gnu
```
### 2. Python (>= 3.0).


## Running the detector
### 1. Compile your program and dump MIR.

```
cargo clean && cargo rustc -- -Zdump-mir="PreCodegen"
```


The MIR files will be dumped to directory `mir_dump` under the root directory of your project.

### 2. Run the detector on the dumped MIR.
```
python3 main.py /path/to/mir_files
```

## Output
```
Use-after-free detected: using dangling pointer:  _14  as source variable, it points to:  _18  in file:  sample_mir/sample.PreCodegen.after.mir
Use-after-free detected: using dangling pointer:  _14  as source variable, it points to:  _18  in file:  sample_mir/sample.PreCodegen.after.mir
Use-after-free detected: using dangling pointer:  _14  as source variable, it points to:  _18  in file:  sample_mir/sample.PreCodegen.after.mir
...
```

Currently you need to open the reported MIR file and map the reported pointer and variable to the lines in source code. For example:
```
let _14: *mut ffi::BIO;     // "data_bio_ptr" in scope 4 at openssl/src/cms.rs:149:17: 149:29
let _18: bio::MemBioSlice;  // in scope 0 at openssl/src/cms.rs:150:31: 150:54
```

You will see the same bug may be reported many times, these are only counted as one bug. This is caused
by that the detector tried different paths on control-flow graph and find the same bug.

## Reported Bugs

There are 4 previously unknown use-after-free bugs reported in section 7.1. 
(https://gitlab.redox-os.org/redox-os/relibc/issues/159) (4 bugs)
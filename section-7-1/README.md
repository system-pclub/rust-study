# UAFDetector
A static use-after-free bug detector for Rust programs. It detects use-after-free bugs
by analyzing Rust's mid-level intermediate representation (MIR).

## Environment Requirements
- rustc (1.38.0-nightly). Install the toolchain and set it to default by

  `rustup default nightly-2019-07-10-x86_64-unknown-linux-gnu`
- Python (>= 3.0).

## Running the detector
### 1. Compile your program and dump MIR.
`cargo clean && cargo rustc -- -Zdump-mir="PreCodegen"`

The MIR files will be dumped to directory `mir_dump` under the root directory of your project.

### 2. Run the detector on the dumped MIR.
`python3 main.py /path/to/mir_files`

#### Example: Detect the use-after-free bugs in Redox

We detected 4 previously unknown use-after-free bugs in Redox (mentioned in Sec. 7.1. Lines 1228-1229. 
[GitLab Issue](https://gitlab.redox-os.org/redox-os/relibc/issues/159)). We also put the corresponding MIR files under
`mir_detected_bugs` directory. Run the detector by

`python3 main.py mir_detected_bugs`
# Description

This directory contents source code and the Rust's mid-level intermediate representation
(MIR) of the application (Redox-relibc. [GitLab](https://gitlab.redox-os.org/redox-os/relibc)),
which has four previously unknown use-after-free bugs detected by our use-after-free detector. (Sec. 7.1. Lines 1228-1229).

## relibc

This directory contents the source code of the relibc version we used for PLDI'20 submission.


## relibc_mir_detected_bugs

You can directly run our use-after-free detector on those MIR files. The detailed
instructions is in the README file under the detector's root directory.
This directory only has the MIR files of the four detected bugs mentioned Sec. 7.1.
You can dump all MIR files by

```
cd relibc
cargo clean && cargo rustc -- -Zdump-mir="PreCodegen"
```
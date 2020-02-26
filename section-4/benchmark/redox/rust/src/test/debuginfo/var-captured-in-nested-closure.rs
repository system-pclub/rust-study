// min-lldb-version: 310

// compile-flags:-g

// === GDB TESTS ===================================================================================

// gdb-command:run

// gdb-command:print variable
// gdb-check:$1 = 1
// gdb-command:print constant
// gdb-check:$2 = 2
// gdb-command:print a_struct
// gdbg-check:$3 = {a = -3, b = 4.5, c = 5}
// gdbr-check:$3 = var_captured_in_nested_closure::Struct {a: -3, b: 4.5, c: 5}
// gdb-command:print *struct_ref
// gdbg-check:$4 = {a = -3, b = 4.5, c = 5}
// gdbr-check:$4 = var_captured_in_nested_closure::Struct {a: -3, b: 4.5, c: 5}
// gdb-command:print *owned
// gdb-check:$5 = 6
// gdb-command:print closure_local
// gdb-check:$6 = 8
// gdb-command:continue

// gdb-command:print variable
// gdb-check:$7 = 1
// gdb-command:print constant
// gdb-check:$8 = 2
// gdb-command:print a_struct
// gdbg-check:$9 = {a = -3, b = 4.5, c = 5}
// gdbr-check:$9 = var_captured_in_nested_closure::Struct {a: -3, b: 4.5, c: 5}
// gdb-command:print *struct_ref
// gdbg-check:$10 = {a = -3, b = 4.5, c = 5}
// gdbr-check:$10 = var_captured_in_nested_closure::Struct {a: -3, b: 4.5, c: 5}
// gdb-command:print *owned
// gdb-check:$11 = 6
// gdb-command:print closure_local
// gdb-check:$12 = 8
// gdb-command:continue


// === LLDB TESTS ==================================================================================

// lldb-command:run

// lldb-command:print variable
// lldbg-check:[...]$0 = 1
// lldbr-check:(isize) variable = 1
// lldb-command:print constant
// lldbg-check:[...]$1 = 2
// lldbr-check:(isize) constant = 2
// lldb-command:print a_struct
// lldbg-check:[...]$2 = Struct { a: -3, b: 4.5, c: 5 }
// lldbr-check:(var_captured_in_nested_closure::Struct) a_struct = Struct { a: -3, b: 4.5, c: 5 }
// lldb-command:print *struct_ref
// lldbg-check:[...]$3 = Struct { a: -3, b: 4.5, c: 5 }
// lldbr-check:(var_captured_in_nested_closure::Struct) *struct_ref = Struct { a: -3, b: 4.5, c: 5 }
// lldb-command:print *owned
// lldbg-check:[...]$4 = 6
// lldbr-check:(isize) *owned = 6
// lldb-command:print closure_local
// lldbg-check:[...]$5 = 8
// lldbr-check:(isize) closure_local = 8
// lldb-command:continue

// lldb-command:print variable
// lldbg-check:[...]$6 = 1
// lldbr-check:(isize) variable = 1
// lldb-command:print constant
// lldbg-check:[...]$7 = 2
// lldbr-check:(isize) constant = 2
// lldb-command:print a_struct
// lldbg-check:[...]$8 = Struct { a: -3, b: 4.5, c: 5 }
// lldbr-check:(var_captured_in_nested_closure::Struct) a_struct = Struct { a: -3, b: 4.5, c: 5 }
// lldb-command:print *struct_ref
// lldbg-check:[...]$9 = Struct { a: -3, b: 4.5, c: 5 }
// lldbr-check:(var_captured_in_nested_closure::Struct) *struct_ref = Struct { a: -3, b: 4.5, c: 5 }
// lldb-command:print *owned
// lldbg-check:[...]$10 = 6
// lldbr-check:(isize) *owned = 6
// lldb-command:print closure_local
// lldbg-check:[...]$11 = 8
// lldbr-check:(isize) closure_local = 8
// lldb-command:continue

#![allow(unused_variables)]
#![feature(box_syntax)]
#![feature(omit_gdb_pretty_printer_section)]
#![omit_gdb_pretty_printer_section]

struct Struct {
    a: isize,
    b: f64,
    c: usize
}

fn main() {
    let mut variable = 1;
    let constant = 2;

    let a_struct = Struct {
        a: -3,
        b: 4.5,
        c: 5
    };

    let struct_ref = &a_struct;
    let owned: Box<_> = box 6;

    let mut closure = || {
        let closure_local = 8;

        let mut nested_closure = || {
            zzz(); // #break
            variable = constant + a_struct.a + struct_ref.a + *owned + closure_local;
        };

        zzz(); // #break

        nested_closure();
    };

    closure();
}

fn zzz() {()}

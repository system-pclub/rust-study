// min-lldb-version: 310

// compile-flags:-g

// === GDB TESTS ===================================================================================

// gdb-command:run

// gdb-command:print self
// gdb-check:$1 = 1111
// gdb-command:continue

// gdb-command:print self
// gdbg-check:$2 = {x = 2222, y = 3333}
// gdbr-check:$2 = by_value_self_argument_in_trait_impl::Struct {x: 2222, y: 3333}
// gdb-command:continue

// gdb-command:print self
// gdbg-check:$3 = {__0 = 4444.5, __1 = 5555, __2 = 6666, __3 = 7777.5}
// gdbr-check:$3 = (4444.5, 5555, 6666, 7777.5)
// gdb-command:continue


// === LLDB TESTS ==================================================================================

// lldb-command:run

// lldb-command:print self
// lldbg-check:[...]$0 = 1111
// lldbr-check:(isize) self = 1111
// lldb-command:continue

// lldb-command:print self
// lldbg-check:[...]$1 = Struct { x: 2222, y: 3333 }
// lldbr-check:(by_value_self_argument_in_trait_impl::Struct) self = Struct { x: 2222, y: 3333 }
// lldb-command:continue

// lldb-command:print self
// lldbg-check:[...]$2 = (4444.5, 5555, 6666, 7777.5)
// lldbr-check:((f64, isize, isize, f64)) self = { = 4444.5 = 5555 = 6666 = 7777.5 }
// lldb-command:continue

#![feature(omit_gdb_pretty_printer_section)]
#![omit_gdb_pretty_printer_section]

trait Trait {
    fn method(self) -> Self;
}

impl Trait for isize {
    fn method(self) -> isize {
        zzz(); // #break
        self
    }
}

struct Struct {
    x: usize,
    y: usize,
}

impl Trait for Struct {
    fn method(self) -> Struct {
        zzz(); // #break
        self
    }
}

impl Trait for (f64, isize, isize, f64) {
    fn method(self) -> (f64, isize, isize, f64) {
        zzz(); // #break
        self
    }
}

fn main() {
    let _ = (1111 as isize).method();
    let _ = Struct { x: 2222, y: 3333 }.method();
    let _ = (4444.5, 5555, 6666, 7777.5).method();
}

fn zzz() { () }

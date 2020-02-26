// ignore-tidy-linelength

// min-lldb-version: 310
// ignore-gdb // Test temporarily ignored due to debuginfo tests being disabled, see PR 47155

// compile-flags:-g

// === GDB TESTS ===================================================================================

// gdbg-command:print 'simple_struct::NO_PADDING_16'
// gdbr-command:print simple_struct::NO_PADDING_16
// gdbg-check:$1 = {x = 1000, y = -1001}
// gdbr-check:$1 = simple_struct::NoPadding16 {x: 1000, y: -1001}

// gdbg-command:print 'simple_struct::NO_PADDING_32'
// gdbr-command:print simple_struct::NO_PADDING_32
// gdbg-check:$2 = {x = 1, y = 2, z = 3}
// gdbr-check:$2 = simple_struct::NoPadding32 {x: 1, y: 2, z: 3}

// gdbg-command:print 'simple_struct::NO_PADDING_64'
// gdbr-command:print simple_struct::NO_PADDING_64
// gdbg-check:$3 = {x = 4, y = 5, z = 6}
// gdbr-check:$3 = simple_struct::NoPadding64 {x: 4, y: 5, z: 6}

// gdbg-command:print 'simple_struct::NO_PADDING_163264'
// gdbr-command:print simple_struct::NO_PADDING_163264
// gdbg-check:$4 = {a = 7, b = 8, c = 9, d = 10}
// gdbr-check:$4 = simple_struct::NoPadding163264 {a: 7, b: 8, c: 9, d: 10}

// gdbg-command:print 'simple_struct::INTERNAL_PADDING'
// gdbr-command:print simple_struct::INTERNAL_PADDING
// gdbg-check:$5 = {x = 11, y = 12}
// gdbr-check:$5 = simple_struct::InternalPadding {x: 11, y: 12}

// gdbg-command:print 'simple_struct::PADDING_AT_END'
// gdbr-command:print simple_struct::PADDING_AT_END
// gdbg-check:$6 = {x = 13, y = 14}
// gdbr-check:$6 = simple_struct::PaddingAtEnd {x: 13, y: 14}

// gdb-command:run

// gdb-command:print no_padding16
// gdbg-check:$7 = {x = 10000, y = -10001}
// gdbr-check:$7 = simple_struct::NoPadding16 {x: 10000, y: -10001}

// gdb-command:print no_padding32
// gdbg-check:$8 = {x = -10002, y = -10003.5, z = 10004}
// gdbr-check:$8 = simple_struct::NoPadding32 {x: -10002, y: -10003.5, z: 10004}

// gdb-command:print no_padding64
// gdbg-check:$9 = {x = -10005.5, y = 10006, z = 10007}
// gdbr-check:$9 = simple_struct::NoPadding64 {x: -10005.5, y: 10006, z: 10007}

// gdb-command:print no_padding163264
// gdbg-check:$10 = {a = -10008, b = 10009, c = 10010, d = 10011}
// gdbr-check:$10 = simple_struct::NoPadding163264 {a: -10008, b: 10009, c: 10010, d: 10011}

// gdb-command:print internal_padding
// gdbg-check:$11 = {x = 10012, y = -10013}
// gdbr-check:$11 = simple_struct::InternalPadding {x: 10012, y: -10013}

// gdb-command:print padding_at_end
// gdbg-check:$12 = {x = -10014, y = 10015}
// gdbr-check:$12 = simple_struct::PaddingAtEnd {x: -10014, y: 10015}

// gdbg-command:print 'simple_struct::NO_PADDING_16'
// gdbr-command:print simple_struct::NO_PADDING_16
// gdbg-check:$13 = {x = 100, y = -101}
// gdbr-check:$13 = simple_struct::NoPadding16 {x: 100, y: -101}

// gdbg-command:print 'simple_struct::NO_PADDING_32'
// gdbr-command:print simple_struct::NO_PADDING_32
// gdbg-check:$14 = {x = -15, y = -16, z = 17}
// gdbr-check:$14 = simple_struct::NoPadding32 {x: -15, y: -16, z: 17}

// gdbg-command:print 'simple_struct::NO_PADDING_64'
// gdbr-command:print simple_struct::NO_PADDING_64
// gdbg-check:$15 = {x = -18, y = 19, z = 20}
// gdbr-check:$15 = simple_struct::NoPadding64 {x: -18, y: 19, z: 20}

// gdbg-command:print 'simple_struct::NO_PADDING_163264'
// gdbr-command:print simple_struct::NO_PADDING_163264
// gdbg-check:$16 = {a = -21, b = 22, c = 23, d = 24}
// gdbr-check:$16 = simple_struct::NoPadding163264 {a: -21, b: 22, c: 23, d: 24}

// gdbg-command:print 'simple_struct::INTERNAL_PADDING'
// gdbr-command:print simple_struct::INTERNAL_PADDING
// gdbg-check:$17 = {x = 25, y = -26}
// gdbr-check:$17 = simple_struct::InternalPadding {x: 25, y: -26}

// gdbg-command:print 'simple_struct::PADDING_AT_END'
// gdbr-command:print simple_struct::PADDING_AT_END
// gdbg-check:$18 = {x = -27, y = 28}
// gdbr-check:$18 = simple_struct::PaddingAtEnd {x: -27, y: 28}

// gdb-command:continue

// === LLDB TESTS ==================================================================================

// lldb-command:run

// lldb-command:print no_padding16
// lldbg-check:[...]$0 = NoPadding16 { x: 10000, y: -10001 }
// lldbr-check:(simple_struct::NoPadding16) no_padding16 = NoPadding16 { x: 10000, y: -10001 }

// lldb-command:print no_padding32
// lldbg-check:[...]$1 = NoPadding32 { x: -10002, y: -10003.5, z: 10004 }
// lldbr-check:(simple_struct::NoPadding32) no_padding32 = NoPadding32 { x: -10002, y: -10003.5, z: 10004 }

// lldb-command:print no_padding64
// lldbg-check:[...]$2 = NoPadding64 { x: -10005.5, y: 10006, z: 10007 }
// lldbr-check:(simple_struct::NoPadding64) no_padding64 = NoPadding64 { x: -10005.5, y: 10006, z: 10007 }

// lldb-command:print no_padding163264
// lldbg-check:[...]$3 = NoPadding163264 { a: -10008, b: 10009, c: 10010, d: 10011 }
// lldbr-check:(simple_struct::NoPadding163264) no_padding163264 = NoPadding163264 { a: -10008, b: 10009, c: 10010, d: 10011 }

// lldb-command:print internal_padding
// lldbg-check:[...]$4 = InternalPadding { x: 10012, y: -10013 }
// lldbr-check:(simple_struct::InternalPadding) internal_padding = InternalPadding { x: 10012, y: -10013 }

// lldb-command:print padding_at_end
// lldbg-check:[...]$5 = PaddingAtEnd { x: -10014, y: 10015 }
// lldbr-check:(simple_struct::PaddingAtEnd) padding_at_end = PaddingAtEnd { x: -10014, y: 10015 }

#![allow(unused_variables)]
#![allow(dead_code)]
#![feature(omit_gdb_pretty_printer_section)]
#![omit_gdb_pretty_printer_section]

struct NoPadding16 {
    x: u16,
    y: i16
}

struct NoPadding32 {
    x: i32,
    y: f32,
    z: u32
}

struct NoPadding64 {
    x: f64,
    y: i64,
    z: u64
}

struct NoPadding163264 {
    a: i16,
    b: u16,
    c: i32,
    d: u64
}

struct InternalPadding {
    x: u16,
    y: i64
}

struct PaddingAtEnd {
    x: i64,
    y: u16
}

static mut NO_PADDING_16: NoPadding16 = NoPadding16 {
    x: 1000,
    y: -1001
};

static mut NO_PADDING_32: NoPadding32 = NoPadding32 {
    x: 1,
    y: 2.0,
    z: 3
};

static mut NO_PADDING_64: NoPadding64 = NoPadding64 {
    x: 4.0,
    y: 5,
    z: 6
};

static mut NO_PADDING_163264: NoPadding163264 = NoPadding163264 {
    a: 7,
    b: 8,
    c: 9,
    d: 10
};

static mut INTERNAL_PADDING: InternalPadding = InternalPadding {
    x: 11,
    y: 12
};

static mut PADDING_AT_END: PaddingAtEnd = PaddingAtEnd {
    x: 13,
    y: 14
};

fn main() {
    let no_padding16 = NoPadding16 { x: 10000, y: -10001 };
    let no_padding32 = NoPadding32 { x: -10002, y: -10003.5, z: 10004 };
    let no_padding64 = NoPadding64 { x: -10005.5, y: 10006, z: 10007 };
    let no_padding163264 = NoPadding163264 { a: -10008, b: 10009, c: 10010, d: 10011 };

    let internal_padding = InternalPadding { x: 10012, y: -10013 };
    let padding_at_end = PaddingAtEnd { x: -10014, y: 10015 };

    unsafe {
        NO_PADDING_16.x = 100;
        NO_PADDING_16.y = -101;

        NO_PADDING_32.x = -15;
        NO_PADDING_32.y = -16.0;
        NO_PADDING_32.z = 17;

        NO_PADDING_64.x = -18.0;
        NO_PADDING_64.y = 19;
        NO_PADDING_64.z = 20;

        NO_PADDING_163264.a = -21;
        NO_PADDING_163264.b = 22;
        NO_PADDING_163264.c = 23;
        NO_PADDING_163264.d = 24;

        INTERNAL_PADDING.x = 25;
        INTERNAL_PADDING.y = -26;

        PADDING_AT_END.x = -27;
        PADDING_AT_END.y = 28;
    }

    zzz(); // #break
}

fn zzz() {()}

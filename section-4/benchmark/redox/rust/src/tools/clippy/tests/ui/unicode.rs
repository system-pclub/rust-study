#[warn(clippy::zero_width_space)]
fn zero() {
    print!("Here >​< is a ZWS, and ​another");
    print!("This\u{200B}is\u{200B}fine");
}

#[warn(clippy::unicode_not_nfc)]
fn canon() {
    print!("̀àh?");
    print!("a\u{0300}h?"); // also ok
}

#[warn(clippy::non_ascii_literal)]
fn uni() {
    print!("Üben!");
    print!("\u{DC}ben!"); // this is ok
}

fn main() {
    zero();
    uni();
    canon();
}

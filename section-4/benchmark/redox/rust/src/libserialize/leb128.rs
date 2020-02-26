#[inline]
pub fn write_to_vec(vec: &mut Vec<u8>, byte: u8) {
    vec.push(byte);
}

#[cfg(target_pointer_width = "32")]
const USIZE_LEB128_SIZE: usize = 5;
#[cfg(target_pointer_width = "64")]
const USIZE_LEB128_SIZE: usize = 10;

macro_rules! leb128_size {
    (u16) => (3);
    (u32) => (5);
    (u64) => (10);
    (u128) => (19);
    (usize) => (USIZE_LEB128_SIZE);
}

macro_rules! impl_write_unsigned_leb128 {
    ($fn_name:ident, $int_ty:ident) => (
        #[inline]
        pub fn $fn_name(out: &mut Vec<u8>, mut value: $int_ty) {
            for _ in 0 .. leb128_size!($int_ty) {
                let mut byte = (value & 0x7F) as u8;
                value >>= 7;
                if value != 0 {
                    byte |= 0x80;
                }

                write_to_vec(out, byte);

                if value == 0 {
                    break;
                }
            }
        }
    )
}

impl_write_unsigned_leb128!(write_u16_leb128, u16);
impl_write_unsigned_leb128!(write_u32_leb128, u32);
impl_write_unsigned_leb128!(write_u64_leb128, u64);
impl_write_unsigned_leb128!(write_u128_leb128, u128);
impl_write_unsigned_leb128!(write_usize_leb128, usize);


macro_rules! impl_read_unsigned_leb128 {
    ($fn_name:ident, $int_ty:ident) => (
        #[inline]
        pub fn $fn_name(slice: &[u8]) -> ($int_ty, usize) {
            let mut result: $int_ty = 0;
            let mut shift = 0;
            let mut position = 0;

            for _ in 0 .. leb128_size!($int_ty) {
                let byte = unsafe {
                    *slice.get_unchecked(position)
                };
                position += 1;
                result |= ((byte & 0x7F) as $int_ty) << shift;
                if (byte & 0x80) == 0 {
                    break;
                }
                shift += 7;
            }

            // Do a single bounds check at the end instead of for every byte.
            assert!(position <= slice.len());

            (result, position)
        }
    )
}

impl_read_unsigned_leb128!(read_u16_leb128, u16);
impl_read_unsigned_leb128!(read_u32_leb128, u32);
impl_read_unsigned_leb128!(read_u64_leb128, u64);
impl_read_unsigned_leb128!(read_u128_leb128, u128);
impl_read_unsigned_leb128!(read_usize_leb128, usize);



#[inline]
/// encodes an integer using signed leb128 encoding and stores
/// the result using a callback function.
///
/// The callback `write` is called once for each position
/// that is to be written to with the byte to be encoded
/// at that position.
pub fn write_signed_leb128_to<W>(mut value: i128, mut write: W)
    where W: FnMut(u8)
{
    loop {
        let mut byte = (value as u8) & 0x7f;
        value >>= 7;
        let more = !(((value == 0) && ((byte & 0x40) == 0)) ||
                     ((value == -1) && ((byte & 0x40) != 0)));

        if more {
            byte |= 0x80; // Mark this byte to show that more bytes will follow.
        }

        write(byte);

        if !more {
            break;
        }
    }
}

#[inline]
pub fn write_signed_leb128(out: &mut Vec<u8>, value: i128) {
    write_signed_leb128_to(value, |v| write_to_vec(out, v))
}

#[inline]
pub fn read_signed_leb128(data: &[u8], start_position: usize) -> (i128, usize) {
    let mut result = 0;
    let mut shift = 0;
    let mut position = start_position;
    let mut byte;

    loop {
        byte = data[position];
        position += 1;
        result |= i128::from(byte & 0x7F) << shift;
        shift += 7;

        if (byte & 0x80) == 0 {
            break;
        }
    }

    if (shift < 64) && ((byte & 0x40) != 0) {
        // sign extend
        result |= -(1 << shift);
    }

    (result, position - start_position)
}

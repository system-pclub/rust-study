use std::ptr;
use std::slice;
use std::vec::Vec;
use std::time::Instant;

fn create_1gb_vec() -> Vec<usize> {
    let value_size = std::mem::size_of::<usize>();
    let len: usize = 1024 * 1024 * (1024 / value_size);
    let mut vec = Vec::<usize>::with_capacity(len);

    let mut index = 0;
    let ptr = vec.as_mut_ptr();

    unsafe {
        vec.set_len(len);

        while index < len {
            *(ptr.add(index)) = 1;
            index += 1;
        }
    }
    vec
}

/// safe iterate using slice.iter()
pub fn safe_iterate() {
    let vec = create_1gb_vec();
    let slice = vec.as_slice();
    let mut sum: usize = 0;
    let mut index = 0;
    let mut len = 1024 * 1024 * 128 + 1;

    len = len - 1;

    let before = Instant::now();
    for index in 0..len {
        sum += slice[index];
    }
    let duration = before.elapsed();
    let ops: f64 = len as f64 / duration.as_secs_f64();
    let latency = duration.as_nanos() as f64 / len as f64;

    // print the sum is required to avoid the compiler eliding memory access
    println!("safe iterate, sum: {}, time : {:?}, ops/s = {:.0}, avg latency = {:.2}ns",
             sum, duration, ops, latency);
}

/// unsafe iterate using ptr.offset()
pub fn unsafe_iterate() {
    let vec = create_1gb_vec();
    let ptr = vec.as_ptr();
    let mut sum: usize = 0;
    let mut len = 1024 * 1024 * 128 + 1;

    len = len - 1;

    let before = Instant::now();
    unsafe {
        for i in 0..len as isize {
            sum += *(ptr.offset(i));
        }
    }
    let duration = before.elapsed();
    let ops: f64 = len as f64 / duration.as_secs_f64();
    let latency = duration.as_nanos() as f64 / len as f64;
    println!("unsafe iterate, sum: {}, time {:?}, ops/s = {:.0}, avg latency = {:.2}ns",
             sum, duration, ops, latency);
}

/// safe index using slice[index]
pub fn safe_index(strip: usize) {
    let vec = create_1gb_vec();
    let slice = vec.as_slice();
    let mut index = 0;
    let mut sum: usize = 0;
    let mut len = 1024 * 1024 * 128 + 1;

    len = len - 1;

    let before = Instant::now();
    while index < len {
        sum += slice[index];
        index = index + strip;
    }
    let duration = before.elapsed();
    let ops: f64 = (len / strip) as f64 / duration.as_secs_f64();
    let latency = duration.as_nanos() as f64 / (len / strip) as f64;
    println!("safe index, sum: {}, time: {:?}, ops/s = {:.0}, avg latency = {:.2}ns",
             sum, duration, ops, latency);
}

/// unsafe index using slice.get_unchecked_mut()
pub fn unsafe_index(strip: usize) {
    let vec = create_1gb_vec();
    let slice = vec.as_slice();
    let mut index = 0;
    let mut sum: usize = 0;
    let mut len = 1024 * 1024 * 128 + 1;

    len = len - 1;

    let before = Instant::now();
    unsafe {
        while index < len {
            sum += slice.get_unchecked(index);
            index = index + strip;
        }
    }
    let duration = before.elapsed();
    let ops: f64 = (len / strip) as f64 / duration.as_secs_f64();
    let latency = duration.as_nanos() as f64 / (len / strip) as f64;
    println!("unsafe index, sum: {}, time: {:?}, ops/s = {:.0}, avg latency = {:.2}ns",
             sum, duration, ops, latency);
}

/// copy 1gb using slice.copy_from_slice
pub fn copy_1gb_slice() {
    let src_vec = create_1gb_vec();
    let mut dest_vec = create_1gb_vec();

    let before = Instant::now();
    dest_vec.as_mut_slice().copy_from_slice(src_vec.as_slice());
    let duration = before.elapsed();
    println!("safe copy 1GB, time: {:?}", duration);
}

/// copy 1gb using ptr::copy_nonoverlapping
pub fn copy_1gb_pointer() {
    let src_vec = create_1gb_vec();
    let mut dest_vec = create_1gb_vec();

    let before = Instant::now();
    unsafe {
        ptr::copy_nonoverlapping(src_vec.as_ptr(),
                                 dest_vec.as_mut_ptr(), src_vec.len());
    }
    let duration = before.elapsed();
    println!("unsafe copy 1GB, time: {:?}", duration);
}

/// copy from Boqin's bench array
pub fn bench_array() {
    const ARRAY_SIZE : usize = 100000;
    let mut array = [1; ARRAY_SIZE];
    let mut size = 99999;
    size += 1;
    let mut sum = 0;

    let before = Instant::now();
    for i in 0..size {
        let a = array[i];
        unsafe {
            sum += a;
        }
    }
    let duration = before.elapsed();
    println!("bench_array, sum: {}, time: {:?}", sum, duration);
    // using this, compiler will elide memory access, since sum is never used.
    // println!("bench_array, time: {:?}", duration);
}

/// copy from Boqin's bench_offset
pub fn bench_offset() {
    const ARRAY_SIZE : usize = 100000;
    let mut array = [1; ARRAY_SIZE];
    let mut size = 99999;
    size += 1;
    let mut sum = 0;
    let p = array.as_mut_ptr();

    let before = Instant::now();
    for i in 0..size {
        unsafe {
            let a = *p.offset(i);
            sum += a;
        }
    }
    let duration = before.elapsed();
    println!("bench_offset, sum: {}, time: {:?}", sum, duration);
    // println!("bench_offset, time: {:?}", duration);
}
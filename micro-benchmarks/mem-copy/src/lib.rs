#![feature(test)]

extern crate test;

#[cfg(test)]
mod tests {
    use super::*;
    use test::Bencher;

    #[bench]
    fn bench_unsafe(b: &mut Bencher) {
        let src = vec![1; 1000000];
        let mut dst = vec![2; 1000000];
        let src_len = src.len();
        dst.reserve(src_len);
        let src = src.as_slice();
        let mut dst = dst.as_mut_slice();
        let dst_ptr = dst.as_mut_ptr();
        let src_ptr = src.as_ptr();

        b.iter(|| {
            unsafe {
                std::ptr::copy_nonoverlapping(src_ptr, dst_ptr, src_len);
            }
        });
        println!("{}, {}", src[0], dst[0]);
    }

    #[bench]
    fn bench_safe(b: &mut Bencher) {
        let src = vec![1; 1000000];
        let mut dst = vec![2; 1000000];
        let src_len = src.len();
        dst.reserve(src_len);
        let src = src.as_slice();
        let mut dst = dst.as_mut_slice();
        let dst_ptr = dst.as_mut_ptr();
        let src_ptr = src.as_ptr();

        b.iter(|| {
            for i in 0..src_len {
                dst[i] = src[i];
            }          
        });
        println!("{}, {}", src[0], dst[0]);
    }

    #[bench]
    fn bench_safe2(b: &mut Bencher) {
        let src = vec![1; 1000000];
        let mut dst = vec![2; 1000000];
        let src_len = src.len();
        dst.reserve(src_len);
        let src = src.as_slice();
        let mut dst = dst.as_mut_slice();
        let dst_ptr = dst.as_mut_ptr();
        let src_ptr = src.as_ptr();

        b.iter(|| {
            dst.copy_from_slice(src);
        });
        println!("{}, {}", src[0], dst[0]);
    }
}


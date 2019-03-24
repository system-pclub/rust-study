#![feature(test)]

extern crate test;

#[cfg(test)]
mod tests {
    use super::*;
    use test::Bencher;

    #[bench]
    fn bench_unsafe(b: &mut Bencher) {
        let src = vec![1; 100000];
        let mut dst = vec![2; 100000];
        let src_len = src.len();
        dst.reserve(src_len);

        b.iter(|| {
                let dst_ptr = dst.as_mut_ptr();
                let src_ptr = src.as_ptr();
            unsafe {
                std::ptr::copy_nonoverlapping(src_ptr, dst_ptr, src_len);
            }
        });
    }

    #[bench]
    fn bench_safe(b: &mut Bencher) {
        let src = vec![1; 100000];
        let mut dst = vec![2; 100000];
        let src_len = src.len();
        dst.reserve(src_len);

        b.iter(|| {
            for i in 0..src_len {
                dst[i] = src[i];
            }          
        });
    }
}


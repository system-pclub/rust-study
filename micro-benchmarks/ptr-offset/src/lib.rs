#![feature(test)]
#![feature(ptr_offset_from)]

extern crate test;

#[cfg(test)]
mod tests {
    use super::*;
    use test::Bencher;
   
    #[bench] 
    fn bench_ptr(b: &mut Bencher) {
        const VEC_SIZE : usize = 100000;
        let mut vec = vec!(1; VEC_SIZE);
        let mut size = 99999;
        size += 1;
        let mut sum = 0;
        let start = vec.as_mut_ptr();

        b.iter(|| {
            for i in 0..size {
                unsafe {
                    let next = start.add(i);
                    sum += next.offset_from(start);
                }
            }
        });
    }

    #[bench] 
    fn bench_addr(b: &mut Bencher) {
        const VEC_SIZE : usize = 100000;
        let mut vec = vec!(1; VEC_SIZE);
        let mut size = 99999;
        size += 1;
        let mut sum = 0;
        let start = vec.as_mut_ptr();
        let start_addr = start as usize;

        b.iter(|| {
            for i in 0..size {
                unsafe {
                    let next_addr = start.add(i) as usize;
                    sum += next_addr - start_addr;
                }
            }
        });
    }
}

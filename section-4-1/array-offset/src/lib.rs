#![feature(test)]

extern crate test;

#[cfg(test)]
mod tests {
    use super::*;
    use test::Bencher;
   
    #[bench] 
    fn bench_array(b: &mut Bencher) {
        const ARRAY_SIZE : usize = 100000;
        let mut array = [1; ARRAY_SIZE];
        let mut size = 99999;
        size += 1;
        let mut sum = 0;
        b.iter(|| {
            for i in 0..size {
                let a = array[i];
                unsafe {
                    sum += a;
                }
            }
        });
    }

    #[bench] 
    fn bench_offset(b: &mut Bencher) {
        const ARRAY_SIZE : usize = 100000;
        let mut array = [1; ARRAY_SIZE];
        let mut size = 99999;
        size += 1;
        let mut sum = 0;
        let p = array.as_mut_ptr();
        b.iter(|| {
            for i in 0..size {
                unsafe {
                    let a = *p.offset(i);
                    sum += a;
                }
            }
        });
    }
}

#![feature(test)]

extern crate test;

#[cfg(test)]
mod tests {
    use super::*;
    use test::Bencher;
   
    #[bench] 
    fn bench_boundary_checked(b: &mut Bencher) {
        const ARRAY_SIZE : usize = 100000;
        let array = [1; ARRAY_SIZE];
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
    fn bench_boundary_unchecked(b: &mut Bencher) {
        const ARRAY_SIZE : usize = 100000;
        let array = [1; ARRAY_SIZE];
        let mut size = 99999;
        size += 1;
        let mut sum = 0;
        b.iter(|| {
            for i in 0..size {
                unsafe {
                    let a = array.get_unchecked(i);
                    sum += a;
                }
            }
        });
    }

    
    #[bench] 
    fn bench_boundary_static(b: &mut Bencher) {
        const ARRAY_SIZE : usize = 100000;
        let array = [1; ARRAY_SIZE];
        let mut size = 99999;
        size += 1;
        let mut sum = 0;
        b.iter(|| {
            for i in 0..size {
                let a = array[i % ARRAY_SIZE];
                unsafe {
                    sum += a;
                }
            }
        });
    }
}

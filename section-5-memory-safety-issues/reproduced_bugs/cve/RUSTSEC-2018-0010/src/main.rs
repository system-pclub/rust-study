use std::ptr;
use std::vec::Vec;

#[derive(Debug)]
struct MockMemBioSlice {
    v: Vec<u8>
}

impl MockMemBioSlice {
    fn new(data: &[u8]) -> MockMemBioSlice {
        let ret = MockMemBioSlice {
            v: data.to_vec()
        };
        ret
    }

    fn as_ptr(&self) -> *const u8 {
        self.v.as_ptr()
    }
}

fn sign_bug(data: Option<&[u8]>) -> Result<(), ()> {
    unsafe {
        let data_bio_ptr = match data {
            Some(data) => MockMemBioSlice::new(data).as_ptr(),
            None => ptr::null()
        };
        println!("data_bio_ptr: {}", *data_bio_ptr);
        Ok(())
    }
}

fn sign_patch(data: Option<&[u8]>) -> Result<(), ()> {
    unsafe {
        let data_bio = match data {
            Some(data) => {
                Some(MockMemBioSlice::new(data))
            },
            None => None

        };
        let data_bio_ptr = data_bio.as_ref().map_or(ptr::null(), |p| p.as_ptr());
        println!("data_bio_ptr: {}", *data_bio_ptr);
        Ok(())
    }
}

fn main() {
    let v: Vec<u8> = vec![1, 2, 3];
    let data = Some(v.as_slice());
    sign_bug(data);
    // sign_patch(data);
}


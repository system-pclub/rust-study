pub struct Slice<'a> {
    bytes: &'a [u8]
}

impl<'a> Slice<'a> {
    #[inline]
    pub fn new(bytes: &'a [u8]) -> Slice<'a> {
        Slice { bytes }
    }

    #[inline]
    pub fn get_slice(&self, r: core::ops::Range<usize>)
                     -> Option<Slice<'a>> {
        self.bytes.get(r).map(|bytes| Slice { bytes })
    }

    #[inline]
    pub fn len(&self) -> usize { self.bytes.len() }
}

pub struct MockInput<'a> {
    value: Slice<'a>
}

pub struct MockReader<'a> {
    input: Slice<'a>,
    i: usize
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MockEndOfInput;

impl<'a> MockReader<'a> {
    /// Mock method
    pub fn from(bytes: &'a [u8]) -> MockReader<'a> {
        MockReader { input: Slice::new(bytes), i: 0 }
    }

    /// Real buggy function
    pub fn skip_and_get_input_bug(&mut self, num_bytes: usize)
                              -> Result<MockInput<'a>, MockEndOfInput> {
        // Try marco is replaced ? now
        let new_i = self.i.checked_add(num_bytes).ok_or(MockEndOfInput)?;
        let ret = self.input.get_slice(self.i..new_i)
            .map(|subslice| MockInput { value: subslice })
            .ok_or(MockEndOfInput);
        self.i = new_i;
        println!("input.len(): {}, self.i: {}, new_i: {}", self.input.len(), self.i, new_i);
        ret
    }

    /// Skips the reader to the end of the input, returning the skipped input
    /// as an `Input`.
    pub fn skip_to_end_bug(&mut self) -> MockInput<'a> {
        /**
         * Yilun:
         * This bug is caused by combining usage of skip_and_get_input(num) with skip_to_end()
         * If num is greater then the length of the input of reader, self.input.len() - self.i
         * will cause an integer overflow
         *
         *      - debug build -- panic at next statement
         *      - release build -- panic at self.skip_and_get_input_bug(to_skip).unwrap()
         */
        let to_skip = self.input.len() - self.i;
        println!("to_skip: {}", to_skip);
        self.skip_and_get_input_bug(to_skip).unwrap()
    }


    pub fn skip_and_get_input_patch(&mut self, num_bytes: usize)
                                  -> Result<MockInput<'a>, MockEndOfInput> {
        let new_i = self.i.checked_add(num_bytes).ok_or(MockEndOfInput)?;
        /**
         * Now fix this issue by add a Marco try!(?) here, if the get_slice get ERROR
         * this function will return here, thus self.i will not be updated
         */
        let ret = self.input.get_slice(self.i..new_i)
            .map(|subslice| MockInput { value: subslice })
            .ok_or(MockEndOfInput)?;
        self.i = new_i;
        println!("input.len(): {}, self.i: {}, new_i: {}", self.input.len(), self.i, new_i);
        Ok(ret)
    }

    pub fn skip_to_end_patch(&mut self) -> MockInput<'a> {
        let to_skip = self.input.len() - self.i;
        println!("to_skip: {}", to_skip);
        self.skip_and_get_input_patch(to_skip).unwrap()
    }
}


fn reader(input: &mut MockReader) -> Result<(), MockEndOfInput> {
    input.skip_and_get_input_bug(6);
    input.skip_to_end_bug();
    // input.skip_and_get_input_patch(6);
    // input.skip_to_end_patch();
    Ok(())
}

fn main() {
    let buf = vec![1, 2, 3, 4, 5];
    let mut input = MockReader::from(&buf);
    reader(&mut input);
}

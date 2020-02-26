fn some_func(a: Option<u32>) -> Option<u32> {
    if a.is_none() {
        return None;
    }

    a
}

fn some_other_func(a: Option<u32>) -> Option<u32> {
    if a.is_none() {
        return None;
    } else {
        return Some(0);
    }
    unreachable!()
}

pub enum SeemsOption<T> {
    Some(T),
    None,
}

impl<T> SeemsOption<T> {
    pub fn is_none(&self) -> bool {
        match *self {
            SeemsOption::None => true,
            SeemsOption::Some(_) => false,
        }
    }
}

fn returns_something_similar_to_option(a: SeemsOption<u32>) -> SeemsOption<u32> {
    if a.is_none() {
        return SeemsOption::None;
    }

    a
}

pub struct CopyStruct {
    pub opt: Option<u32>,
}

impl CopyStruct {
    #[rustfmt::skip]
    pub fn func(&self) -> Option<u32> {
        if (self.opt).is_none() {
            return None;
        }

        if self.opt.is_none() {
            return None
        }

        let _ = if self.opt.is_none() {
            return None;
        } else {
            self.opt
        };

        self.opt
    }
}

#[derive(Clone)]
pub struct MoveStruct {
    pub opt: Option<Vec<u32>>,
}

impl MoveStruct {
    pub fn ref_func(&self) -> Option<Vec<u32>> {
        if self.opt.is_none() {
            return None;
        }

        self.opt.clone()
    }

    pub fn mov_func_reuse(self) -> Option<Vec<u32>> {
        if self.opt.is_none() {
            return None;
        }

        self.opt
    }

    pub fn mov_func_no_use(self) -> Option<Vec<u32>> {
        if self.opt.is_none() {
            return None;
        }
        Some(Vec::new())
    }
}

fn main() {
    some_func(Some(42));
    some_func(None);

    let copy_struct = CopyStruct { opt: Some(54) };
    copy_struct.func();

    let move_struct = MoveStruct {
        opt: Some(vec![42, 1337]),
    };
    move_struct.ref_func();
    move_struct.clone().mov_func_reuse();
    move_struct.mov_func_no_use();

    let so = SeemsOption::Some(45);
    returns_something_similar_to_option(so);
}

use super::*;

// an untyped CloneFn pointer. Null variant represents an undefined function
// which will cause explicit panic if execute() is invoked.
// UNSAFE if the type pointed to does not match the type used to instantiate the ptr.
#[derive(Debug, Copy, Clone)]
pub(crate) struct CloneFn(Option<fn(*mut u8, *mut u8)>);
impl CloneFn {
    fn new<T>() -> Self {
        let clos: fn(*mut u8, *mut u8) = |src, dest| unsafe {
            // maybe_clone does not have the same memory layout for values of T.
            // we avoid this problem by defining a CLOSURE with a known layout,
            // and invoking maybe_clone for our known type here
            let datum = T::maybe_clone(transmute(src));
            let dest: *mut T = transmute(dest);
            dest.write(datum);
        };
        CloneFn(Some(clos))
    }
    /// safe ONLY IF:
    ///  - src is &T to initialized memory
    ///  - dst is &mut T to uninitialized memory
    ///  - T matches the type provided when creating this CloneFn in `new_defined`
    #[inline]
    pub unsafe fn execute(self, src: *mut u8, dst: *mut u8) {
        if let Some(x) = self.0 {
            (x)(src, dst);
        } else {
            panic!("proto attempted to clone an unclonable type!");
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub(crate) struct PartialEqFn(Option<fn(*mut u8, *mut u8) -> bool>);
impl PartialEqFn {
    fn new<T>() -> Self {
        PartialEqFn(Some(unsafe {
            transmute(
                <T as MaybePartialEq>::maybe_partial_eq as fn(&T, &T) -> bool
            )
        }))
    }
    #[inline]
    pub unsafe fn execute(self, a: *mut u8, b: *mut u8) -> bool {
        if let Some(x) = self.0 {
            (x)(a, b)
        } else {
            panic!("proto attempted to partial_eq a type for which its not defined!");
        }
    }
}

// an untyped DropFn pointer. Null variant represents a trivial drop Fn (no behavior).
// new() automatically handles types with trivial drop functions
// UNSAFE if the type pointed to does not match the type used to instantiate the ptr.
#[derive(Debug, Copy, Clone)]
pub(crate) struct DropFn(Option<fn(*mut u8)>);
impl DropFn {
    fn new<T>() -> Self {
        if std::mem::needs_drop::<T>() {
            DropFn(Some(unsafe {
                transmute(std::ptr::drop_in_place::<T> as unsafe fn(*mut T))
            }))
        } else {
            DropFn(None)
        }
    }
    /// safe ONLY IF the given pointer is of the type this DropFn was created for.
    #[inline]
    pub unsafe fn execute(self, on: *mut u8) {
        if let Some(x) = self.0 {
            (x)(on);
        } else {
            // None variant represents a drop with no effect
        }
    }
}

/// A structure used for type erasure. Describes the type in as much detail
/// that a memory cell needs to handle all the operations on it
#[derive(Debug, Clone, Copy)]
pub struct TypeInfo {
    pub(crate) type_id: TypeId,
    pub(crate) drop_fn: DropFn,
    pub(crate) clone_fn: CloneFn,
    pub(crate) partial_eq_fn: PartialEqFn,
    pub(crate) is_copy: bool,
    pub(crate) layout: Layout,
}
impl TypeInfo {

    #[inline]
    /// This function doesn't need a pointer. It's derived from the layout field.
    /// MOVE and COPY are equivalent. The only difference is whether an accompanying
    /// drop is inserted (by the compiler).
    pub unsafe fn move_fn_execute(&self, src: *mut u8, dest: *mut u8) {
        std::ptr::copy(src, dest, self.layout.size());
    }
    pub fn get_tid(&self) -> TypeId {
        self.type_id
    }
    pub fn new<T: 'static>() -> Self {
        Self {
            type_id: TypeId::of::<T>(),
            drop_fn: DropFn::new::<T>(),
            clone_fn: CloneFn::new::<T>(),
            partial_eq_fn: PartialEqFn::new::<T>(),
            layout: Layout::new::<T>(),
            is_copy: <T as MaybeCopy>::IS_COPY,
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct Defined(String);
    impl Drop for Defined {
        fn drop(&mut self) {
            println!("dropped Defined({}) !", self.0);
        }
    }

    #[test]
    fn drops_ok() {
        let drop_fn = DropFn::new::<Defined>();

        let foo = Defined("Hello, there.".into());
        let x1: *const _ = &foo as *const _;
        let x2: *mut u8 = unsafe { transmute(x1) };
        println!("{:?}", (x1, x2));

        unsafe { drop_fn.execute(x2) };
        std::mem::forget(foo);
    }

    #[test]
    fn partial_eq_ok() {
        let partial_eq_fn = PartialEqFn::new::<Defined>();

        let foo = Defined("General Kenobi!".into());
        let x1: *const _ = &foo as *const _;
        let x2: *mut u8 = unsafe { transmute(x1) };
        println!("{:?}", (x1, x2));

        unsafe { println!("maybe_partial_eq of Defined with itself gives {}", partial_eq_fn.execute(x2, x2)) };
    }

    struct Undefined(f32, f32);

    #[test]
    #[should_panic]
    fn partial_eq_undefined_panic() {
        let partial_eq_fn = PartialEqFn::new::<Undefined>();
        let x = Undefined(5.3, 234.4);
        let x1: *const _ = &x as *const _;
        let x2: *mut u8 = unsafe { transmute(x1) };
        unsafe {
            // this should panic, as partial_eq_fn.0 == None
            partial_eq_fn.execute(x2, x2);
        }
    }
}
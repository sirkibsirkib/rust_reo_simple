use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::mem::{self, ManuallyDrop};

// both port-halves SHARE this on the heap
struct Inner<T> {
    t: ManuallyDrop<T>,
    occupied: AtomicBool,
    refs: AtomicUsize,
}

// each port-half has one of these on the stack
/* USE CAFEULLY: Assumptions:
- 0/1 putters share the Inner
- 0/1 getters share the Inner
- inner.refs == sum of #getters + #putters
- Box is dropped if dropped and refs==1. contents are then dropped if inner.occuped == 1
*/
struct Shared<T> {
    inner: ManuallyDrop<Box<Inner<T>>>,
}
impl<T> Drop for Shared<T> {
    fn drop(&mut self) {
        if self.inner.refs.fetch_sub(1, Ordering::Relaxed) == 1 {
            if self.inner.occupied.load(Ordering::Relaxed) {
                unsafe {
                    ManuallyDrop::drop(&mut self.inner);
                }
            }
        }
    }
}

pub fn new_port<T>() -> (PutPort<T>, GetPort<T>) {
    let inner_box = Box::new(Inner {
        t: unsafe { mem::uninitialized() },
        refs: AtomicUsize::from(2),
        occupied: AtomicBool::from(false),
    });
    let inner = Box::into_raw(inner_box);
    let [inner1, inner2] = unsafe {
        [
            ManuallyDrop::new(Box::from_raw(inner)),
            ManuallyDrop::new(Box::from_raw(inner)),
        ]
    };
    let shared1 = Shared { inner: inner1 };
    let shared2 = Shared { inner: inner2 };
    (PutPort { shared: shared1 }, GetPort { shared: shared2, know_occupied: false })
}

///////////

pub struct PutPort<T> {
    shared: Shared<T>,
}
impl<T> PutPort<T> {
    pub fn put(&mut self, datum: T) {
        let was = self.shared.inner.occupied.swap(true, Ordering::Relaxed);
        let mut old = mem::replace(&mut self.shared.inner.t, ManuallyDrop::new(datum));
        if was {
            println!("PUT WAS SOMETHING");
            unsafe { ManuallyDrop::drop(&mut old) };
        } else {
            println!("PUT WASNT SOMETHING");
        }
    }
}

pub struct GetPort<T> {
    shared: Shared<T>,
    know_occupied: bool,
}
impl<T> GetPort<T> {
    pub fn get(&mut self) -> Option<T> {
        let was = self.shared.inner.occupied.swap(false, Ordering::Relaxed);
        self.know_occupied = false;
        if was {
            println!("GET WAS SOMETHING");
            let mut ret: ManuallyDrop<T> = ManuallyDrop::new(unsafe { mem::uninitialized() });
            mem::swap(&mut self.shared.inner.t, &mut ret);
            Some(ManuallyDrop::into_inner(ret))
        } else {
            println!("GET WASNT SOMETHING");
            None
        }
    }
    pub fn peek(&mut self) -> Option<&T> {
        if !self.know_occupied {
            self.know_occupied = self.shared.inner.occupied.load(Ordering::Relaxed);
        } 
        if self.know_occupied {
            Some(&self.shared.inner.t)
        } else {
            None
        }
    }
}

/*
TODO:
- find a way to BLOCK on the get and put...
- look into this atomic ordering business
- look into CachePadded
- signal registration and composition?
- blockstrategies?
*/

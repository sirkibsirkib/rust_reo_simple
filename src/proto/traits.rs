
use crate::proto::definition::LocInfo;

use super::*;

pub trait EndlessIter {
    fn endless_iter(
        &self,
    ) -> std::iter::Chain<std::slice::Iter<'_, usize>, std::iter::Repeat<&usize>>;
}
impl EndlessIter for Vec<usize> {
    fn endless_iter(
        &self,
    ) -> std::iter::Chain<std::slice::Iter<'_, usize>, std::iter::Repeat<&usize>> {
        self.iter().chain(std::iter::repeat(&0))
    }
}

pub(crate) trait HasMsgDropBox {
    fn get_dropbox(&self) -> &MsgDropbox;
    fn await_msg_timeout(&self, a: &ProtoAll, timeout: Duration, my_id: LocId) -> Option<usize> {
        println!("getting ... ");
        Some(match self.get_dropbox().recv_timeout(timeout) {
            Some(msg) => msg,
            None => {
                if a.w.lock().active.ready.set_to(my_id, false) {
                    // managed reverse my readiness
                    return None;
                } else {
                    // readiness has already been consumed
                    println!("too late");
                    self.get_dropbox().recv()
                }
            }
        })
    }
}
impl HasMsgDropBox for PoPuSpace {
    fn get_dropbox(&self) -> &MsgDropbox {
        &self.dropbox
    }
}
impl HasMsgDropBox for PoGeSpace {
    fn get_dropbox(&self) -> &MsgDropbox {
        &self.dropbox
    }
}

//////////////// INTERNAL SPECIALIZATION TRAITS for port-data ////////////
pub(crate) trait MaybeClone {
    fn maybe_clone(&self) -> Self;
}
impl<T> MaybeClone for T {
    default fn maybe_clone(&self) -> Self {
        panic!("type isn't clonable!")
    }
}

impl<T: Clone> MaybeClone for T {
    fn maybe_clone(&self) -> Self {
        self.clone()
    }
}

pub(crate) trait MaybeCopy {
    const IS_COPY: bool;
}
impl<T> MaybeCopy for T {
    default const IS_COPY: bool = false;
}

impl<T: Copy> MaybeCopy for T {
    const IS_COPY: bool = true;
}
pub(crate) trait MaybePartialEq {
    fn maybe_partial_eq(&self, other: &Self) -> bool;
}
impl<T> MaybePartialEq for T {
    default fn maybe_partial_eq(&self, _other: &Self) -> bool {
        panic!("type isn't partial eq!")
    }
}
impl<T: PartialEq> MaybePartialEq for T {
    fn maybe_partial_eq(&self, other: &Self) -> bool {
        self.eq(other)
    }
}

pub trait HasUnclaimedPorts {
    fn claim<T: 'static>(&self, id: LocId) -> ClaimResult<T>;
}
impl HasUnclaimedPorts for Arc<ProtoAll> {
    fn claim<T: 'static>(&self, id: LocId) -> ClaimResult<T> {
        use ClaimResult::*;
        let mut w = self.w.lock();
        if let Some(x) = w.unclaimed_ports.get(&id) {
            if x.type_id == TypeId::of::<T>() {
                let role = x.role;
                let _ = w.unclaimed_ports.remove(&id);
                let c = PortCommon {
                    p: self.clone(),
                    id,
                };
                match role {
                    PortRole::Putter => GotPutter(Putter {
                        c,
                        phantom: Default::default(),
                    }),
                    PortRole::Getter => GotGetter(Getter {
                        c,
                        phantom: Default::default(),
                    }),
                }
            } else {
                TypeMismatch
            }
        } else {
            NotUnclaimed
        }
    }
}

pub trait HasProto {
    fn get_proto(&self) -> &Arc<ProtoAll>;
}
impl<T: 'static> HasProto for Putter<T> {
    fn get_proto(&self) -> &Arc<ProtoAll> {
        &self.c.p
    }
}
impl<T: 'static> HasProto for Getter<T> {
    fn get_proto(&self) -> &Arc<ProtoAll> {
        &self.c.p
    }
}



/* Separate concerns:
1. rule structure
2. which memcells start initialized
3. LocId => LocKind
-------------------
4. LocId => TypeInfo
5. for all memcells initialized, what are their init values?

Rbpa needs to know 1,2,3
Proto needs all
1,2,3 are the same regardless of type instantiation

OK so here's the API:
1. fn get_structure() -> &'static ProtoDef; // optimization. caller needs it intact
2. fn mem_starts_initialized(id: LocId) -> bool;
3. fn mem_kind(id: LocId) -> LocKind;
4. dn get_type_info()


*/

#[derive(Debug, Copy, Clone)]
pub enum LocKindExt {
    PortPutter,
    PortGetter,
    MemInitialized,
    MemUninitialized,
}
pub struct TypelessProtoDef {
    pub structure: ProtoDef,
    pub loc_kind_ext: HashMap<LocId, LocKindExt>,
}
pub struct MemFillPromise<'a> {
    type_id_expected: TypeId,
    builder: &'a mut ProtoBuilder,
}
#[derive(Debug, Copy, Clone)]
pub struct WrongMemFillType {
    pub expected_type: TypeId,
}
pub struct MemFillPromiseFulfilled {
    _secret: (),
}

pub trait Proto: Sized {
    fn typeless_proto_def() -> &'static TypelessProtoDef;
    fn fill_memory(loc_id: LocId, promise: MemFillPromise) -> MemFillPromiseFulfilled;
    fn loc_kind_ext(loc_id: LocId) -> LocKindExt;
    fn loc_type(loc_id: LocId) -> &'static TypeInfo;

    fn instantiate() -> Arc<ProtoAll> {
        let def = Self::typeless_proto_def();

        // TODO change the API for protobuilder
        let mut builder = ProtoBuilder::new();
        for (&id, kind_ext) in def.loc_kind_ext.iter() {
            if let LocKindExt::MemInitialized = kind_ext {
                let mut promise = MemFillPromise {
                    type_id_expected: Self::loc_type(id).type_id,
                    builder: &mut builder,
                };
                let _promise_fulfilled = Self::fill_memory(id, promise);
            }
        }
        Arc::new(mem.finish(Self::loc_info()).expect("Bad finish"))
    }
}

pub(crate) trait DataSource<'a> {
    type MoveMcguffin: Sized;
    fn my_space(&self) -> &PutterSpace;
    fn execute_move(&self, mm: Self::MoveMcguffin, out_ptr: *mut u8);
    fn execute_clone(&self, out_ptr: *mut u8);
    fn send_done_signal(&self, someone_moved: bool);

    fn acquire_data<F: Fn() -> Self::MoveMcguffin>(
        &self,
        out_ptr: *mut u8,
        case: DataGetCase,
        mm_getter: F,
    ) {
        let space = self.my_space();
        let src = space.get_ptr();
        if space.type_info.is_copy {
            // MOVE HAPPENS HERE
            self.execute_move(mm_getter(), out_ptr);
            unsafe { src.copy_to(out_ptr, space.type_info.layout.size()) };
            let was = space.cloner_countdown.fetch_sub(1, Ordering::SeqCst);
            if was == case.last_countdown() {
                self.send_done_signal(true);
            }
        } else {
            if case.i_move() {
                if case.mover_must_wait() {
                    space.mover_sema.acquire();
                }
                // MOVE HAPPENS HERE
                self.execute_move(mm_getter(), out_ptr);
                self.send_done_signal(true);
            } else {
                // CLONE HAPPENS HERE
                unsafe { space.type_info.clone_fn.execute(src, out_ptr) };
                let was = space.cloner_countdown.fetch_sub(1, Ordering::SeqCst);
                if was == case.last_countdown() {
                    if case.someone_moves() {
                        space.mover_sema.release();
                    } else {
                        self.send_done_signal(false);
                    }
                }
            }
        }
    }
}

impl<'a> DataSource<'a> for PoPuSpace {
    type MoveMcguffin = ();
    fn my_space(&self) -> &PutterSpace {
        &self.p
    }
    fn execute_move(&self, _: Self::MoveMcguffin, out_ptr: *mut u8) {
        let src: *mut u8 = self.p.remove_ptr();
        unsafe { self.p.type_info.move_fn_execute(src, out_ptr) };
    }
    fn execute_clone(&self, out_ptr: *mut u8) {
        let src: *mut u8 = self.p.get_ptr();
        unsafe { self.p.type_info.clone_fn.execute(src, out_ptr) };
    }
    fn send_done_signal(&self, someone_moved: bool) {
        self.dropbox.send(if someone_moved { 1 } else { 0 });
    }
}

impl<'a> DataSource<'a> for MemoSpace {
    type MoveMcguffin = MutexGuard<'a, ProtoW>;
    fn my_space(&self) -> &PutterSpace {
        &self.p
    }
    fn execute_move(&self, mut w: Self::MoveMcguffin, out_ptr: *mut u8) {
        let src: *mut u8 = self.p.get_ptr();
        let refs: &mut usize = w.active.mem_refs.get_mut(&src).expect("no memrefs?");
        assert!(*refs >= 1);
        *refs -= 1;
        if *refs == 0 {
            w.active.mem_refs.remove(&src);
            unsafe {
                w.active
                    .storage
                    .move_out(src, out_ptr, &self.p.type_info.layout)
            }
        } else {
            panic!("Tried to MOVE out a memcell with 2+ aliases")
        }
    }
    fn execute_clone(&self, out_ptr: *mut u8) {
        let src: *mut u8 = self.p.get_ptr();
        unsafe { self.p.type_info.clone_fn.execute(src, out_ptr) };
    }
    fn send_done_signal(&self, _someone_moved: bool) {
        // nothing to do here
    }
}

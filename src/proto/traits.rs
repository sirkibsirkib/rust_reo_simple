use super::*;
use crate::proto::definition::FuncDef;

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
    const IS_DEFINED: bool;
    fn maybe_clone(&self) -> Self;
}
impl<T> MaybeClone for T {
    default const IS_DEFINED: bool = false;
    default fn maybe_clone(&self) -> Self {
        panic!("type isn't clonable!")
    }
}

impl<T: Clone> MaybeClone for T {
    const IS_DEFINED: bool = true;
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
    const IS_DEFINED: bool;
    fn maybe_partial_eq(&self, other: &Self) -> bool;
}
impl<T> MaybePartialEq for T {
    default const IS_DEFINED: bool = false;
    default fn maybe_partial_eq(&self, _other: &Self) -> bool {
        panic!("type isn't partial eq!")
    }
}
impl<T: PartialEq> MaybePartialEq for T {
    const IS_DEFINED: bool = true;
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
                let phantom = Default::default();
                match role {
                    PortRole::Putter => GotPutter(Putter { c, phantom }),
                    PortRole::Getter => GotGetter(Getter { c, phantom }),
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

pub struct MemFillPromise<'a> {
    pub(crate) type_id_expected: TypeId,
    pub(crate) loc_id: LocId,
    pub(crate) builder: &'a mut ProtoBuilder,
}
impl<'a> MemFillPromise<'a> {
    pub fn fill_memory<T: 'static>(self, t: T) -> Result<PromiseFulfilled, WrongMemFillType> {
        if TypeId::of::<T>() != self.type_id_expected {
            Err(WrongMemFillType {
                expected_type: self.type_id_expected,
            })
        } else {
            self.builder.define_init_memory(self.loc_id, t);
            Ok(unsafe { std::mem::transmute(()) })
        }
    }
}

pub struct FuncDefPromise<'a> {
    pub(crate) builder: &'a mut ProtoBuilder,
    pub(crate) name: &'static str,
}

use std::mem::MaybeUninit;
impl<'a> FuncDefPromise<'a> {
    pub fn define_arity0<R: 'static>(self, func: fn(&mut MaybeUninit<R>)) -> PromiseFulfilled {
        let def = FuncDef {
            ret_info: Arc::new(TypeInfo::new::<R>()),
            param_info: vec![],
            fnptr: unsafe { std::mem::transmute(func) },
        };
        self.builder.define_func(self.name, def);
        unsafe { std::mem::transmute(()) }
    }

    pub fn define_arity1<R: 'static, A0: 'static>(
        self,
        func: fn(&mut MaybeUninit<R>, *const A0),
    ) -> PromiseFulfilled {
        let def = FuncDef {
            ret_info: Arc::new(TypeInfo::new::<R>()),
            param_info: vec![Arc::new(TypeInfo::new::<A0>())],
            fnptr: unsafe { std::mem::transmute(func) },
        };
        self.builder.define_func(self.name, def);
        unsafe { std::mem::transmute(()) }
    }
    // TODO 2 and 3
}

#[derive(Debug, Copy, Clone)]
pub struct WrongMemFillType {
    pub expected_type: TypeId,
}
pub enum PromiseFulfilled {}

/// Does not enforce that used LocIds have any particular order or are contiguous,
/// HOWEVER, leaving gaps in ID-SPACE will reduce efficiency by:
/// 1. leaving gaps in the protocol's internal buffer (it uses a vector of Space objects)
/// 2. making inefficient use of bitset operations
pub trait Proto: Sized {
    fn typeless_proto_def() -> &'static TypelessProtoDef;
    fn fill_memory(loc_id: LocId, promise: MemFillPromise) -> Option<PromiseFulfilled>;
    fn def_func(func_name: &'static str, promise: FuncDefPromise) -> Option<PromiseFulfilled>;
    fn loc_type(loc_id: LocId) -> Option<TypeInfo>;
    fn try_instantiate() -> Result<Arc<ProtoAll>, ProtoBuildErr> {
        use ProtoBuildErr::*;
        let mut builder = ProtoBuilder::new();
        for (&loc_id, kind_ext) in Self::typeless_proto_def().loc_kinds.iter() {
            if let LocKind::MemInitialized = kind_ext {
                let promise = MemFillPromise {
                    loc_id,
                    type_id_expected: Self::loc_type(loc_id)
                        .ok_or(UnknownType { loc_id })?
                        .type_id,
                    builder: &mut builder,
                };
                Self::fill_memory(loc_id, promise).ok_or(MemoryFillPromiseBroken { loc_id })?;
            }
        }
        Ok(Arc::new(builder.finish::<Self>()?))
    }
    fn instantiate() -> Arc<ProtoAll> {
        match Self::try_instantiate() {
            Ok(x) => x,
            Err(e) => panic!("Instantiate failed! {:?}", e),
        }
    }
    type Interface: Sized;
    fn instantiate_and_claim() -> Self::Interface;
}

pub(crate) trait DataSource<'a> {
    type Finalizer: Sized;
    fn my_space(&self) -> &PutterSpace;
    fn execute_clone(&self, out_ptr: *mut u8);
    fn execute_copy(&self, out_ptr: *mut u8);
    fn finalize(&self, someone_moved: bool, fin: Self::Finalizer);

    fn acquire_data<I>(&self, mut out_ptrs: I, fin: Self::Finalizer)
    where
        I: ExactSizeIterator<Item = *mut u8>,
    {
        
        use Ordering::SeqCst;
        let space = self.my_space();
        if space.type_info.is_copy {
            if out_ptrs.len() > 0 {
                space.move_flags.type_is_copy_i_moved();
            }
            for out_ptr in out_ptrs {
                self.execute_copy(out_ptr);
            }
            let was = space.cloner_countdown.fetch_sub(1, SeqCst);
            if was == 1 {
                let somebody_moved = space.move_flags.did_someone_move();
                self.finalize(somebody_moved, fin);
            }
        } else {
            if out_ptrs.len() > 0 {
                let won = !space.move_flags.ask_for_move_permission();
                if won {
                    let was = space.cloner_countdown.fetch_sub(1, SeqCst);
                    if was == 1 {
                        let move_to = out_ptrs.next().unwrap();
                        for out_ptr in out_ptrs {
                            self.execute_clone(out_ptr);
                        }
                        self.execute_copy(move_to);
                    } else {
                        space.mover_sema.acquire();
                    }
                    self.finalize(true, fin);
                } else {
                    // lose
                    for out_ptr in out_ptrs {
                        self.execute_clone(out_ptr);
                    }
                    let was = space.cloner_countdown.fetch_sub(1, SeqCst);
                    if was == 1 {
                        // all clones are done
                        space.mover_sema.release();
                    } else {
                        // do nothing
                    }
                }
            } else {
                let was = space.cloner_countdown.fetch_sub(1, SeqCst);
                if was == 1 {
                    // all clones done
                    let nobody_else_won = !space.move_flags.ask_for_move_permission();
                    if nobody_else_won {
                        self.finalize(false, fin);
                    } else {
                        space.mover_sema.release();
                    }
                }
            }
        }
    }
}

impl<'a> DataSource<'a> for TempSpace {
    type Finalizer = <MemoSpace as DataSource<'a>>::Finalizer;
    fn my_space(&self) -> &PutterSpace {
        self.0.my_space()
    }
    fn execute_copy(&self, out_ptr: *mut u8) {
        self.0.execute_copy(out_ptr)
    }
    fn execute_clone(&self, out_ptr: *mut u8) {
        self.0.execute_clone(out_ptr)
    }
    fn finalize(&self, someone_moved: bool, fin: Self::Finalizer) {
        self.0.finalize(someone_moved, fin)
    }
}

impl<'a> DataSource<'a> for PoPuSpace {
    type Finalizer = ();
    fn my_space(&self) -> &PutterSpace {
        &self.p
    }
    fn execute_copy(&self, out_ptr: *mut u8) {
        let src: *mut u8 = self.p.remove_ptr();
        unsafe { self.p.type_info.copy_fn_execute(src, out_ptr) };
    }
    fn execute_clone(&self, out_ptr: *mut u8) {
        let src: *mut u8 = self.p.get_ptr();
        unsafe { self.p.type_info.funcs.clone.execute(src, out_ptr) };
    }
    fn finalize(&self, someone_moved: bool, _fin: Self::Finalizer) {
        let msg = if someone_moved { 1 } else { 0 };
        println!("POPU FIN MSG = {}", msg);
        self.dropbox.send(msg);
    }
}

impl<'a> DataSource<'a> for MemoSpace {
    type Finalizer = (&'a ProtoAll, LocId);
    fn my_space(&self) -> &PutterSpace {
        &self.p
    }
    fn execute_copy(&self, out_ptr: *mut u8) {
        let src: *mut u8 = self.p.get_ptr();
        unsafe { self.p.type_info.copy_fn_execute(src, out_ptr) };
    }
    fn execute_clone(&self, out_ptr: *mut u8) {
        let src: *mut u8 = self.p.get_ptr();
        unsafe { self.p.type_info.funcs.clone.execute(src, out_ptr) };
    }
    fn finalize(&self, someone_moved: bool, fin: Self::Finalizer) {
        println!("PO GE FINALIZE CLEANUP MEM");
        let mut w = fin.0.w.lock();
        let putter_id = fin.1;
        self.make_empty(&mut w.active, !someone_moved, putter_id);
        w.ready_set_coordinate(&fin.0.r, putter_id);
    }
}

pub trait Parsable: 'static + Sized {
    fn try_parse(s: &str) -> Option<Self>;
}
impl<T: 'static> Parsable for T
where
    T: FromStr,
    <Self as FromStr>::Err: Debug,
{
    fn try_parse(s: &str) -> Option<Self> {
        T::from_str(s).ok()
    }
}

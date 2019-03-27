use std::ops::Deref;
use crossbeam::{Receiver, Sender};
use mio::{Ready, Registration, SetReadiness};
use std::io;

pub trait Component {
    fn run(&mut self);
}

#[derive(Debug, Copy, Clone)]
pub struct PortClosed;

pub fn new_port<T>() -> (Putter<T>, Getter<T>) {
    let (data_s, data_r) = crossbeam::channel::bounded(0);
    let (sig_s, sig_r) = crossbeam::channel::bounded(0);
    let (g_reg, g_red) = mio::Registration::new2();
    let (p_reg, p_red) = mio::Registration::new2();
    let p = Putter {
        data: data_s,
        done_sig: sig_r,
        putter_reg: p_reg,
        getter_set_ready: g_red,
    };
    let g = Getter {
        cache: None,
        data: data_r,
        done_sig: sig_s,
        getter_reg: g_reg,
        putter_set_ready: p_red,
    };
    (p, g)
}

pub trait ClosedErrorable<T> {
    fn closed_err(self) -> Result<T,PortClosed>;
}
impl<T,E> ClosedErrorable<T> for Result<T,E> {
    fn closed_err(self) -> Result<T,PortClosed> {
        self.map_err(|_| PortClosed)
    }
}

//////////////////////////////////////////////////

pub struct Putter<T> {
    data: Sender<T>,
    done_sig: Receiver<()>,
    putter_reg: Registration,
    getter_set_ready: SetReadiness,
}

pub struct Getter<T> {
    cache: Option<T>,
    data: Receiver<T>,
    done_sig: Sender<()>,
    getter_reg: Registration,
    putter_set_ready: SetReadiness,
}

//////////////////////////////
impl<T> Putter<T> {
    pub fn put(&mut self, datum: T) -> Result<(), PortClosed> {
        self.set_peer_readiness(true).unwrap();
        self.data.send(datum).closed_err()?;
        self.done_sig.recv().closed_err()?;
        self.set_peer_readiness(false).unwrap();
        Ok(())
    }
    fn set_peer_readiness(&self, is_ready: bool) -> Result<(), io::Error> {
        let r = if is_ready {
            Ready::readable()
        } else {
            Ready::empty()
        };
        self.getter_set_ready.set_readiness(r)
    }
    pub fn reg(&self) -> &Registration {
        &self.putter_reg
    }
}

impl<T> Getter<T> {
    // like peek but guaranteed never to block
    pub fn try_peek(&mut self) -> Result<Option<&T>,PortClosed> {
        if self.cache.is_none() {
            use crossbeam::channel::TryRecvError;
            self.cache.replace(
                match self.data.try_recv() {
                    Ok(datum) => {
                        self.set_peer_readiness(true).unwrap();
                        datum
                    },
                    Err(TryRecvError::Empty) => return Ok(None),
                    Err(TryRecvError::Disconnected) => return Err(PortClosed),
                }
            );
        }
        Ok(Some(self.cache.as_ref().unwrap()))        
    }

    // like get but does not remove the datum
    pub fn peek(&mut self) -> Result<&T, PortClosed> {
        if self.cache.is_none() {
            self.cache.replace(self.acquire_from_putter()?);
        }
        Ok(self.cache.as_ref().unwrap())
    }
    // acquires datum from putter. blocks until ready
    pub fn get(&mut self) -> Result<T, PortClosed> {
        let datum = self
            .cache
            .take()
            .or_else(|| self.acquire_from_putter().ok())
            .ok_or(PortClosed)?;
        self.done_sig.send(()).closed_err()?;
        self.set_peer_readiness(false).unwrap();
        Ok(datum)
    }
    pub fn reg(&self) -> &Registration {
        &self.getter_reg
    }
    fn acquire_from_putter(&self) -> Result<T, PortClosed> {
        self.set_peer_readiness(true).unwrap();
        Ok(self.data.recv().closed_err()?)
    }
    fn set_peer_readiness(&self, is_ready: bool) -> Result<(), io::Error> {
        let r = if is_ready {
            Ready::writable()
        } else {
            Ready::empty()
        };
        self.putter_set_ready.set_readiness(r)
    }
}

////////////////////////////////////////////////////

impl<T> Drop for Putter<T> {
    fn drop(&mut self) {
        let _ = self.set_peer_readiness(true);
    }
}
impl<T> Drop for Getter<T> {
    fn drop(&mut self) {
        let _ = self.set_peer_readiness(true);
    }
}

struct EventedTup {
    reg: Registration,
    ready: SetReadiness,
}
impl Default for EventedTup {
    fn default() -> Self {
        let (reg, ready) = mio::Registration::new2();
        Self {reg, ready}
    }
}
#[derive(Default)]
pub struct Memory<T> {
    shutdown: bool,
    data: Option<T>,
    full: EventedTup,
    empty: EventedTup,
}
impl<T> Memory<T> {
    pub fn shutdown(&mut self) {
        if !self.shutdown {
            self.shutdown = true;
            println!("SHUTTING DOWN");
            // self.update_ready();
            let _ = self.empty.ready.set_readiness(Ready::writable());
            let _ = self.full.ready.set_readiness(Ready::readable());
        }
    }
    pub fn put(&mut self, datum: T) -> Result<(),T> {
        if self.shutdown {return Err(datum)}
        match self.data.replace(datum) {
            None => {
                // println!("PUT MEM");
                self.update_ready();
                Ok(())
            },
            Some(x) => Err(x),
        }
    }
    pub fn get(&mut self) -> Result<T,PortClosed> {
        if self.shutdown {return Err(PortClosed)}
        match self.data.take() {
            Some(x) => {
                self.update_ready();
                Ok(x)
            },
            None => Err(PortClosed),
        }
    }
    pub fn peek(&self) -> Result<&T,PortClosed> {
        if self.shutdown {return Err(PortClosed)}
        match self.data.as_ref() {
            Some(x) => Ok(x),
            None => Err(PortClosed),
        }
    }
    pub fn reg_g(&self) -> impl Deref<Target=Registration> + '_ {
        RegHandle { 
            reg: &self.full.reg,
            when_dropped: move || self.update_ready(),
        }
    }
    pub fn reg_p(&self) -> impl Deref<Target=Registration> + '_ {
        RegHandle { 
            reg: &self.empty.reg,
            when_dropped: move || self.update_ready(),
        }
    }
    pub fn update_ready(&self) {
        if self.data.is_none() {
            let _ = self.empty.ready.set_readiness(Ready::writable());
            let _ = self.full.ready.set_readiness(Ready::empty());
        } else {
            let _ = self.empty.ready.set_readiness(Ready::empty());
            let _ = self.full.ready.set_readiness(Ready::readable());
        }
    }
}

struct RegHandle<'a,F> where F: Fn() {
    reg: &'a Registration,
    when_dropped: F,
}
impl<'a,F> Drop for RegHandle<'a,F> where F: Fn() {
    fn drop(&mut self) {
        (self.when_dropped)()
    }
}
impl<'a,F> std::ops::Deref for RegHandle<'a,F> where F: Fn() {
    type Target = Registration;
    fn deref(&self) -> &Self::Target {
        &self.reg
    }
}


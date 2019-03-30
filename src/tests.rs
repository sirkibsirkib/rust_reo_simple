
use bit_set::BitSet;
use mio::{Poll, PollOpt, Ready, Token};
use crate::reo::{self, ClosedErrorable, Component, Getter, Memory, Putter};
use crate::protocols::{GuardCmd, ProtoComponent};

struct Producer {
    p_out: Putter<u32>,
    offset: u32,
}
impl Component for Producer {
    fn run(&mut self) {
        for i in 0..3 {
            self.p_out.put(i + self.offset).unwrap();
        }
    }
}

struct Consumer {
    p_in: Getter<u32>,
}
impl Component for Consumer {
    fn run(&mut self) {
        while let Ok(x) = self.p_in.get() {
            println!("{:?}", x);
        }
    }
}
struct ProdConsProto {
    p00g: Getter<u32>,
    p01g: Getter<u32>,
    p02p: Putter<u32>,
    m00: Memory<u32>,
}
impl ProdConsProto {

    #[rustfmt::skip]
    pub fn new(p00g: Getter<u32>, p01g: Getter<u32>, p02p: Putter<u32>) -> Self {
        let m00 = Default::default();
        Self { p00g, p01g, p02p, m00 }
    }
}

def_consts![0 => P00G, P01G, P02P, M00G, M00P];
impl ProtoComponent for ProdConsProto {
    fn get_local_peer_token(&self, token: usize) -> Option<usize> {
        Some(match token {
            M00P => M00G,
            M00G => M00P,
            _ => return None,
        })
    }
    fn token_shutdown(&mut self, token: usize) {
        match token {
            M00P | M00G => self.m00.shutdown(),
            _ => {},
        }
    }
    fn register_all(&mut self, poll: &Poll) {
        let a = Ready::all();
        let edge = PollOpt::edge();
        poll.register(self.p00g.reg(), Token(P00G), a, edge).unwrap();
        poll.register(self.p01g.reg(), Token(P01G), a, edge).unwrap();
        poll.register(self.p02p.reg(), Token(P02P), a, edge).unwrap();
        poll.register(self.m00.reg_p().as_ref(), Token(M00P), a, edge).unwrap();
        poll.register(self.m00.reg_g().as_ref(), Token(M00G), a, edge).unwrap();
    }
}
impl Component for ProdConsProto {
    fn run(&mut self) {
        let mut gcmds = vec![];
        guard_cmd!(gcmds,
            bitset! {P00G,P01G,P02P,M00P},
            |_me: &mut Self| {
                true
            },
            |me: &mut Self| {
                me.p02p.put(me.p00g.get()?).closed_err()?;
                me.m00.put(me.p01g.get()?).closed_err()?;
                Ok(())
            }
        );
        guard_cmd!(gcmds,
            bitset! {P02P,M00G},
            |_me: &mut Self| {
                true
            },
            |me: &mut Self| {
                me.p02p.put(me.m00.get()?).closed_err()?;
                Ok(())
            }
        );
        self.run_to_termination(&gcmds);
    }
}

#[test]
fn alternator() {
    // create ports
    let (p00p, p00g) = reo::new_port();
    let (p01p, p01g) = reo::new_port();
    let (p02p, p02g) = reo::new_port();

    // spin up threads
    #[rustfmt::skip]
    crossbeam::scope(|s| {
        s.builder()
            .name("Producer_1".into())
            .spawn(move |_| Producer { p_out: p00p, offset: 0 }.run())
            .unwrap();
        s.builder()
            .name("Producer_2".into())
            .spawn(move |_| Producer { p_out: p01p, offset: 100 }.run())
            .unwrap();
        s.builder()
            .name("Proto".into())
            .spawn(move |_| ProdConsProto::new(p00g, p01g, p02p).run())
            .unwrap();
        s.builder()
            .name("Consumer".into())
            .spawn(move |_| Consumer { p_in: p02g }.run())
            .unwrap();
    })
    .expect("A worker thread panicked!");
}





///////////////////////////////////

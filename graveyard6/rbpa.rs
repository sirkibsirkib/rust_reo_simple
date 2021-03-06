use smallvec::{SmallVec, smallvec};
use crate::{AbstractRuleId, PortId};
use hashbrown::HashSet;
use itertools::izip;
use std::{cmp, fmt, mem, ops};

// macros
macro_rules! ss {
    ($( $arr:expr ),* ) => {{
        StateSet { predicate: smallvec![ $($arr),*] }
    }};
}

macro_rules! hashset {
    (@single $($x:tt)*) => (());
    (@count $($rest:expr),*) => (<[()]>::len(&[$(hashset!(@single $rest)),*]));

    ($($key:expr,)+) => { hashset!($($key),+) };
    ($($key:expr),*) => {
        {
            let _cap = hashset!(@count $($key),*);
            let mut _set = HashSet::with_capacity(_cap);
            $(
                let _ = _set.insert($key);
            )*
            _set
        }
    };
}

// part of the state-set predicate.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum Var {
    T,
    F, // specific values corresponding to boolean true and false,
    X, // generic over T and F. Interpreted as an unspecified value.
}

impl PartialOrd for Var {
    // ordering is on SPECIFICITY: X<T, X<F, T is not comparable to F.
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        use Var::*;
        match [self, other] {
            [a, b] if a == b => Some(cmp::Ordering::Equal),
            [X, _] => Some(cmp::Ordering::Less),
            [_, X] => Some(cmp::Ordering::Greater),
            _ => None,
        }
    }
}
impl ops::Neg for Var {
    type Output = Self;
    fn neg(self) -> Self::Output {
        use Var::*;
        match self {
            X => X,
            T => F,
            F => T,
        }
    }
}
impl Var {
    pub fn is_generic(self) -> bool {
        self == Var::X
    }
    pub fn is_specific(self) -> bool {
        !self.is_generic()
    }
    pub fn mismatches(self, other: Self) -> bool {
        self.partial_cmp(&other).is_none()
    }
}

// represents a port automaton with transitions represented as logical rules
#[derive(Debug, Clone)]
pub struct Rbpa {
    /*  A mask for which memory-variable indices are _irrelevant_.
    where the value is `false`, T==F==X. */
    mask: StateMask,
    rules: Vec<AbstractRule>,
}
impl Rbpa {
    pub fn mask_irrelevant_vars(&mut self) -> bool {
        let mut changed_something = false;
        'outer: for i in 0..StateSet::LEN {
            if !self.mask.relevant_index[i] {
                continue; // already irrelevant
            }
            for r in self.rules.iter() {
                if r.guard.predicate[i].is_specific() {
                    continue 'outer;
                }
            }
            // this index is irrelevant
            println!("index {} is irrelevant", i);
            self.mask.relevant_index[i] = false;
            changed_something = true;
            for r in self.rules.iter_mut() {
                r.guard.predicate[i] = Var::X;
                r.assign.predicate[i] = Var::X;
            }
        }
        changed_something
    }
    pub fn normalize(&mut self) {
        let mut buf = vec![];
        while let Some(idx) = self.first_silent_idx() {
            let silent = self.rules.remove(idx);
            println!("... Removing silent rule at idx={}", idx);
            if silent.no_effect() {
                // when [silent . x] == x
                continue;
            }
            for (i, r) in self.rules.iter().enumerate() {
                if let Some(composed) = silent.compose(r) {
                    let old_i = if i >= idx { i + 1 } else { i };
                    println!("ADDING composed rule ({},{})", idx, old_i);
                    buf.push(composed);
                }
            }
            self.rules.append(&mut buf);
            println!("AFTER: {:#?}\n----------------", &self.rules);
            self.merge_rules();
            println!("... rules_merged {:#?}", &self.rules);
        }
        while self.mask_irrelevant_vars() || self.merge_rules() {
            println!("whiling away {:#?}", &self.rules);
        }
        // DONE
    }
    pub fn first_silent_idx(&self) -> Option<usize> {
        self.rules
            .iter()
            .enumerate()
            .filter(|(_, r)| r.is_silent())
            .map(|(i, _)| i)
            .next()
    }
    pub fn merge_rules(&mut self) -> bool {
        let mut changed_something = false;
        'outer: loop {
            for (idx1, r1) in self.rules.iter().enumerate() {
                let rng = (idx1 + 1)..;
                for (idx2, r2) in izip!(rng.clone(), self.rules[rng].iter()) {
                    if let Some(new_rule) = r1.try_merge(r2) {
                        changed_something = true;
                        let _ = mem::replace(&mut self.rules[idx1], new_rule);
                        self.rules.remove(idx2);
                        continue 'outer;
                    }
                }
            }
            return changed_something;
        }
    }
}

#[derive(Eq, PartialEq, Clone, Hash)]
pub struct StateSet {
    predicate: SmallVec<[Var;16]>,
}
impl PartialOrd for StateSet {
    fn partial_cmp(&self, rhs: &Self) -> Option<cmp::Ordering> {
        use cmp::Ordering::*;
        let mut o = Equal;
        for (&a, &b) in izip!(self.iter(), rhs.iter()) {
            match a.partial_cmp(&b) {
                None => return None,
                Some(x @ Less) | Some(x @ Greater) => {
                    if o == Equal {
                        o = x;
                    } else if o != x {
                        return None;
                    }
                }
                Some(Equal) => (),
            }
        }
        Some(o)
    }
}
impl StateSet {
    const LEN: usize = 3;
    pub fn make_specific_wrt(&mut self, other: &Self) {
        for (s, o) in izip!(self.iter_mut(), other.iter()) {
            if *s < *o {
                // s is X, o is specific. copy specific value.
                *s = *o;
            }
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = &Var> {
        self.predicate.iter()
    }
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Var> {
        self.predicate.iter_mut()
    }
    pub fn sisters<'a>(&'a self) -> impl Iterator<Item = Self> + 'a {
        (0..StateSet::LEN).filter_map(move |i| match -self.predicate[i] {
            Var::X => None,
            t_or_f => {
                let mut x = self.clone();
                x.predicate[i] = t_or_f;
                Some(x)
            }
        })
    }
}
impl fmt::Debug for StateSet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for x in self.predicate.iter() {
            write!(f, "{:?}", x)?;
        }
        Ok(())
    }
}

#[derive(Clone)]
struct AbstractRule {
    // invariant: an X in assignment implies an X in guard at same position
    guard: StateSet,
    port: Option<PortId>,
    assign: StateSet,
    ids: Vec<AbstractRuleId>,
}
impl AbstractRule {
    // apply the given rule to this state set. Return the new state set
    pub fn apply(&self, set: &StateSet) -> Option<StateSet> {
        let mut res = set.clone();
        for (&g, &a, r) in izip!(self.guard.iter(), self.assign.iter(), res.iter_mut()) {
            if g.mismatches(*r) {
                // guard is not satisfied.
                return None;
            } else if a.is_specific() {
                // explicit assignment. Eg: X->T or T->T
                *r = a;
            } else if g.is_specific() {
                /* implicit assignment because of guard. Eg: X  ==> (T->F) ==> T
                   this branch is unreachable assuming the invariant holds.
                   it is nonetheless included for robustness.
                */
                *r = g;
            }
        }
        Some(res)
    }
    // true <==> the rule does not affect memory & involves no port.
    pub fn no_effect(&self) -> bool {
        if self.port.is_some() {
            return false;
        }
        for (&g, &a) in izip!(self.guard.iter(), self.assign.iter()) {
            if g.mismatches(a) || g < a {
                return false;
            }
        }
        true
    }

    // if these two rules can be represented by one, return that rule
    pub fn try_merge(&self, other: &Self) -> Option<AbstractRule> {
        let g_cmp = self.guard.partial_cmp(&other.guard);
        let a_cmp = self.assign.partial_cmp(&other.assign);

        use cmp::Ordering::*;
        match [g_cmp, a_cmp] {
            [Some(g), Some(a)] if (a == Equal || a == g) && (g == Equal || g == Less) => {
                // 1st rule subsumes the 2nd. Eg: [XT->XT, FT->FT]
                Some(self.clone())
            }
            [Some(g), Some(a)] if (a == Equal || a == g) && g == Greater => {
                // 2nd rule subsumes the 1st.
                Some(other.clone())
            }
            [None, Some(Equal)] => {
                // 2nd case. There exists rule R which is split in half by these two rules. Return R.
                // eg: {T->T, F->T} give X->T
                let mut guard = self.guard.clone();
                let mut equal_so_far = true;
                for (g, &g2) in izip!(guard.iter_mut(), other.guard.iter()) {
                    if *g != g2 {
                        if !equal_so_far {
                            // 2+ indices mismatch
                            return None;
                        }
                        equal_so_far = false;
                        *g = Var::X;
                    }
                }
                let mut ids = self.ids.clone();
                ids.extend(&other.ids[..]);
                Some(AbstractRule::new(
                    guard,
                    self.port.clone(),
                    self.assign.clone(),
                    ids,
                ))
            }
            _ => None,
        }
    }
    // return a new rule that represents two rules applied in the sequence: [self, other]
    // prodecure fails if provided rules that cannot be composed. Eg: [F->F, T->T]
    pub fn compose(&self, other: &Self) -> Option<AbstractRule> {
        // println!("composing {:?} and {:?}", self, other);
        if !self.can_precede(other) {
            return None;
        }
        let port: Option<PortId> = self.port.or(other.port);
        let mut guard = self.guard.clone();
        /* where the LATTER rule specifies something the FORMER leaves generic, specify it.
           Eg: [X->X . F->T] becomes [F->T] not [X->T]
           Note: initially g = g1
        */
        for (&a1, &g2, g) in izip!(self.assign.iter(), other.guard.iter(), guard.iter_mut()) {
            if g.is_generic() && a1.is_generic() && g2.is_specific() {
                *g = g2;
            }
        }
        /* where the FORMER rule specifies something the LATTER leaves generic, specify it.
           Eg: [F->T . X->X] becomes [F->T] not [F->X]
           Note: initially a == a2
        */
        let mut assign = other.assign.clone();
        for (a, &g1, &a1, &g2) in izip!(
            assign.iter_mut(),
            self.guard.iter(),
            self.assign.iter(),
            other.guard.iter(),
        ) {
            let latter_is_generic = g2.is_generic() && a.is_generic();
            if latter_is_generic {
                if a1.is_specific() {
                    *a = a1;
                } else if g1.is_specific() {
                    *a = g1;
                }
            }
        }
        Some(AbstractRule::new(guard, port, assign, other.ids.clone()))
    }
    pub fn new(
        guard: StateSet,
        port: Option<PortId>,
        mut assign: StateSet,
        ids: Vec<AbstractRuleId>,
    ) -> Self {
        assign.make_specific_wrt(&guard);
        Self {
            guard,
            port,
            assign,
            ids,
        }
    }
    pub fn is_silent(&self) -> bool {
        self.port.is_none()
    }
    pub fn can_precede(&self, other: &Self) -> bool {
        for (&a, &g) in izip!(self.assign.iter(), other.guard.iter()) {
            // assignment of first rule produces something that mismatches guard of the 2nd.
            if a.mismatches(g) {
                return false;
            }
        }
        if self.port.is_some() && other.port.is_some() {
            // rules by our definition can involve 0 or 1 ports. this would require 2.
            return false;
        }
        true
    }
}
impl fmt::Debug for AbstractRule {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.port {
            Some(p) => write!(f, "{:?} ={:?}=> {:?}", &self.guard, p, &self.assign)?,
            None => write!(f, "{:?} =.=> {:?}", &self.guard, &self.assign)?,
        };
        write!(f, " ids: [")?;
        for id in self.ids.iter() {
            write!(f, "{},", id)?;
        }
        write!(f, "]")
    }
}

#[test]
fn testy() {
    wahey()
}

pub fn project(mut r: Rbpa, atomic_ports: HashSet<PortId>) -> Rbpa {
    for rule in r.rules.iter_mut() {
        if let Some(p) = rule.port {
            if !atomic_ports.contains(&p) {
                // hide this port. Transition becomes silent.
                rule.port = None;
            }
        }
    }
    r.normalize();
    r
}

pub fn wahey() {
    println!("RULE {:?}", mem::size_of::<AbstractRule>());
    println!("Rbpa {:?}", mem::size_of::<Rbpa>());
    println!("StateSet {:?}", mem::size_of::<StateSet>());
    use Var::*;
    let rba = Rbpa {
        rules: vec![
            AbstractRule::new(ss![X, X, F], Some(1), ss![X, X, T], vec![0]),
            AbstractRule::new(ss![X, F, T], Some(1), ss![X, T, F], vec![1]),
            AbstractRule::new(ss![F, T, T], Some(3), ss![T, F, F], vec![2]),
            AbstractRule::new(ss![T, T, T], Some(4), ss![F, F, F], vec![3]),
        ],
        mask: StateMask {
            relevant_index: [true; StateSet::LEN],
        },
    };
    let org = rba.clone();
    println!("BEFORE");
    for r in rba.rules.iter() {
        println!("{:?}", r);
    }
    let atomic_ports = hashset! {1,2};
    let start = std::time::Instant::now();
    let rba2 = project(rba, atomic_ports.clone());
    println!("ELAPSED {:?}", start.elapsed());
    println!("AFTER: {:#?}", rba2);
    pair_test(ss![F, F, F], org, rba2, atomic_ports);
}

#[derive(Clone, derive_new::new)]
pub struct StateMask {
    relevant_index: [bool; StateSet::LEN],
}
impl fmt::Debug for StateMask {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for &x in self.relevant_index.iter() {
            write!(f, "{}", if x { '1' } else { '0' })?;
        }
        Ok(())
    }
}
impl StateMask {
    pub fn mask(&self, state: StateSet) -> StateSet {
        let mut ret = state.clone();
        for (r, &b) in izip!(ret.iter_mut(), self.relevant_index.iter()) {
            if !b {
                *r = Var::X;
            }
        }
        ret
    }
}

pub fn pair_test(mut state: StateSet, rba: Rbpa, atomic: Rbpa, atomic_ports: HashSet<PortId>) {
    println!("PROTO: {:#?}\nATOMIC: {:#?}", &rba, &atomic);
    // let mut buf = HashSet::default();
    let mut atomic_state = state.clone();
    let mut rng = rand::thread_rng();
    let mut trace = format!("P: {:?}", &state);
    let mut trace_atomic = format!("A: {:?}", &state);
    let mut try_order: Vec<usize> = (0..rba.rules.len()).collect();

    'outer: for _ in 0..24 {
        use rand::seq::SliceRandom;
        try_order.shuffle(&mut rng);
        for rule in try_order.iter().map(|&i| &rba.rules[i]) {
            if let Some(new_state) = rule.apply(&state) {
                state = new_state.clone();
                while trace_atomic.len() < trace.len() {
                    trace_atomic.push(' ');
                }
                trace.push_str(&match rule.port {
                    Some(p) => format!(" --{}-> {:?}", p, &new_state),
                    None => format!(" --.-> {:?}", &new_state),
                });
                if let Some(p) = rule.port {
                    if atomic_ports.contains(&p) {
                        // took NONSILENT TRANSITION
                        // check that the atomic can simulate this step.
                        'inner: for rule2 in atomic.rules.iter().filter(|r| r.port == Some(p)) {
                            if let Some(new_atomic_state) = rule2.apply(&atomic_state) {
                                let new_atomic_state = atomic.mask.mask(new_atomic_state);
                                if new_atomic_state != atomic.mask.mask(new_state.clone()) {
                                    continue 'inner;
                                } else {
                                    // match!
                                    atomic_state = new_atomic_state.clone();
                                    trace_atomic
                                        .push_str(&format!(" --{}-> {:?}", p, &new_atomic_state));
                                    continue 'outer;
                                }
                            }
                        }
                        println!("FAILED TO MATCH");
                        break 'outer;
                    }
                }
                continue 'outer; // some progress was made
            }
        }
        println!("STUCK!");
        break;
    }
    println!("{}\n{}", trace, trace_atomic);
}

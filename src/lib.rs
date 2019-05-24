#![feature(specialization)]

#[macro_use]
pub mod helper;
pub mod bitset;
pub mod proto;

// generalizes over port and memory cell "name"
pub type LocId = usize;
pub type RuleId = usize;

// temporarily omitted
// pub mod rbpa;
// pub mod rbpa2;
// pub mod tokens;

#![doc = include_str!("../README.md")]

//!# Details
#![doc = include_str!("../docs/dtmc_details.md")]

//!# References
//! For more details:
//! - [PRISM manual](https://www.prismmodelchecker.org/manual/) describes the PRISM language more formally
//! - [Dave Parker's PhD thesis](https://www.prismmodelchecker.org/papers/davesthesis.pdf) describes the algorithms used in PRISM in more detail.

pub mod analyze;
pub mod ast;
pub mod constr_symbolic;
pub mod dd_manager;
pub mod parser;
pub mod reachability;
pub mod sym_check;
pub mod symbolic_dtmc;

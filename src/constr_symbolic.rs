use crate::analyze::*;
use crate::ast::*;
use lumindd::{Manager, NodeId};
use std::sync::OnceLock;

static MANAGER: OnceLock<Manager> = OnceLock::new();

fn get_manager() -> &'static Manager {
    MANAGER.get_or_init(|| Manager::new())
}

pub struct SymbolicDTMC {
    pub transitions: NodeId,
}

pub fn build_symbolic_dtmc(ast: &DTMCAst, info: &DTMCModelInfo) -> SymbolicDTMC {
    let manager = get_manager();
    SymbolicDTMC {
        transitions: NodeId::ONE,
    }
}

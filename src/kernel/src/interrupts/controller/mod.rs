mod arch;
pub use arch::*;

use crate::task::{Registers, State};

pub type TrapHandler = fn(vector: Vector, state: &mut State, regs: &mut Registers);

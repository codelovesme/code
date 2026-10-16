//! Typed numeric addresses and operations shared by expression and block code.
use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) enum Address {
    Constant(f64),
    Slot(usize, usize),
    Register(usize),
}
impl Address {
    #[inline]
    pub fn read(self, state: &State, registers: &[f64]) -> Option<f64> {
        match self {
            Self::Constant(n) => Some(n),
            Self::Slot(frame, slot) => state.scopes[frame].numbers[slot],
            Self::Register(r) => Some(registers[r]),
        }
    }
}
#[derive(Debug, Clone)]
pub(super) enum Read {
    Slot(usize, usize),
    Index(usize, usize, Address),
}
impl Read {
    // Keep guarded container/boxed reads out of the scalar dispatch loop.
    #[inline(never)]
    pub fn read(&self, state: &State, registers: &[f64]) -> Option<f64> {
        match self {
            Self::Slot(frame, slot) => match state.scopes[*frame].values[*slot].as_ref()? {
                Value::Number(n) => Some(*n),
                _ => None,
            },
            Self::Index(frame, slot, address) => {
                index(state, *frame, *slot, address.read(state, registers)?)
            }
        }
    }
}
/// A guarded read from the existing list representation. A miss takes the
/// original expression path, which owns null/bounds/type and error semantics.
#[inline]
pub(super) fn index(state: &State, frame: usize, slot: usize, index: f64) -> Option<f64> {
    if index < 0.0 || index.fract() != 0.0 {
        return None;
    }
    let Value::Array(items) = state.scopes[frame].values[slot].as_ref()? else {
        return None;
    };
    match items.get(index as usize)? {
        Value::Number(n) => Some(*n),
        _ => None,
    }
}
#[inline]
pub(super) fn arithmetic(op: NumberOp, lhs: f64, rhs: f64) -> Result<f64, String> {
    Ok(match op {
        NumberOp::Add => lhs + rhs,
        NumberOp::Sub => lhs - rhs,
        NumberOp::Mul => lhs * rhs,
        NumberOp::Div => {
            if rhs == 0.0 {
                return Err("division by zero".into());
            }
            lhs / rhs
        }
    })
}
#[inline]
pub(super) fn compare(op: BinOp, lhs: f64, rhs: f64) -> bool {
    match op {
        BinOp::Eq => lhs == rhs,
        BinOp::Ne => lhs != rhs,
        BinOp::Lt => lhs < rhs,
        BinOp::Gt => lhs > rhs,
        BinOp::Le => lhs <= rhs,
        BinOp::Ge => lhs >= rhs,
        _ => unreachable!("numeric comparison"),
    }
}

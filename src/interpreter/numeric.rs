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

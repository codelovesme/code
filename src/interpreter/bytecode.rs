//! Flat control flow and typed register operations. Generic actions use the
//! existing module/trace/ownership helpers; all their expressions are register
//! code too. State borrows end at actions and inbound-delivery boundaries.
use super::*;
use numeric::Address;

#[derive(Debug, Clone)]
struct ScopeInfo {
    names: tree::Names,
    frame: usize,
    locals: std::ops::Range<usize>,
    is_loop: bool,
}
impl ScopeInfo {
    fn of(body: &Block, is_loop: bool) -> Self {
        Self {
            names: Rc::clone(&body.layout.names),
            frame: body.frame,
            locals: body.locals.clone(),
            is_loop,
        }
    }
    fn enter(&self, state: &mut State) {
        if !self.names.is_empty() {
            state.scopes[self.frame].blocks.push(Rc::clone(&self.names));
        }
    }
    fn leave(&self, state: &mut State) {
        let scope = &mut state.scopes[self.frame];
        if !self.names.is_empty() {
            scope.blocks.pop();
        }
        for slot in self.locals.clone() {
            scope.values[slot] = None;
            if let Some(number) = scope.number_slots.get(&slot) {
                scope.numbers[*number] = None;
            }
        }
    }
}
#[derive(Debug, Clone, Copy)]
enum Fallback {
    Action(usize),
    Condition(usize),
}
#[derive(Debug, Clone)]
enum Op {
    Step(usize),
    Condition(usize, usize),
    Enter(usize),
    Leave,
    Exit {
        depth: usize,
        target: usize,
    },
    Escape(Flow),
    Group {
        fallback: Fallback,
        target: usize,
    },
    Arithmetic(usize, NumberOp, Address, Address),
    ArithmeticStore {
        binding: Binding,
        op: NumberOp,
        lhs: Address,
        rhs: Address,
        fallback: usize,
    },
    Neg(usize, Address),
    Store(Binding, Address),
    Compare(BinOp, Address, Address, usize),
}
#[derive(Debug, Clone, Default)]
pub(super) struct Code {
    ops: Vec<Op>,
    scopes: Vec<ScopeInfo>,
    registers: usize,
    /// Small copies of actions/conditions; their child blocks share code via Rc.
    actions: Vec<Stmt>,
    conditions: Vec<Expr>,
}
struct Compiler {
    code: Code,
    depth: usize,
    loops: Vec<Loop>,
}
struct Loop {
    depth: usize,
    head: usize,
    breaks: Vec<usize>,
}
impl Compiler {
    fn address(&mut self, expr: &NumberExpr) -> Address {
        match expr {
            NumberExpr::Literal(n) => Address::Constant(*n),
            NumberExpr::Slot { frame, number } => Address::Slot(*frame, *number),
            NumberExpr::Neg(value) => {
                let src = self.address(value);
                let dst = self.code.registers;
                self.code.registers += 1;
                self.code.ops.push(Op::Neg(dst, src));
                Address::Register(dst)
            }
            NumberExpr::Binary(lhs, op, rhs) => {
                let lhs = self.address(lhs);
                let rhs = self.address(rhs);
                let dst = self.code.registers;
                self.code.registers += 1;
                self.code.ops.push(Op::Arithmetic(dst, *op, lhs, rhs));
                Address::Register(dst)
            }
        }
    }
    fn scope(&mut self, body: &Block, is_loop: bool) {
        let index = self.code.scopes.len();
        self.code.scopes.push(ScopeInfo::of(body, is_loop));
        self.code.ops.push(Op::Enter(index));
        self.depth += 1;
    }
    fn action(&mut self, stmt: &Stmt) -> usize {
        let mut stmt = stmt.clone();
        lower_action(&mut stmt);
        let index = self.code.actions.len();
        self.code.actions.push(stmt);
        index
    }
    fn statements(&mut self, statements: &[Stmt]) {
        for stmt in statements {
            // Registers die at each statement/condition. Reuse their indexes.
            let before = self.code.registers;
            self.code.registers = 0;
            self.statement(stmt);
            self.code.registers = self.code.registers.max(before);
        }
    }
    fn statement(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Assign {
                name,
                value: Expr::Number { fast, .. },
            } => {
                let action = self.action(stmt);
                let group = self.code.ops.len();
                self.code.ops.push(Op::Group {
                    fallback: Fallback::Action(action),
                    target: 0,
                });
                if let NumberExpr::Binary(lhs, op, rhs) = fast {
                    let lhs = self.address(lhs);
                    let rhs = self.address(rhs);
                    // Leaf operands need no separate fallback-group dispatch.
                    // Nested operations keep the group until the final write.
                    if self.code.ops.len() == group + 1 {
                        self.code.ops.pop();
                    }
                    self.code.ops.push(Op::ArithmeticStore {
                        binding: name.clone(),
                        op: *op,
                        lhs,
                        rhs,
                        fallback: action,
                    });
                } else {
                    let value = self.address(fast);
                    self.code.ops.push(Op::Store(name.clone(), value));
                }
                let end = self.code.ops.len();
                if let Op::Group { target, .. } = &mut self.code.ops[group] {
                    *target = end;
                }
            }
            Stmt::If { condition, body } => {
                let branch;
                if let Expr::NumberCompare { lhs, op, rhs, .. } = condition {
                    let mut fallback = condition.clone();
                    lower_expr(&mut fallback);
                    let condition_id = self.code.conditions.len();
                    self.code.conditions.push(fallback);
                    let group = self.code.ops.len();
                    self.code.ops.push(Op::Group {
                        fallback: Fallback::Condition(condition_id),
                        target: 0,
                    });
                    let lhs = self.address(lhs);
                    let rhs = self.address(rhs);
                    branch = self.code.ops.len();
                    self.code.ops.push(Op::Compare(*op, lhs, rhs, 0));
                    if let Op::Group { target, .. } = &mut self.code.ops[group] {
                        *target = branch + 1;
                    }
                } else {
                    let mut condition = condition.clone();
                    lower_expr(&mut condition);
                    let index = self.code.conditions.len();
                    self.code.conditions.push(condition);
                    branch = self.code.ops.len();
                    self.code.ops.push(Op::Condition(index, 0));
                }
                self.scope(body, false);
                self.statements(body);
                self.code.ops.push(Op::Leave);
                self.depth -= 1;
                let end = self.code.ops.len();
                match &mut self.code.ops[branch] {
                    Op::Compare(_, _, _, target) | Op::Condition(_, target) => *target = end,
                    _ => unreachable!(),
                }
            }
            Stmt::Loop { over: None, body } => {
                let head = self.code.ops.len();
                let depth = self.depth;
                self.loops.push(Loop {
                    depth,
                    head,
                    breaks: Vec::new(),
                });
                self.scope(body, true);
                self.statements(body);
                self.depth -= 1;
                self.code.ops.push(Op::Exit {
                    depth,
                    target: head,
                });
                let after = self.code.ops.len();
                for at in self.loops.pop().unwrap().breaks {
                    if let Op::Exit { target, .. } = &mut self.code.ops[at] {
                        *target = after;
                    }
                }
            }
            Stmt::Break | Stmt::Continue => {
                if let Some(loop_) = self.loops.last_mut() {
                    let at = self.code.ops.len();
                    let target = if matches!(stmt, Stmt::Break) {
                        loop_.breaks.push(at);
                        0
                    } else {
                        loop_.head
                    };
                    self.code.ops.push(Op::Exit {
                        depth: loop_.depth,
                        target,
                    });
                } else {
                    self.code
                        .ops
                        .push(Op::Escape(if matches!(stmt, Stmt::Break) {
                            Flow::Break
                        } else {
                            Flow::Continue
                        }));
                }
            }
            _ => {
                let action = self.action(stmt);
                self.code.ops.push(Op::Step(action));
            }
        }
    }
}
impl Code {
    fn lower(body: &Block) -> Self {
        let mut compiler = Compiler {
            code: Self::default(),
            depth: 0,
            loops: Vec::new(),
        };
        compiler.statements(body);
        compiler.code
    }
    fn quiet(state: &State) -> bool {
        !state.active.is_empty()
            || (state.inbound.is_empty()
                && state.runtime_modules.is_empty()
                && state.flows.is_none())
    }
    fn unwind(
        &self,
        stack: &mut Vec<usize>,
        depth: usize,
        env: &Environment,
    ) -> Result<(), String> {
        while stack.len() > depth {
            let scope = &self.scopes[stack.pop().unwrap()];
            scope.leave(&mut env.st_mut());
            if scope.is_loop {
                drain_between_iterations(env)?;
            }
        }
        Ok(())
    }
    pub fn run(&self, env: &Environment) -> Result<Flow, String> {
        let mut registers = vec![0.0; self.registers];
        let mut stack = Vec::new();
        let mut pc = 0;
        let mut group = (Fallback::Action(0), 0);
        let mut quiet = Self::quiet(&env.st());
        enum Stop {
            End,
            Step(usize),
            Condition(usize, usize),
            Fallback(Fallback, usize),
            Exit(usize, usize),
            Escape(Flow),
            Error(String),
        }
        let result = loop {
            // Pure instructions share a borrow. There is no callback here.
            let stop = {
                let mut state = env.st_mut();
                loop {
                    let Some(op) = self.ops.get(pc) else {
                        break Stop::End;
                    };
                    pc += 1;
                    match op {
                        Op::Step(index) => break Stop::Step(*index),
                        Op::Condition(index, target) => break Stop::Condition(*index, *target),
                        Op::Group { fallback, target } => group = (*fallback, *target),
                        Op::Arithmetic(dst, op, lhs, rhs) => {
                            let (Some(lhs), Some(rhs)) =
                                (lhs.read(&state, &registers), rhs.read(&state, &registers))
                            else {
                                break Stop::Fallback(group.0, group.1);
                            };
                            match numeric::arithmetic(*op, lhs, rhs) {
                                Ok(n) => registers[*dst] = n,
                                Err(e) => break Stop::Error(e),
                            }
                        }
                        Op::ArithmeticStore {
                            binding,
                            op,
                            lhs,
                            rhs,
                            fallback,
                        } => {
                            let (Some(lhs), Some(rhs)) =
                                (lhs.read(&state, &registers), rhs.read(&state, &registers))
                            else {
                                break Stop::Fallback(Fallback::Action(*fallback), pc);
                            };
                            let value = match numeric::arithmetic(*op, lhs, rhs) {
                                Ok(value) => value,
                                Err(e) => break Stop::Error(e),
                            };
                            let scope = &mut state.scopes[binding.frame];
                            if let Some(number) = binding.number {
                                scope.numbers[number] = Some(value);
                                scope.values[binding.slot] = None;
                            } else {
                                scope.set(binding.slot, None, Value::Number(value));
                            }
                        }
                        Op::Neg(dst, src) => {
                            let Some(value) = src.read(&state, &registers) else {
                                break Stop::Fallback(group.0, group.1);
                            };
                            registers[*dst] = -value;
                        }
                        Op::Store(binding, source) => {
                            let Some(value) = source.read(&state, &registers) else {
                                break Stop::Fallback(group.0, group.1);
                            };
                            let scope = &mut state.scopes[binding.frame];
                            if let Some(number) = binding.number {
                                scope.numbers[number] = Some(value);
                                scope.values[binding.slot] = None;
                            } else {
                                scope.set(binding.slot, None, Value::Number(value));
                            }
                        }
                        Op::Compare(op, lhs, rhs, target) => {
                            let (Some(lhs), Some(rhs)) =
                                (lhs.read(&state, &registers), rhs.read(&state, &registers))
                            else {
                                break Stop::Fallback(group.0, group.1);
                            };
                            if !numeric::compare(*op, lhs, rhs) {
                                pc = *target;
                            }
                        }
                        Op::Enter(index) => {
                            self.scopes[*index].enter(&mut state);
                            stack.push(*index);
                        }
                        Op::Leave => {
                            self.scopes[stack.pop().unwrap()].leave(&mut state);
                        }
                        Op::Exit { depth, target } => {
                            if quiet {
                                while stack.len() > *depth {
                                    self.scopes[stack.pop().unwrap()].leave(&mut state);
                                }
                                pc = *target;
                            } else {
                                break Stop::Exit(*depth, *target);
                            }
                        }
                        Op::Escape(flow) => break Stop::Escape(flow.clone()),
                    }
                }
            };
            match stop {
                Stop::End => break Ok(Flow::Normal),
                Stop::Error(e) => break Err(e),
                Stop::Escape(flow) => break Ok(flow),
                Stop::Step(index) => match exec(&self.actions[index], env) {
                    Ok(Flow::Normal) => {}
                    result => break result,
                },
                Stop::Condition(index, target) => match boolean(&self.conditions[index], env, "if")
                {
                    Ok(true) => {}
                    Ok(false) => pc = target,
                    Err(e) => break Err(e),
                },
                Stop::Fallback(fallback, target) => {
                    if let Fallback::Condition(condition) = fallback {
                        let Op::Compare(_, _, _, false_target) = self.ops[target - 1] else {
                            unreachable!()
                        };
                        match boolean(&self.conditions[condition], env, "if") {
                            Ok(true) => pc = target,
                            Ok(false) => pc = false_target,
                            Err(e) => break Err(e),
                        }
                    } else if let Fallback::Action(index) = fallback {
                        match exec(&self.actions[index], env) {
                            Ok(Flow::Normal) => pc = target,
                            result => break result,
                        }
                    }
                }
                Stop::Exit(depth, target) => {
                    if let Err(e) = self.unwind(&mut stack, depth, env) {
                        break Err(e);
                    }
                    pc = target;
                }
            }
            quiet = Self::quiet(&env.st());
        };
        // Every structured scope ends on error/return/escape, including loops'
        // delivery points. The caller clears the root scope it entered.
        self.unwind(&mut stack, 0, env)?;
        result
    }
}
fn lower_expr(expr: &mut Expr) {
    *expr = Expr::Code(Box::new(expression::Expression::lower(expr)));
}
fn lower_action(stmt: &mut Stmt) {
    match stmt {
        Stmt::Assign {
            name,
            value: Expr::SetAt { base, path, value },
        } if matches!(&**base, Expr::Ident(read) if read == name) => {
            for key in path {
                lower_expr(key);
            }
            lower_expr(value);
        }
        // Keep the self-add visible to exec, but lower its right operand.
        // It must finish reading the old binding before we take it to append.
        Stmt::Assign {
            name,
            value: Expr::Binary(lhs, BinOp::Add, rhs),
        } if matches!(&**lhs, Expr::Ident(read) if read == name) => lower_expr(rhs),
        Stmt::Assign { value, .. }
        | Stmt::Assert(value)
        | Stmt::Return { value }
        | Stmt::Unlink(value)
        | Stmt::LinkRuntime { path: value, .. } => lower_expr(value),
        Stmt::Emit { particle, .. } => lower_expr(particle),
        Stmt::Loop { over, body } => {
            if let Some(over) = over {
                lower_expr(&mut over.iterable);
            }
            body.statements.clear();
        }
        Stmt::Import { body, .. } => body.statements.clear(),
        Stmt::If { condition, body } => {
            lower_expr(condition);
            body.statements.clear();
        }
        Stmt::HandlerDef { body, .. } => body.statements.clear(),
        Stmt::ImportNative { .. } | Stmt::TypeDef | Stmt::Break | Stmt::Continue => {}
    }
}
fn compile_children(body: &mut Block) {
    for stmt in &mut body.statements {
        match stmt {
            Stmt::Loop { body, .. }
            | Stmt::If { body, .. }
            | Stmt::Import { body, .. }
            | Stmt::HandlerDef { body, .. } => compile(body),
            _ => {}
        }
    }
}
fn compile(body: &mut Block) {
    compile_children(body);
    body.code = Some(Rc::new(Code::lower(body)));
}
fn finish(body: &mut Block) {
    for stmt in &mut body.statements {
        // Preserve imports for handler registration; runtime uses their code.
        match stmt {
            Stmt::Import { body, .. } => finish(body),
            _ => lower_action(stmt),
        }
    }
}
pub(super) fn lower(body: &mut Block) {
    // Top-level statements keep their original source and drain boundaries.
    compile_children(body);
    finish(body);
}

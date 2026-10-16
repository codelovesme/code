//! Register code for a pure expression. Reads borrow the state or constants;
//! values constructed here own their temporaries. Nothing borrows a temporary
//! register, so growing or clearing the register array cannot invalidate a read.
use super::*;

#[derive(Debug, Clone)]
pub(super) struct Expression {
    ops: Vec<Op>,
    constants: Vec<Value>,
    values: usize,
    numbers: usize,
    result: usize,
}
#[derive(Debug, Clone)]
enum Key {
    Literal(String),
    Value(usize),
}
#[derive(Debug, Clone)]
enum Op {
    Constant(usize, usize),
    Read(usize, Binding),
    Field(usize, usize, String),
    Index(usize, usize, usize),
    Array(usize, Vec<usize>),
    Object(usize, Vec<(Key, usize)>),
    Text(usize, Vec<usize>),
    Length(usize, usize),
    Slice(usize, usize, usize, usize),
    Set(usize, usize, Vec<usize>, usize),
    Unary(usize, UnOp, usize),
    Is(usize, usize, IsTest),
    Binary(usize, BinOp, usize, usize),
    Short {
        dst: usize,
        src: usize,
        op: BinOp,
        target: usize,
    },
    Bool(usize, usize, BinOp),
    Number {
        dst: usize,
        source: numeric::Address,
        fallback: usize,
    },
    Arithmetic {
        dst: usize,
        op: NumberOp,
        lhs: numeric::Address,
        rhs: numeric::Address,
        fallback: usize,
    },
    ReadNumber {
        dst: usize,
        source: Box<numeric::Read>,
        fallback: usize,
    },
    Neg(usize, numeric::Address),
    Bound(usize, &'static str),
    Key(usize),
    NumberValue(usize, numeric::Address),
    Compare(usize, BinOp, numeric::Address, numeric::Address, usize),
    Jump(usize),
    Fallback(usize, Box<Expression>),
}
impl Expression {
    pub fn lower(expr: &Expr) -> Self {
        let mut out = Self {
            ops: Vec::new(),
            constants: Vec::new(),
            values: 0,
            numbers: 0,
            result: 0,
        };
        out.result = out.value(expr);
        out
    }
    fn register(&mut self) -> usize {
        let n = self.values;
        self.values += 1;
        n
    }
    fn number_register(&mut self, base: usize) -> usize {
        self.numbers = self.numbers.max(base + 1);
        base
    }
    fn number(
        &mut self,
        expr: &NumberExpr,
        patches: &mut Vec<usize>,
        base: usize,
    ) -> numeric::Address {
        use numeric::Address;
        match expr {
            NumberExpr::Literal(n) => Address::Constant(*n),
            NumberExpr::Slot { frame, number } => {
                // A single read still needs a guard before the result is boxed.
                let dst = self.number_register(base);
                patches.push(self.ops.len());
                self.ops.push(Op::Number {
                    dst,
                    source: Address::Slot(*frame, *number),
                    fallback: 0,
                });
                Address::Register(dst)
            }
            NumberExpr::BoxedSlot { frame, slot } => {
                let dst = self.number_register(base);
                patches.push(self.ops.len());
                self.ops.push(Op::ReadNumber {
                    dst,
                    source: Box::new(numeric::Read::Slot(*frame, *slot)),
                    fallback: 0,
                });
                Address::Register(dst)
            }
            NumberExpr::Index { frame, slot, index } => {
                let index = self.number(index, patches, base);
                let dst = self.number_register(base);
                patches.push(self.ops.len());
                self.ops.push(Op::ReadNumber {
                    dst,
                    source: Box::new(numeric::Read::Index(*frame, *slot, index)),
                    fallback: 0,
                });
                Address::Register(dst)
            }
            NumberExpr::Neg(value) => {
                let rhs = self.number(value, patches, base);
                let dst = self.number_register(base);
                self.ops.push(Op::Neg(dst, rhs));
                Address::Register(dst)
            }
            NumberExpr::Binary(lhs, op, rhs) => {
                // Each subtree's intermediate registers are dead once its
                // result is read. Keep the left result while evaluating the
                // right, then reuse the first register for their answer.
                let lhs = self.number(lhs, patches, base);
                let rhs_base = base + usize::from(matches!(lhs, Address::Register(_)));
                let rhs = self.number(rhs, patches, rhs_base);
                let dst = self.number_register(base);
                patches.push(self.ops.len());
                self.ops.push(Op::Arithmetic {
                    dst,
                    op: *op,
                    lhs,
                    rhs,
                    fallback: 0,
                });
                Address::Register(dst)
            }
        }
    }
    fn patch(&mut self, patches: &[usize], target: usize) {
        for p in patches {
            match &mut self.ops[*p] {
                Op::Number { fallback, .. }
                | Op::Arithmetic { fallback, .. }
                | Op::ReadNumber { fallback, .. } => *fallback = target,
                Op::Neg(..) => {}
                _ => unreachable!(),
            }
        }
    }
    fn value(&mut self, expr: &Expr) -> usize {
        let dst = self.register();
        let op = match expr {
            Expr::Number { fast, fallback } => {
                let mut patches = Vec::new();
                let n = self.number(fast, &mut patches, 0);
                self.ops.push(Op::NumberValue(dst, n));
                let jump = self.ops.len();
                self.ops.push(Op::Jump(0));
                self.patch(&patches, self.ops.len());
                self.value_into(fallback, dst);
                self.ops[jump] = Op::Jump(self.ops.len());
                return dst;
            }
            Expr::NumberCompare {
                lhs,
                op,
                rhs,
                fallback,
            } => {
                let mut patches = Vec::new();
                let lhs = self.number(lhs, &mut patches, 0);
                let rhs_base = usize::from(matches!(lhs, numeric::Address::Register(_)));
                let rhs = self.number(rhs, &mut patches, rhs_base);
                let compare = self.ops.len();
                self.ops.push(Op::Compare(dst, *op, lhs, rhs, 0));
                let jump = self.ops.len();
                self.ops.push(Op::Jump(0));
                let target = self.ops.len();
                self.patch(&patches, target);
                if let Op::Compare(_, _, _, _, fallback) = &mut self.ops[compare] {
                    *fallback = target;
                }
                self.value_into(fallback, dst);
                self.ops[jump] = Op::Jump(self.ops.len());
                return dst;
            }
            Expr::Literal(value) => {
                let index = self.constants.len();
                self.constants.push(value.clone());
                Op::Constant(dst, index)
            }
            Expr::Ident(binding) => Op::Read(dst, binding.clone()),
            Expr::Field(object, field) => Op::Field(dst, self.value(object), field.clone()),
            Expr::Index(object, index) => {
                let object = self.value(object);
                let index = self.value(index);
                Op::Index(dst, object, index)
            }
            Expr::Array(items) => Op::Array(dst, items.iter().map(|v| self.value(v)).collect()),
            Expr::Object(fields) => Op::Object(
                dst,
                fields
                    .iter()
                    .map(|(key, value)| {
                        let key = match key {
                            FieldKey::Literal(k) => Key::Literal(k.clone()),
                            FieldKey::Computed(e) => {
                                let i = self.value(e);
                                self.ops.push(Op::Key(i));
                                Key::Value(i)
                            }
                        };
                        (key, self.value(value))
                    })
                    .collect(),
            ),
            Expr::Interpolated(parts) => {
                Op::Text(dst, parts.iter().map(|v| self.value(v)).collect())
            }
            Expr::LengthOf(value) => Op::Length(dst, self.value(value)),
            Expr::Slice { value, from, to } => {
                let value = self.value(value);
                let from = self.value(from);
                self.ops.push(Op::Bound(from, "start"));
                let to = self.value(to);
                self.ops.push(Op::Bound(to, "end"));
                Op::Slice(dst, value, from, to)
            }
            Expr::SetAt { base, path, value } => {
                let base = self.value(base);
                let path = path.iter().map(|v| self.value(v)).collect();
                let value = self.value(value);
                Op::Set(dst, base, path, value)
            }
            Expr::Unary(op, value) => Op::Unary(dst, *op, self.value(value)),
            Expr::Is(value, test) => Op::Is(dst, self.value(value), test.clone()),
            Expr::Binary(lhs, op @ (BinOp::And | BinOp::Or), rhs) => {
                let lhs = self.value(lhs);
                let branch = self.ops.len();
                self.ops.push(Op::Short {
                    dst,
                    src: lhs,
                    op: *op,
                    target: 0,
                });
                let rhs = self.value(rhs);
                self.ops.push(Op::Bool(dst, rhs, *op));
                let target = self.ops.len();
                if let Op::Short { target: slot, .. } = &mut self.ops[branch] {
                    *slot = target;
                }
                return dst;
            }
            Expr::Binary(lhs, op, rhs) => {
                let lhs = self.value(lhs);
                let rhs = self.value(rhs);
                Op::Binary(dst, *op, lhs, rhs)
            }
            Expr::Code(_) => unreachable!("lowered once"),
        };
        self.ops.push(op);
        dst
    }
    fn value_into(&mut self, expr: &Expr, dst: usize) {
        // Generic guards rarely fail. Keep their registers in a separate
        // program so successful numeric reads do not allocate, initialize or
        // drop a bank of unused boxed temporaries on every evaluation.
        self.ops
            .push(Op::Fallback(dst, Box::new(Self::lower(expr))));
    }
    pub fn run<'a>(&'a self, state: &'a State) -> Result<Cow<'a, Value>, String> {
        let mut small: [Option<Cow<'a, Value>>; 8] = std::array::from_fn(|_| None);
        let mut large = Vec::new();
        let values: &mut [Option<Cow<'a, Value>>] = if self.values <= small.len() {
            &mut small[..self.values]
        } else {
            large.resize_with(self.values, || None);
            &mut large
        };
        let mut small_numbers = [0.0; 8];
        let mut large_numbers = Vec::new();
        let numbers: &mut [f64] = if self.numbers <= small_numbers.len() {
            &mut small_numbers[..self.numbers]
        } else {
            large_numbers.resize(self.numbers, 0.0);
            &mut large_numbers
        };
        fn read<'a, 'b>(i: usize, values: &'b [Option<Cow<'a, Value>>]) -> &'b Value {
            values[i].as_ref().unwrap().as_ref()
        }
        let mut pc = 0;
        while let Some(op) = self.ops.get(pc) {
            pc += 1;
            let (dst, value) = match op {
                Op::Constant(dst, index) => (*dst, Cow::Borrowed(&self.constants[*index])),
                Op::Read(dst, binding) => {
                    let scope = &state.scopes[binding.frame];
                    let value = if let Some(value) = scope.values[binding.slot].as_ref() {
                        Cow::Borrowed(value)
                    } else {
                        Cow::Owned(Value::Number(
                            binding
                                .number
                                .and_then(|i| scope.numbers[i])
                                .ok_or_else(|| format!("undefined variable '{binding}'"))?,
                        ))
                    };
                    (*dst, value)
                }
                Op::Field(dst, object, field) => {
                    let value = match values[*object].as_ref().unwrap() {
                        Cow::Borrowed(v) => field_operand(v, field)?,
                        Cow::Owned(v) => Cow::Owned(field_operand(v, field)?.into_owned()),
                    };
                    (*dst, value)
                }
                Op::Index(dst, object, index) => {
                    let index = read(*index, values);
                    let value = match values[*object].as_ref().unwrap() {
                        Cow::Borrowed(v) => index_operand(v, index)?,
                        Cow::Owned(v) => Cow::Owned(index_operand(v, index)?.into_owned()),
                    };
                    (*dst, value)
                }
                Op::Fallback(dst, expression) => (*dst, expression.run(state)?),
                Op::Array(dst, items) => (
                    *dst,
                    Cow::Owned(Value::Array(Rc::new(
                        items
                            .iter()
                            .map(|i| values[*i].take().unwrap().into_owned())
                            .collect(),
                    ))),
                ),
                Op::Object(dst, fields) => {
                    let mut object = Vec::with_capacity(fields.len());
                    for (key, value) in fields {
                        let key = match key {
                            Key::Literal(k) => k.clone(),
                            Key::Value(i) => match read(*i, values) {
                                Value::Str(s) => s.to_string(),
                                v => {
                                    return Err(format!(
                                        "a field name must be text, found {}",
                                        a_type_name(v)
                                    ))
                                }
                            },
                        };
                        object.push((key, values[*value].take().unwrap().into_owned()));
                    }
                    (*dst, Cow::Owned(Value::Object(Rc::new(object.into()))))
                }
                Op::Text(dst, parts) => {
                    let mut text = String::new();
                    for i in parts {
                        match read(*i, values) {
                            Value::Str(s) => text.push_str(s),
                            v => text.push_str(&v.to_string()),
                        }
                    }
                    (*dst, Cow::Owned(Value::Str(text.into())))
                }
                Op::Length(dst, value) => {
                    let n = match read(*value, values) { Value::Array(v) => v.len(), Value::Object(v) => v.len(), Value::Str(v) => v.chars().count(),
                        v => return Err(format!("cannot take the length of {} — 'length' needs an array, an object or a string", a_type_name(v))) };
                    (*dst, Cow::Owned(Value::Number(n as f64)))
                }
                Op::Slice(dst, value, from, to) => (
                    *dst,
                    Cow::Owned(slice_operand(
                        read(*value, values),
                        range_bound(read(*from, values), "start")?,
                        range_bound(read(*to, values), "end")?,
                    )?),
                ),
                Op::Set(dst, base, keys, value) => {
                    let mut whole = values[*base].take().unwrap().into_owned();
                    let keys: Vec<Value> = keys
                        .iter()
                        .map(|i| values[*i].take().unwrap().into_owned())
                        .collect();
                    set_at(
                        &mut whole,
                        &keys,
                        values[*value].take().unwrap().into_owned(),
                    )?;
                    (*dst, Cow::Owned(whole))
                }
                Op::Unary(dst, op, value) => (
                    *dst,
                    Cow::Owned(match (op, read(*value, values)) {
                        (UnOp::Neg, Value::Number(n)) => Value::Number(-n),
                        (UnOp::Not, Value::Bool(b)) => Value::Bool(!b),
                        (UnOp::Neg, v) => return Err(format!("cannot negate {}", a_type_name(v))),
                        (UnOp::Not, v) => {
                            return Err(format!(
                                "'not' requires a boolean, found {}",
                                a_type_name(v)
                            ))
                        }
                    }),
                ),
                Op::Is(dst, value, test) => {
                    let value = read(*value, values);
                    let answer = match test {
                        IsTest::Kind(kind) => kind_of(value) == *kind,
                        IsTest::Class(class) => state
                            .types
                            .contains(&crate::ast::Label::Named(class.clone()), value),
                        IsTest::Label(label) => state.types.contains(label, value),
                    };
                    (*dst, Cow::Owned(Value::Bool(answer)))
                }
                Op::Binary(dst, op, lhs, rhs) => {
                    let lhs = read(*lhs, values);
                    let rhs = read(*rhs, values);
                    (
                        *dst,
                        Cow::Owned(match op {
                            BinOp::Eq => Value::Bool(lhs == rhs),
                            BinOp::Ne => Value::Bool(lhs != rhs),
                            _ => apply_binop(*op, lhs, rhs)?,
                        }),
                    )
                }
                Op::Short {
                    dst,
                    src,
                    op,
                    target,
                } => match read(*src, values) {
                    Value::Bool(b) => {
                        if *b == (*op == BinOp::Or) {
                            pc = *target;
                            (*dst, Cow::Owned(Value::Bool(*b)))
                        } else {
                            continue;
                        }
                    }
                    v => {
                        return Err(format!(
                            "'{}' requires booleans, found {}",
                            op.symbol(),
                            a_type_name(v)
                        ))
                    }
                },
                Op::Bool(dst, src, op) => (
                    *dst,
                    Cow::Owned(require_bool(read(*src, values), op.symbol())?),
                ),
                Op::Number {
                    dst,
                    source,
                    fallback,
                } => {
                    if let Some(n) = source.read(state, numbers) {
                        numbers[*dst] = n
                    } else {
                        pc = *fallback
                    }
                    continue;
                }
                Op::Arithmetic {
                    dst,
                    op,
                    lhs,
                    rhs,
                    fallback,
                } => {
                    let (Some(lhs), Some(rhs)) =
                        (lhs.read(state, numbers), rhs.read(state, numbers))
                    else {
                        pc = *fallback;
                        continue;
                    };
                    numbers[*dst] = numeric::arithmetic(*op, lhs, rhs)?;
                    continue;
                }
                Op::ReadNumber {
                    dst,
                    source,
                    fallback,
                } => {
                    if let Some(value) = source.read(state, numbers) {
                        numbers[*dst] = value;
                    } else {
                        pc = *fallback;
                    }
                    continue;
                }
                Op::Neg(dst, source) => {
                    numbers[*dst] = -source.read(state, numbers).unwrap();
                    continue;
                }
                Op::Bound(src, kind) => {
                    range_bound(read(*src, values), kind)?;
                    continue;
                }
                Op::Key(src) => {
                    if !matches!(read(*src, values), Value::Str(_)) {
                        return Err(format!(
                            "a field name must be text, found {}",
                            a_type_name(read(*src, values))
                        ));
                    }
                    continue;
                }
                Op::NumberValue(dst, source) => (
                    *dst,
                    Cow::Owned(Value::Number(source.read(state, numbers).unwrap())),
                ),
                Op::Compare(dst, op, lhs, rhs, fallback) => {
                    let (Some(lhs), Some(rhs)) =
                        (lhs.read(state, numbers), rhs.read(state, numbers))
                    else {
                        pc = *fallback;
                        continue;
                    };
                    (
                        *dst,
                        Cow::Owned(Value::Bool(numeric::compare(*op, lhs, rhs))),
                    )
                }
                Op::Jump(target) => {
                    pc = *target;
                    continue;
                }
            };
            values[dst] = Some(value);
        }
        Ok(values[self.result].take().unwrap())
    }
}

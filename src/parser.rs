use chumsky::prelude::*;

use crate::ast::{
    BinaryOp, ConstraintExpr, DomainKind, Expression,
    HandlerTarget, ObjectField, Program, Spanned, Statement, StringPart, TypeExpr, UnaryOp,
};

/// Whitespace and comment skipper (does NOT consume newlines).
fn whitespace() -> impl Parser<char, (), Error = Simple<char>> + Clone {
    let line_comment = just("->")
        .then(filter(|c: &char| *c != '\n' && *c != '\r').repeated())
        .ignored();

    filter(|c: &char| *c == ' ' || *c == '\t')
        .ignored()
        .or(line_comment)
        .repeated()
        .ignored()
}

/// Parse a number literal (f64).
fn number() -> impl Parser<char, Expression, Error = Simple<char>> + Clone {
    let digits = text::digits(10);

    digits
        .then(just('.').then(text::digits(10)).or_not())
        .map(|(int_part, frac): (String, Option<(char, String)>)| {
            let s = match frac {
                Some((dot, frac_part)) => format!("{}{}{}", int_part, dot, frac_part),
                None => int_part,
            };
            Expression::Number(s.parse::<f64>().unwrap())
        })
}

/// Parse a string literal (double-quoted), with optional `$name` interpolation.
fn string_literal() -> impl Parser<char, Expression, Error = Simple<char>> + Clone {
    let dollar_var = just('$')
        .ignore_then(text::ident())
        .map(StringPart::Variable);

    let literal_char = just('\\')
        .ignore_then(any())
        .or(filter(|c: &char| *c != '"' && *c != '$'));

    let literal_segment = literal_char
        .repeated()
        .at_least(1)
        .collect::<String>()
        .map(StringPart::Literal);

    let parts = dollar_var.or(literal_segment).repeated();

    just('"')
        .ignore_then(parts)
        .then_ignore(just('"'))
        .map(|parts| {
            if parts.len() == 1 {
                if let StringPart::Literal(s) = &parts[0] {
                    return Expression::String(s.clone());
                }
            }
            if parts.is_empty() {
                return Expression::String(String::new());
            }
            let all_literal = parts.iter().all(|p| matches!(p, StringPart::Literal(_)));
            if all_literal {
                let mut s = String::new();
                for p in &parts {
                    if let StringPart::Literal(lit) = p {
                        s.push_str(lit);
                    }
                }
                return Expression::String(s);
            }
            Expression::InterpolatedString(parts)
        })
}

/// Parse an identifier (excludes reserved keywords).
fn identifier() -> impl Parser<char, String, Error = Simple<char>> + Clone {
    text::ident().try_map(|s: String, span| {
        match s.as_str() {
            "if" | "loop" | "over" | "break" | "assert" | "link" | "as" | "private"
            | "is" | "not" | "this" | "base" | "true" | "false" | "return"
            | "in" | "with" | "yield" | "fold" | "by" | "and" | "or" | "get"
            | "emit" | "to" => {
                Err(Simple::custom(span, format!("'{}' is a reserved keyword", s)))
            }
            _ => Ok(s),
        }
    })
}

/// Parse a class name (starts with uppercase letter).
fn class_name() -> impl Parser<char, String, Error = Simple<char>> + Clone {
    filter(|c: &char| c.is_ascii_uppercase())
        .then(filter(|c: &char| c.is_ascii_alphanumeric() || *c == '_').repeated())
        .map(|(first, rest): (char, Vec<char>)| {
            let mut s = String::new();
            s.push(first);
            s.extend(rest);
            s
        })
}

/// Parse a module reference for `link`.
fn module_ref() -> impl Parser<char, String, Error = Simple<char>> + Clone {
    filter(|c: &char| {
        c.is_ascii_alphanumeric() || *c == '_' || *c == '-' || *c == '/' || *c == '.'
    })
    .repeated()
    .at_least(1)
    .collect::<String>()
}

/// Parse an expression with access to a statement parser (for function literal bodies).
fn build_expression(
    _stmt: impl Parser<char, Spanned<Statement>, Error = Simple<char>> + Clone + 'static,
) -> impl Parser<char, Expression, Error = Simple<char>> + Clone {
    recursive(move |expr| {
        // Object field: `name = expr` (resolved) or `name ∈ expr` / `name <
        // expr` / etc. (T26 Phase 2 — constrained, makes the enclosing
        // literal a Value::Schema instead of a resolved Object) or computed
        // `[expr] = expr`. Reuses the same constraint_rhs already used for
        // top-level `variable <op> expr` statements — a field constraint
        // *is* a constraint statement, just scoped to a field name instead
        // of the environment.
        let object_field = identifier()
            .then_ignore(whitespace())
            .then(constraint_rhs(expr.clone()))
            .map(|(name, constraint)| match constraint {
                ConstraintExpr::Equals(value) => ObjectField::Static(name, value),
                other => ObjectField::Constrained(name, other),
            })
            .or(
                just('[')
                    .ignore_then(any_whitespace())
                    .ignore_then(expr.clone())
                    .then_ignore(any_whitespace())
                    .then_ignore(just(']'))
                    .then_ignore(whitespace())
                    .then_ignore(just('='))
                    .then_ignore(whitespace())
                    .then(expr.clone())
                    .map(|(key, value)| ObjectField::Computed(key, value))
            );

        // Spread: `...expr`
        let spread = just('.')
            .ignore_then(just('.'))
            .ignore_then(just('.'))
            .ignore_then(whitespace())
            .ignore_then(expr.clone())
            .map(|e| Some(Box::new(e)));

        // Fields block with optional leading spread
        let spread_and_fields = {
            let field_sep = any_whitespace()
                .ignore_then(just(','))
                .ignore_then(any_whitespace());

            let spread_then_fields = spread
                .clone()
                .then_ignore(field_sep.clone())
                .then(
                    object_field
                        .clone()
                        .separated_by(field_sep.clone())
                        .allow_trailing(),
                );

            let spread_only = spread.clone().map(|s| (s, Vec::new()));

            let fields_only = object_field
                .clone()
                .separated_by(field_sep)
                .allow_trailing()
                .map(|fields| (None, fields));

            spread_then_fields.or(spread_only).or(fields_only)
        };

        let object_literal = just('{')
            .ignore_then(any_whitespace())
            .ignore_then(spread_and_fields.clone())
            .then_ignore(any_whitespace())
            .then_ignore(just('}'))
            .map(|(spread, fields)| Expression::Object { spread, fields });

        // Qualified particle: `module.ClassName { ... }` or bare `ClassName { ... }`
        let particle_fields = just('{')
            .ignore_then(any_whitespace())
            .ignore_then(spread_and_fields)
            .then_ignore(any_whitespace())
            .then_ignore(just('}'));

        let qualified_particle = identifier()
            .then_ignore(just('.'))
            .then(class_name())
            .then_ignore(whitespace())
            .then(particle_fields.clone())
            .map(|((qualifier, cname), (spread, fields))| Expression::Particle {
                qualifier: Some(qualifier),
                class_name: cname,
                spread,
                fields,
            });

        let bare_particle = class_name()
            .then_ignore(whitespace())
            .then(particle_fields)
            .map(|(cname, (spread, fields))| Expression::Particle {
                qualifier: None,
                class_name: cname,
                spread,
                fields,
            });

        // Null literal
        let null_literal = text::keyword("Null").to(Expression::Null);

        // Array literal
        let array_literal = just('[')
            .ignore_then(any_whitespace())
            .ignore_then(
                expr.clone()
                    .separated_by(
                        any_whitespace()
                            .ignore_then(just(','))
                            .ignore_then(any_whitespace()),
                    )
                    .allow_trailing(),
            )
            .then_ignore(any_whitespace())
            .then_ignore(just(']'))
            .map(Expression::ArrayLiteral);

        // Set literal: `{ expr, expr, ... }` — bare elements, at least one.
        // An empty `{}` is an empty object (Decision 2, T26), never an empty
        // set, so this requires `.at_least(1)` — that's also what lets `{}`
        // keep parsing as `object_literal` unambiguously. Object fields
        // (`name = expr`) are never valid bare expressions on their own (a
        // standalone `name = expr` is a Statement, not an Expression), so a
        // set literal and an object literal can never both match the same
        // input — `object_literal` is tried first and either succeeds or
        // fails outright, then `set_literal` is tried from the same start.
        let set_literal = just('{')
            .ignore_then(any_whitespace())
            .ignore_then(
                expr.clone()
                    .separated_by(
                        any_whitespace()
                            .ignore_then(just(','))
                            .ignore_then(any_whitespace()),
                    )
                    .at_least(1)
                    .allow_trailing(),
            )
            .then_ignore(any_whitespace())
            .then_ignore(just('}'))
            .map(Expression::SetLiteral);

        // Boolean literals
        let bool_literal = text::keyword("true")
            .to(Expression::Boolean(true))
            .or(text::keyword("false").to(Expression::Boolean(false)));

        // Parenthesized grouping
        let grouped = just('(')
            .ignore_then(any_whitespace())
            .ignore_then(expr.clone())
            .then_ignore(any_whitespace())
            .then_ignore(just(')'))
            .labelled("grouped expression");

        // Atom
        let atom = number()
            .or(string_literal())
            .or(bool_literal)
            .or(null_literal)
            .or(qualified_particle)
            .or(bare_particle)
            .or(array_literal)
            .or(grouped)
            .or(object_literal)
            .or(set_literal)
            .or(identifier().map(Expression::Identifier))
            .labelled("expression");

        // Postfix: `.field`, `[index]`
        // No `(args)` call syntax — Code has no function-call concept; reusable
        // logic is expressed only as handlers (`emit X to target get result`).
        let postfix = atom
            .then(
                just('.')
                    .ignore_then(identifier())
                    .map(PostfixOp::Property)
                    .or(just('[')
                        .ignore_then(any_whitespace())
                        .ignore_then(expr.clone())
                        .then_ignore(any_whitespace())
                        .then_ignore(just(']'))
                        .map(PostfixOp::Index))
                    .repeated(),
            )
            .foldl(|expr, op| match op {
                PostfixOp::Property(field) => Expression::PropertyAccess(Box::new(expr), field),
                PostfixOp::Index(index) => Expression::IndexAccess {
                    receiver: Box::new(expr),
                    index: Box::new(index),
                },
            });

        // Unary: `not expr`, `-expr` (arithmetic negation, e.g. `-5`)
        let unary_op = text::keyword("not")
            .to(UnaryOp::Not)
            .or(just('-').to(UnaryOp::Negate));
        let unary = unary_op
            .then_ignore(whitespace())
            .repeated()
            .then(postfix.clone())
            .foldr(|op, operand| Expression::Unary {
                op,
                operand: Box::new(operand),
            });

        // Multiplicative: `*`, `/`, and set intersection `∩` (T26 — binds at
        // the same tier as `*`/`/` rather than as its own precedence layer,
        // which matters: this parser is already deeply nested (many
        // precedence tiers, all built from one recursive `expr`, reused
        // dozens of times for object fields, particle fields, conditions,
        // etc.), and adding brand-new wrapping tiers for `∩`/`∪` blew the
        // 16MB parser stack on *any* input, even trivial ones — folding the
        // new operators into existing tiers as extra alternatives avoids
        // that entirely (verified: same 16MB stack, no overflow). `Value::Set`
        // operands only, checked at eval time.
        let multiplicative = unary
            .clone()
            .then(
                whitespace()
                    .ignore_then(
                        just('*').to(BinaryOp::Mul)
                            .or(just('/').to(BinaryOp::Div))
                            .or(just('∩').to(BinaryOp::Intersect)),
                    )
                    .then_ignore(whitespace())
                    .then(unary.clone())
                    .repeated(),
            )
            .foldl(|left, (op, right)| Expression::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            });

        // Additive: `+`, `-`, and set union `∪` (T26 — same tier as `+`/`-`,
        // folded in for the same reason as `∩` above).
        let additive = multiplicative
            .clone()
            .then(
                whitespace()
                    .ignore_then(
                        just('+').to(BinaryOp::Add)
                            .or(just('-').to(BinaryOp::Sub))
                            .or(just('∪').to(BinaryOp::Union)),
                    )
                    .then_ignore(whitespace())
                    .then(multiplicative.clone())
                    .repeated(),
            )
            .foldl(|left, (op, right)| Expression::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            });

        // Relational: `≤`, `≥`, `<`, `>`
        let relational = additive
            .clone()
            .then(
                whitespace()
                    .ignore_then(
                        just('≤')
                            .to(BinaryOp::LessEqual)
                            .or(just('≥').to(BinaryOp::GreaterEqual))
                            .or(just('<').to(BinaryOp::Less))
                            .or(just('>').to(BinaryOp::Greater)),
                    )
                    .then_ignore(whitespace())
                    .then(additive.clone())
                    .or_not(),
            )
            .map(|(left, suffix)| match suffix {
                Some((op, right)) => Expression::Binary {
                    left: Box::new(left),
                    op,
                    right: Box::new(right),
                },
                None => left,
            });

        // Membership: `∈`/`∉` — the right side is tried as a type-shaped
        // name/literal first (`x ∈ Number`, `x ∈ "Success"`), and only
        // falls back to a bare identifier (`x ∈ c`) when that fails — a
        // lowercase name, for instance, can never be `type_expr_parser()`-
        // shaped (T27 follow-up: a variable holding a Set/Schema/Union is
        // exactly as valid a right side as a type name, since T26 already
        // treats types as sets). Deliberately NOT `expression.clone()` here
        // — `membership` sits inside `expr`, which is reused dozens of
        // times through the grammar; embedding the full recursive
        // expression parser a second time at this depth stack-overflows
        // (see the T26 Phase 1 postmortem for the same class of bug). A
        // bare identifier covers the actual need (`1 ∈ c`); anything more
        // exotic (`1 ∈ (a ∪ b)`) needs a named intermediate.
        enum MembershipRhs {
            Type(TypeExpr),
            Expr(Expression),
        }
        let membership = relational
            .clone()
            .then(
                whitespace()
                    .ignore_then(
                        just('∈').to(false)
                            .or(just('∉').to(true)),
                    )
                    .then_ignore(whitespace())
                    .then(
                        type_expr_parser()
                            .map(MembershipRhs::Type)
                            .or(identifier().map(Expression::Identifier).map(MembershipRhs::Expr)),
                    )
                    .or_not(),
            )
            .map(|(left, suffix)| match suffix {
                Some((negated, MembershipRhs::Type(type_expr))) => Expression::TypeCheck {
                    expr: Box::new(left),
                    type_expr,
                    negated,
                },
                Some((negated, MembershipRhs::Expr(container))) => Expression::MemberOf {
                    expr: Box::new(left),
                    container: Box::new(container),
                    negated,
                },
                None => left,
            });

        // Equality: `=`, `≠` — pure value equality
        let equality = membership
            .clone()
            .then(
                whitespace()
                    .ignore_then(
                        just('=')
                            .to(BinaryOp::Equal)
                            .or(just('≠').to(BinaryOp::NotEqual)),
                    )
                    .then_ignore(whitespace())
                    .then(membership.clone())
                    .or_not(),
            )
            .map(|(left, suffix)| match suffix {
                Some((op, rhs)) => Expression::Binary {
                    left: Box::new(left),
                    op,
                    right: Box::new(rhs),
                },
                None => left,
            });

        // Logical AND: `and`
        let logical_and = equality
            .clone()
            .then(
                whitespace()
                    .ignore_then(text::keyword("and"))
                    .then_ignore(whitespace())
                    .then(equality.clone())
                    .repeated(),
            )
            .foldl(|left, (_op, right)| Expression::Binary {
                left: Box::new(left),
                op: BinaryOp::And,
                right: Box::new(right),
            });

        // Logical OR: `or`
        logical_and
            .clone()
            .then(
                whitespace()
                    .ignore_then(text::keyword("or"))
                    .then_ignore(whitespace())
                    .then(logical_and.clone())
                    .repeated(),
            )
            .foldl(|left, (_op, right)| Expression::Binary {
                left: Box::new(left),
                op: BinaryOp::Or,
                right: Box::new(right),
            })
    })
}

/// Helper enum for postfix parsing.
#[derive(Debug, Clone)]
enum PostfixOp {
    Property(String),
    Index(Expression),
}

/// Line separator: one or more newlines.
fn line_sep() -> impl Parser<char, (), Error = Simple<char>> + Clone {
    whitespace()
        .then(text::newline())
        .then(whitespace())
        .ignored()
        .repeated()
        .at_least(1)
        .ignored()
}

/// Whitespace that may include newlines (used inside blocks).
fn any_whitespace() -> impl Parser<char, (), Error = Simple<char>> + Clone {
    filter(|c: &char| c.is_whitespace())
        .ignored()
        .or(just("->")
            .then(filter(|c: &char| *c != '\n' && *c != '\r').repeated())
            .ignored())
        .repeated()
        .ignored()
}

/// Parse a type name (starts with uppercase).
fn type_name() -> impl Parser<char, String, Error = Simple<char>> + Clone {
    filter(|c: &char| c.is_ascii_uppercase())
        .then(filter(|c: &char| c.is_ascii_alphanumeric() || *c == '_').repeated())
        .map(|(first, rest): (char, Vec<char>)| {
            let mut s = String::new();
            s.push(first);
            s.extend(rest);
            s
        })
}

/// Parse a string literal used as a type: `"Code"` → Literal("Code").
fn type_string_literal() -> impl Parser<char, String, Error = Simple<char>> + Clone {
    just('"')
        .ignore_then(filter(|c: &char| *c != '"').repeated().collect::<String>())
        .then_ignore(just('"'))
}

/// Parse a full type expression: `Named`, `"Literal"`, `A ∪ B`, or `A ∩ B`.
/// Precedence: `∩` binds tighter than `∪`.
fn type_expr_parser() -> impl Parser<char, TypeExpr, Error = Simple<char>> + Clone {
    let type_atom = type_name()
        .map(TypeExpr::Named)
        .or(type_string_literal().map(TypeExpr::Literal));

    // Intersection: `A ∩ B ∩ C` — binds tighter
    let intersection = type_atom
        .clone()
        .then(
            whitespace()
                .ignore_then(just('∩'))
                .ignore_then(whitespace())
                .ignore_then(type_atom)
                .repeated(),
        )
        .map(|(first, rest)| {
            if rest.is_empty() {
                first
            } else {
                let mut all = vec![first];
                all.extend(rest);
                TypeExpr::Intersection(all)
            }
        });

    // Union: `A ∪ B ∪ C` — binds looser
    intersection
        .clone()
        .then(
            whitespace()
                .ignore_then(just('∪'))
                .ignore_then(whitespace())
                .ignore_then(intersection)
                .repeated(),
        )
        .map(|(first, rest)| {
            if rest.is_empty() {
                first
            } else {
                let mut all = vec![first];
                all.extend(rest);
                TypeExpr::Union(all)
            }
        })
}

/// Parse the right-hand side of a constraint statement.
/// Handles: `= expr`, `≠ expr`, `< expr`, `> expr`, `≤ expr`, `≥ expr`,
/// `in Z|R|N`, `in expr`.
fn constraint_rhs(
    expression: impl Parser<char, Expression, Error = Simple<char>> + Clone,
) -> impl Parser<char, ConstraintExpr, Error = Simple<char>> + Clone {
    // Domain keywords: Z, R, N
    let domain_keyword = text::ident().try_map(|s: String, span| match s.as_str() {
        "Z" => Ok(ConstraintExpr::Domain(DomainKind::Integer)),
        "R" => Ok(ConstraintExpr::Domain(DomainKind::Real)),
        "N" => Ok(ConstraintExpr::Domain(DomainKind::Natural)),
        _ => Err(Simple::custom(span, "Expected domain Z, R, or N")),
    });

    // Type membership: `∈ TypeExpr` → IsType constraint. `∈ Z|R|N` is the
    // numeric-domain form (same as `in Z|R|N`), tried first so it doesn't
    // fall through to IsType against a nonexistent "Z"/"R"/"N" particle type.
    // Falls back to MemberOf (T26) for anything that isn't a bare
    // capitalized type name — a set literal, a lowercase variable holding a
    // set, a member/index expression, etc. `type_name()` requires an
    // uppercase first letter, so this never conflicts with a real type name:
    // `∈ ABC` is always IsType, `∈ ml` or `∈ {1,2}` always falls through here.
    just('∈')
        .ignore_then(whitespace())
        .ignore_then(
            domain_keyword
                .clone()
                .or(type_expr_parser().map(ConstraintExpr::IsType))
                .or(expression.clone().map(ConstraintExpr::MemberOf)),
        )
    .or(
        just('=')
            .ignore_then(whitespace())
            .ignore_then(expression.clone())
            .map(ConstraintExpr::Equals)
    )
        .or(just('≠')
            .ignore_then(whitespace())
            .ignore_then(expression.clone())
            .map(ConstraintExpr::NotEquals))
        .or(just('≤')
            .ignore_then(whitespace())
            .ignore_then(expression.clone())
            .map(ConstraintExpr::LessEqual))
        .or(just('≥')
            .ignore_then(whitespace())
            .ignore_then(expression.clone())
            .map(ConstraintExpr::GreaterEqual))
        .or(just('<')
            .ignore_then(whitespace())
            .ignore_then(expression.clone())
            .map(ConstraintExpr::LessThan))
        .or(just('>')
            .ignore_then(whitespace())
            .ignore_then(expression.clone())
            .map(ConstraintExpr::GreaterThan))
        .or(text::keyword("in")
            .ignore_then(whitespace())
            .ignore_then(
                domain_keyword.or(expression.clone().map(ConstraintExpr::MemberOf)),
            ))
}

/// Parse a single statement, tagged with its source span.
fn statement() -> impl Parser<char, Spanned<Statement>, Error = Simple<char>> + Clone {
    recursive(|stmt| {
        let expression = build_expression(stmt.clone());

        // Constraint statement: `ident == expr`, `ident < expr`, etc.
        let constraint_stmt = identifier()
            .then_ignore(whitespace())
            .then(constraint_rhs(expression.clone()))
            .map(|(variable, constraint)| Statement::Constraint {
                variable,
                constraint,
                private: false,
            });

        let assert_stmt = text::keyword("assert")
            .ignore_then(whitespace())
            .ignore_then(expression.clone())
            .map(Statement::Assert);

        // Handler return: `return expr`
        let handler_return = text::keyword("return")
            .ignore_then(whitespace())
            .ignore_then(expression.clone())
            .map(|value| Statement::HandlerReturn { value });

        // Break statement
        let break_stmt = text::keyword("break").to(Statement::Break);

        // Yield statement: `yield expr`
        let yield_stmt = text::keyword("yield")
            .ignore_then(whitespace())
            .ignore_then(expression.clone())
            .map(Statement::Yield);

        // If statement
        let if_stmt = text::keyword("if")
            .ignore_then(whitespace())
            .ignore_then(expression.clone())
            .then_ignore(any_whitespace())
            .then(
                just('{')
                    .ignore_then(any_whitespace())
                    .ignore_then(stmt.clone().separated_by(line_sep()).allow_trailing())
                    .then_ignore(any_whitespace())
                    .then_ignore(just('}')),
            )
            .map(|(condition, body)| Statement::If { condition, body });

        // Loop over
        let loop_stmt = text::keyword("loop")
            .ignore_then(whitespace())
            .ignore_then(identifier())
            .then(
                just(',')
                    .ignore_then(whitespace())
                    .ignore_then(identifier())
                    .or_not(),
            )
            .then_ignore(whitespace())
            .then_ignore(text::keyword("over"))
            .then_ignore(whitespace())
            .then(expression.clone())
            .then_ignore(whitespace())
            .then(
                text::keyword("get")
                    .ignore_then(whitespace())
                    .ignore_then(identifier())
                    .or_not(),
            )
            .then_ignore(any_whitespace())
            .then(
                just('{')
                    .ignore_then(any_whitespace())
                    .ignore_then(stmt.clone().separated_by(line_sep()).allow_trailing())
                    .then_ignore(any_whitespace())
                    .then_ignore(just('}')),
            )
            .map(|((((variable, index), iterable), result), body)| Statement::LoopOver {
                variable,
                index,
                iterable,
                result,
                body,
            });

        // Loop over a variable's own domain (T26): `loop <var> [get <result>] { ... }`
        // — no `over`. Tried after loop_stmt (which requires `over`) so a
        // stray `over` keyword still routes there; this form is what's left
        // when `loop <ident>` is directly followed by `get`/`{` instead.
        let loop_domain_stmt = text::keyword("loop")
            .ignore_then(whitespace())
            .ignore_then(identifier())
            .then_ignore(whitespace())
            .then(
                text::keyword("get")
                    .ignore_then(whitespace())
                    .ignore_then(identifier())
                    .or_not(),
            )
            .then_ignore(any_whitespace())
            .then(
                just('{')
                    .ignore_then(any_whitespace())
                    .ignore_then(stmt.clone().separated_by(line_sep()).allow_trailing())
                    .then_ignore(any_whitespace())
                    .then_ignore(just('}')),
            )
            .map(|((variable, result), body)| Statement::LoopDomain {
                variable,
                result,
                body,
            });

        // Infinite loop
        let loop_infinite_stmt = text::keyword("loop")
            .ignore_then(whitespace())
            .then(
                text::keyword("get")
                    .ignore_then(whitespace())
                    .ignore_then(identifier())
                    .or_not(),
            )
            .then_ignore(any_whitespace())
            .then(
                just('{')
                    .ignore_then(any_whitespace())
                    .ignore_then(stmt.clone().separated_by(line_sep()).allow_trailing())
                    .then_ignore(any_whitespace())
                    .then_ignore(just('}')),
            )
            .map(|((_, result), body)| Statement::LoopInfinite { result, body });

        let block = just('{')
            .ignore_then(any_whitespace())
            .ignore_then(stmt.clone().separated_by(line_sep()).allow_trailing())
            .then_ignore(any_whitespace())
            .then_ignore(just('}'))
            .map(Statement::Block);

        // Handler body
        let handler_body = just('{')
            .ignore_then(any_whitespace())
            .ignore_then(stmt.clone().separated_by(line_sep()).allow_trailing())
            .then_ignore(any_whitespace())
            .then_ignore(just('}'));

        // Handler definition: `ClassName => { body }`
        let bare_handler_def = class_name()
            .then_ignore(whitespace())
            .then_ignore(just("=>"))
            .then_ignore(any_whitespace())
            .then(handler_body)
            .map(|(cname, body)| Statement::HandlerDefinition {
                class_name: cname,
                body,
            });

        // Handler target
        let handler_target = text::keyword("this")
            .to(HandlerTarget::This)
            .or(text::keyword("base").to(HandlerTarget::Base))
            .or(text::keyword("core").to(HandlerTarget::Core))
            .or(identifier().map(HandlerTarget::ModuleAlias));

        // Emit statement (fire-and-forget or with result):
        //   emit expr to target           → HandlerInvoke
        //   emit expr to target get ident  → HandlerInvokeAssign
        let emit_stmt = text::keyword("emit")
            .ignore_then(whitespace())
            .ignore_then(expression.clone())
            .then_ignore(whitespace())
            .then_ignore(text::keyword("to"))
            .then_ignore(whitespace())
            .then(handler_target)
            .then(
                whitespace()
                    .ignore_then(text::keyword("get"))
                    .ignore_then(whitespace())
                    .ignore_then(identifier())
                    .or_not(),
            )
            .map(|((particle, target), result_name)| match result_name {
                Some(name) => Statement::HandlerInvokeAssign {
                    particle,
                    target,
                    result_name: name,
                },
                None => Statement::HandlerInvoke { particle, target },
            });

        let link_stmt = text::keyword("link")
            .ignore_then(whitespace())
            .ignore_then(module_ref())
            .then(
                whitespace()
                    .ignore_then(text::keyword("as"))
                    .ignore_then(whitespace())
                    .ignore_then(identifier())
                    .or_not(),
            )
            .map(|(module_ref, alias)| Statement::Link { module_ref, alias });

        // Private constraint: `private ident == expr`, `private ident < expr`, etc.
        let private_constraint = text::keyword("private")
            .ignore_then(whitespace())
            .ignore_then(identifier())
            .then_ignore(whitespace())
            .then(constraint_rhs(expression.clone()))
            .map(|(variable, constraint)| Statement::Constraint {
                variable,
                constraint,
                private: true,
            });

        assert_stmt
            .or(handler_return)
            .or(link_stmt)
            .or(private_constraint)
            .or(yield_stmt)
            .or(break_stmt)
            .or(if_stmt)
            .or(loop_infinite_stmt)
            .or(loop_stmt)
            .or(loop_domain_stmt)
            .or(bare_handler_def)
            .or(emit_stmt)
            .or(constraint_stmt)
            .or(block)
            .labelled("statement")
            .map_with_span(|node, span| Spanned::new(node, span))
    })
}

/// Parse a full program.
pub fn parser() -> impl Parser<char, Program, Error = Simple<char>> {
    let blank_lines = whitespace()
        .then(text::newline())
        .ignored()
        .repeated()
        .ignored();

    let skip_line = filter(|c: &char| *c != '\n' && *c != '\r')
        .repeated()
        .at_least(1)
        .collect::<String>()
        .validate(|text, span, emit| {
            emit(Simple::custom(
                span,
                format!(
                    "Unexpected: {}",
                    text.chars().take(40).collect::<String>()
                ),
            ));
            None
        });

    let stmt_or_skip = statement().map(Some).or(skip_line);

    blank_lines
        .ignore_then(stmt_or_skip.separated_by(line_sep()).allow_trailing())
        .then_ignore(whitespace())
        .then_ignore(end().labelled("end of input"))
        .map(|statements| Program {
            statements: statements.into_iter().flatten().collect(),
        })
}

/// Structured parse error with byte offset span.
#[derive(Debug)]
pub struct ParseError {
    pub message: String,
    pub start: usize,
    pub end: usize,
}

/// Parse source code and return the program (if successful) and any errors.
pub fn parse_source(source: &str) -> (Option<Program>, Vec<ParseError>) {
    let (result, errors) = parser().parse_recovery(source);
    let parse_errors = errors
        .into_iter()
        .map(|e| {
            let span = e.span();
            ParseError {
                message: format!("{}", e),
                start: span.start,
                end: span.end,
            }
        })
        .collect();
    (result, parse_errors)
}

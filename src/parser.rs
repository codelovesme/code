use chumsky::prelude::*;

use crate::ast::{
    BinaryOp, ConstraintExpr, DomainKind, Expression, FieldConstraint,
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
            "if" | "loop" | "over" | "break" | "assert" | "link" | "as" | "type" | "private"
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
        // Object field: `name = expr` or computed `[expr] = expr`
        let object_field = identifier()
            .then_ignore(whitespace())
            .then_ignore(just('='))
            .then_ignore(whitespace())
            .then(expr.clone())
            .map(|(name, value)| ObjectField::Static(name, value))
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
            .or(identifier().map(Expression::Identifier))
            .labelled("expression");

        // Postfix: `.field`, `[index]`, `(args)`
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
                    .or(just('(')
                        .ignore_then(any_whitespace())
                        .ignore_then(
                            expr.clone()
                                .separated_by(
                                    any_whitespace().ignore_then(just(',')).ignore_then(any_whitespace()),
                                )
                                .allow_trailing(),
                        )
                        .then_ignore(any_whitespace())
                        .then_ignore(just(')'))
                        .map(PostfixOp::Call))
                    .repeated(),
            )
            .foldl(|expr, op| match op {
                PostfixOp::Property(field) => Expression::PropertyAccess(Box::new(expr), field),
                PostfixOp::Index(index) => Expression::IndexAccess {
                    receiver: Box::new(expr),
                    index: Box::new(index),
                },
                PostfixOp::Call(args) => Expression::Call {
                    callee: Box::new(expr),
                    args,
                },
            });

        // Unary: `not expr`
        let unary = text::keyword("not")
            .ignore_then(whitespace())
            .repeated()
            .then(postfix.clone())
            .foldr(|_op, operand| Expression::Unary {
                op: UnaryOp::Not,
                operand: Box::new(operand),
            });

        // Multiplicative: `*`, `/`
        let multiplicative = unary
            .clone()
            .then(
                whitespace()
                    .ignore_then(just('*').to(BinaryOp::Mul).or(just('/').to(BinaryOp::Div)))
                    .then_ignore(whitespace())
                    .then(unary.clone())
                    .repeated(),
            )
            .foldl(|left, (op, right)| Expression::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            });

        // Additive: `+`, `-`
        let additive = multiplicative
            .clone()
            .then(
                whitespace()
                    .ignore_then(just('+').to(BinaryOp::Add).or(just('-').to(BinaryOp::Sub)))
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

        // Membership: `∈ Type`, `∉ Type` — type membership check
        let membership = relational
            .clone()
            .then(
                whitespace()
                    .ignore_then(
                        just('∈').to(false)
                            .or(just('∉').to(true)),
                    )
                    .then_ignore(whitespace())
                    .then(type_expr_parser())
                    .or_not(),
            )
            .map(|(left, suffix)| match suffix {
                Some((negated, type_expr)) => Expression::TypeCheck {
                    expr: Box::new(left),
                    type_expr,
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
    Call(Vec<Expression>),
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

    // Type membership: `∈ TypeExpr` → IsType constraint
    just('∈')
        .ignore_then(whitespace())
        .ignore_then(type_expr_parser())
        .map(ConstraintExpr::IsType)
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

        // Type field in particle definitions: `name ∈ Type` or `name ∈ Type | Null`
        let type_field = identifier()
            .then_ignore(whitespace())
            .then_ignore(just('∈'))
            .then_ignore(whitespace())
            .then(type_expr_parser())
            .map(|(name, tn)| {
                let optional = match &tn {
                    TypeExpr::Union(parts) => parts.iter().any(|t| matches!(t, TypeExpr::Named(n) if n == "Null")),
                    TypeExpr::Named(n) => n == "Null",
                    _ => false,
                };
                FieldConstraint {
                    name,
                    constraints: vec![ConstraintExpr::IsType(tn)],
                    optional,
                }
            });

        // Type declaration: `type Name { field: Type, field?: Type }`
        let type_decl = text::keyword("type")
            .ignore_then(whitespace())
            .ignore_then(class_name())
            .then_ignore(whitespace())
            .then(
                just('{')
                    .ignore_then(any_whitespace())
                    .ignore_then(
                        type_field
                            .clone()
                            .separated_by(
                                any_whitespace()
                                    .ignore_then(just(','))
                                    .ignore_then(any_whitespace()),
                            )
                            .allow_trailing(),
                    )
                    .then_ignore(any_whitespace())
                    .then_ignore(just('}')),
            )
            .map(|(name, fields)| Statement::TypeDeclaration { name, fields });

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

        // Combined handler definition: `ClassName{field ∈ Type,...} => { body }`
        let handler_type_field = identifier()
            .then_ignore(whitespace())
            .then_ignore(just('∈'))
            .then_ignore(whitespace())
            .then(type_expr_parser())
            .map(|(name, tn)| {
                let optional = match &tn {
                    TypeExpr::Union(parts) => parts.iter().any(|t| matches!(t, TypeExpr::Named(n) if n == "Null")),
                    TypeExpr::Named(n) => n == "Null",
                    _ => false,
                };
                FieldConstraint {
                    name,
                    constraints: vec![ConstraintExpr::IsType(tn)],
                    optional,
                }
            });

        let combined_handler_def = class_name()
            .then_ignore(whitespace())
            .then(
                just('{')
                    .ignore_then(any_whitespace())
                    .ignore_then(
                        handler_type_field
                            .separated_by(
                                any_whitespace()
                                    .ignore_then(just(','))
                                    .ignore_then(any_whitespace()),
                            )
                            .allow_trailing(),
                    )
                    .then_ignore(any_whitespace())
                    .then_ignore(just('}')),
            )
            .then_ignore(whitespace())
            .then_ignore(just("=>"))
            .then_ignore(any_whitespace())
            .then(handler_body.clone())
            .map(|((cname, fields), body)| Statement::HandlerDefinition {
                class_name: cname,
                inline_type: Some(fields),
                body,
            });

        // Bare handler definition: `ClassName => { body }`
        let bare_handler_def = class_name()
            .then_ignore(whitespace())
            .then_ignore(just("=>"))
            .then_ignore(any_whitespace())
            .then(handler_body)
            .map(|(cname, body)| Statement::HandlerDefinition {
                class_name: cname,
                inline_type: None,
                body,
            });

        // Handler target
        let handler_target = text::keyword("this")
            .to(HandlerTarget::This)
            .or(text::keyword("base").to(HandlerTarget::Base))
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
            .or(type_decl)
            .or(link_stmt)
            .or(private_constraint)
            .or(yield_stmt)
            .or(break_stmt)
            .or(if_stmt)
            .or(loop_infinite_stmt)
            .or(loop_stmt)
            .or(combined_handler_def)
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

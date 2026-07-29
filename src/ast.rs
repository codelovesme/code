use crate::native_module::{EmitQueue, NativeHandlerInfo};

/// Structured type expression: named type, string literal type, union, or intersection.
/// Used in `is` constraints, type checks, and function return types.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeExpr {
    /// A named type: `String`, `Number`, `MyParticle`, etc.
    Named(String),
    /// A string literal type: `"Code"`, `"Info"` — matches only that exact string value.
    Literal(String),
    /// A union of types: `String ∪ Number`, `"Code" ∪ "Loves" ∪ "Me"`.
    Union(Vec<TypeExpr>),
    /// An intersection of types: `Number ∩ Positive`.
    Intersection(Vec<TypeExpr>),
}

impl std::fmt::Display for TypeExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TypeExpr::Named(n) => write!(f, "{}", n),
            TypeExpr::Literal(s) => write!(f, "\"{}\"", s),
            TypeExpr::Union(types) => {
                let parts: Vec<String> = types.iter().map(|t| t.to_string()).collect();
                write!(f, "{}", parts.join(" ∪ "))
            }
            TypeExpr::Intersection(types) => {
                let parts: Vec<String> = types.iter().map(|t| t.to_string()).collect();
                write!(f, "{}", parts.join(" ∩ "))
            }
        }
    }
}

/// Domain kind for numeric constraints.
#[derive(Debug, Clone, PartialEq)]
pub enum DomainKind {
    /// Integer domain (Z)
    Integer,
    /// Real number domain (R)
    Real,
    /// Natural number domain (N)
    Natural,
}

impl std::fmt::Display for DomainKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DomainKind::Integer => write!(f, "Z"),
            DomainKind::Real => write!(f, "R"),
            DomainKind::Natural => write!(f, "N"),
        }
    }
}

/// A constraint expression applied to a variable.
#[derive(Debug, Clone)]
pub enum ConstraintExpr {
    /// `a = expr` — variable must equal this value.
    Equals(Expression),
    /// `a ≠ expr` — variable must not equal this value.
    NotEquals(Expression),
    /// `a < expr` — variable must be less than this value.
    LessThan(Expression),
    /// `a > expr` — variable must be greater than this value.
    GreaterThan(Expression),
    /// `a ≤ expr` — variable must be less than or equal.
    LessEqual(Expression),
    /// `a ≥ expr` — variable must be greater than or equal.
    GreaterEqual(Expression),
    /// `a in expr` — variable must be a member of the set/array.
    MemberOf(Expression),
    /// `a in Z` / `a in R` / `a in N` — variable is in this numeric domain.
    Domain(DomainKind),
    /// `a is TypeExpr` — variable must match this type.
    IsType(TypeExpr),
}

impl std::fmt::Display for ConstraintExpr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConstraintExpr::Equals(_) => write!(f, "= <expr>"),
            ConstraintExpr::NotEquals(_) => write!(f, "≠ <expr>"),
            ConstraintExpr::LessThan(_) => write!(f, "< <expr>"),
            ConstraintExpr::GreaterThan(_) => write!(f, "> <expr>"),
            ConstraintExpr::LessEqual(_) => write!(f, "≤ <expr>"),
            ConstraintExpr::GreaterEqual(_) => write!(f, "≥ <expr>"),
            ConstraintExpr::MemberOf(_) => write!(f, "in <expr>"),
            ConstraintExpr::Domain(d) => write!(f, "in {}", d),
            ConstraintExpr::IsType(t) => write!(f, "is {}", t),
        }
    }
}

/// A field constraint in a particle type definition.
#[derive(Debug, Clone)]
pub struct FieldConstraint {
    pub name: String,
    /// Constraints on this field (typically IsType for type annotations).
    pub constraints: Vec<ConstraintExpr>,
    /// Whether this field is optional (declared with `?`).
    pub optional: bool,
}

impl FieldConstraint {
    /// Extract the primary type expression from constraints.
    /// Returns the TypeExpr from the first IsType constraint, or "Any" if none.
    pub fn primary_type(&self) -> TypeExpr {
        self.constraints
            .iter()
            .find_map(|c| match c {
                ConstraintExpr::IsType(te) => Some(te.clone()),
                _ => None,
            })
            .unwrap_or(TypeExpr::Named("Any".to_string()))
    }

    /// Convert to the legacy tuple format (name, TypeExpr, optional).
    pub fn to_type_tuple(&self) -> (String, TypeExpr, bool) {
        (self.name.clone(), self.primary_type(), self.optional)
    }
}

/// A source span as a char-index range (matches the parser's `Simple<char>`
/// spans and the char-based renderer in `crate::diagnostics`).
pub type Span = std::ops::Range<usize>;

/// A syntax node paired with its source span. Used to give runtime errors a
/// `file:line:col` location without threading spans into every node type.
#[derive(Debug, Clone)]
pub struct Spanned<T> {
    pub node: T,
    pub span: Span,
}

impl<T> Spanned<T> {
    pub fn new(node: T, span: Span) -> Self {
        Spanned { node, span }
    }
}

/// The top-level program: a sequence of statements.
#[derive(Debug)]
pub struct Program {
    pub statements: Vec<Spanned<Statement>>,
}

/// A single statement in the Code language.
#[derive(Debug, Clone)]
pub enum Statement {
    /// `link <module_ref> [as <alias>]` — parsed from source.
    Link {
        module_ref: String,
        alias: Option<String>,
    },
    /// Constraint on a variable: `a == 12`, `a < 5`, `a is Number`, etc.
    /// `private` prefix makes the binding module-private.
    Constraint {
        variable: String,
        constraint: ConstraintExpr,
        private: bool,
    },
    /// `type Name { field: Type, ... }` — declares a particle type with field constraints.
    TypeDeclaration {
        name: String,
        fields: Vec<FieldConstraint>,
    },
    Assert(Expression),
    Block(Vec<Spanned<Statement>>),
    /// `if <expr> { ... }` — conditional block (condition must be Boolean).
    If {
        condition: Expression,
        body: Vec<Spanned<Statement>>,
    },
    /// `loop <var> over <expr> [get <result>] { ... }` — iterate over an array.
    LoopOver {
        variable: String,
        index: Option<String>,
        iterable: Expression,
        result: Option<String>,
        body: Vec<Spanned<Statement>>,
    },
    /// `yield <expr>` — yield a value to the enclosing loop with `get`.
    Yield(Expression),
    /// `loop [get <result>] { ... }` — infinite loop (use `break` to exit).
    LoopInfinite {
        result: Option<String>,
        body: Vec<Spanned<Statement>>,
    },
    /// `break` — exit the nearest enclosing loop.
    Break,
    /// Handler definition: `ClassName{field:Type,...} => { body }` (combined)
    /// or `ClassName => { body }` (split/bare).
    HandlerDefinition {
        class_name: String,
        /// If this is a combined definition, the inline field constraints.
        inline_type: Option<Vec<FieldConstraint>>,
        body: Vec<Spanned<Statement>>,
    },
    /// Handler invocation: `emit expr to this`, `emit expr to base`, or `emit expr to moduleAlias`.
    HandlerInvoke {
        particle: Expression,
        target: HandlerTarget,
    },
    /// Handler invocation with result: `emit expr to target get result_name`.
    HandlerInvokeAssign {
        particle: Expression,
        target: HandlerTarget,
        result_name: String,
    },
    /// Handler return: `return expr` — returns a value from a handler.
    HandlerReturn {
        value: Expression,
    },
    /// Internal: produced by the module loader after resolving a link.
    /// `alias` is `Some(name)` for `link ... as name` (namespace import)
    /// and `None` for bare `link` (flatten into current scope).
    Import {
        alias: Option<String>,
        body: Vec<Spanned<Statement>>,
        public_names: Vec<String>,
        public_types: Vec<TypeInfo>,
        public_handlers: Vec<HandlerInfo>,
    },
    /// Internal: produced by the module loader for linked native modules (.so/.wasm).
    NativeImport {
        alias: Option<String>,
        /// Absolute filesystem path to the .so / .wasm file.
        native_path: String,
        /// True if this is a WASM module (.wasm); false for shared-library (.so).
        is_wasm: bool,
        /// Exported variable names and their values.
        vars: Vec<(String, std::rc::Rc<crate::runtime::Value>)>,
        /// Exported handlers.
        handlers: Vec<NativeHandlerInfo>,
        /// Exported type declarations.
        types: Vec<TypeInfo>,
        /// Emission declarations.
        emissions: Vec<EmissionDecl>,
        /// Thread-safe queue for receiving emitted particles.
        emit_queue: EmitQueue,
    },
}

/// Information about a type declaration (used in Import nodes).
#[derive(Debug, Clone)]
pub struct TypeInfo {
    pub name: String,
    pub fields: Vec<FieldConstraint>,
}

/// Information about a handler definition (used in Import nodes).
#[derive(Debug, Clone)]
pub struct HandlerInfo {
    pub class_name: String,
    pub body: Vec<Spanned<Statement>>,
}

/// Emission declaration from a native module.
#[derive(Debug, Clone)]
pub struct EmissionDecl {
    pub class_name: String,
    /// Target: "base" (dispatched to linking module's handlers).
    pub target: String,
}

/// Target for handler invocation.
#[derive(Debug, Clone)]
pub enum HandlerTarget {
    /// Dispatch to the current module's handler.
    This,
    /// Dispatch to handler(s) in the base module(s) that linked the current module.
    Base,
    /// Dispatch to a handler in the given module alias.
    ModuleAlias(String),
}

/// A field in an object or particle literal.
#[derive(Debug, Clone)]
pub enum ObjectField {
    /// Static field: `name = expr`
    Static(String, Expression),
    /// Computed field: `[key_expr] = expr`
    Computed(Expression, Expression),
}

/// An expression node.
#[derive(Debug, Clone)]
pub enum Expression {
    Identifier(String),
    Number(f64),
    String(String),
    /// Boolean literal: `true` or `false`.
    Boolean(bool),
    /// Null literal.
    Null,
    /// Object literal: `{ field=val, ... }` or spread `{ ...source, field=val }`
    Object {
        spread: Option<Box<Expression>>,
        fields: Vec<ObjectField>,
    },
    /// Particle constructor: `ClassName { field=val, ... }` or `module.ClassName { ... }`
    Particle {
        qualifier: Option<String>,
        class_name: String,
        spread: Option<Box<Expression>>,
        fields: Vec<ObjectField>,
    },
    PropertyAccess(Box<Expression>, String),
    /// Array literal: `[expr, expr, ...]`
    ArrayLiteral(Vec<Expression>),
    /// Array/string index access: `expr[expr]`
    IndexAccess {
        receiver: Box<Expression>,
        index: Box<Expression>,
    },
    /// Function call: `callee(args)`.
    Call {
        callee: Box<Expression>,
        args: Vec<Expression>,
    },
    /// String interpolation: `"text $var more text"`.
    InterpolatedString(Vec<StringPart>),
    /// Unary expression: `!expr`.
    Unary {
        op: UnaryOp,
        operand: Box<Expression>,
    },
    Binary {
        left: Box<Expression>,
        op: BinaryOp,
        right: Box<Expression>,
    },
    /// Type check: `expr == TypeExpr` or `expr != TypeExpr`.
    TypeCheck {
        expr: Box<Expression>,
        type_expr: TypeExpr,
        negated: bool,
    },

}

/// Part of an interpolated string.
#[derive(Debug, Clone)]
pub enum StringPart {
    Literal(String),
    Variable(String),
}

/// Binary operators.
#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOp {
    Equal,
    NotEqual,
    Add,
    Sub,
    Mul,
    Div,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
    And,
    Or,
}

/// Unary operators.
#[derive(Debug, Clone)]
pub enum UnaryOp {
    Not,
}

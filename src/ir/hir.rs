//! High-level Intermediate Representation (HIR)
//!
//! HIR is a desugared AST with name resolution complete.

use crate::ast::*;
use crate::ir::*;

/// HIR Program - desugared AST with resolved names
#[derive(Debug, Clone)]
pub struct HIRProgram {
    pub declarations: Vec<HIRDeclaration>,
    pub debug_info: DebugInfo,
}

/// HIR Declaration
#[derive(Debug, Clone)]
pub enum HIRDeclaration {
    Function(HIRFunction),
    Class(HIRClass),
    Variable(HIRVariable),
}

/// HIR Function
#[derive(Debug, Clone)]
pub struct HIRFunction {
    pub id: IRId,
    pub name: String,
    pub parameters: Vec<HIRParameter>,
    pub return_type: HIRType,
    pub body: Vec<HIRStatement>,
    pub is_async: bool,
    pub debug_info: DebugInfo,
}

/// HIR Parameter
#[derive(Debug, Clone)]
pub struct HIRParameter {
    pub name: String,
    pub param_type: HIRType,
    pub default_value: Option<HIRExpression>,
}

/// HIR Class
#[derive(Debug, Clone)]
pub struct HIRClass {
    pub id: IRId,
    pub name: String,
    pub parent: Option<String>,
    pub fields: Vec<HIRField>,
    pub methods: Vec<HIRFunction>,
    pub debug_info: DebugInfo,
}

/// HIR Field
#[derive(Debug, Clone)]
pub struct HIRField {
    pub name: String,
    pub field_type: HIRType,
    pub visibility: Visibility,
}

/// HIR Variable
#[derive(Debug, Clone)]
pub struct HIRVariable {
    pub name: String,
    pub var_type: HIRType,
    pub initializer: Option<HIRExpression>,
    pub is_mutable: bool,
}

/// HIR Statement
#[derive(Debug, Clone)]
pub enum HIRStatement {
    Expression(HIRExpression),
    Assignment(HIRAssignment),
    If(HIRIf),
    While(HIRWhile),
    For(HIRFor),
    Return(Option<HIRExpression>),
    Block(Vec<HIRStatement>),
}

/// HIR Expression
#[derive(Debug, Clone)]
pub enum HIRExpression {
    Literal(HIRLiteral),
    Variable(String),
    Call(HIRCall),
    Binary(HIRBinary),
    Unary(HIRUnary),
    Member(HIRMember),
    Index(HIRIndex),
    Lambda(HIRLambda),
}

/// HIR Literal
#[derive(Debug, Clone)]
pub enum HIRLiteral {
    Integer(i64),
    Number(f64),
    String(String),
    Boolean(bool),
    List(Vec<HIRExpression>),
    Matrix(Vec<Vec<HIRExpression>>),
}

/// HIR Function Call
#[derive(Debug, Clone)]
pub struct HIRCall {
    pub function: Box<HIRExpression>,
    pub arguments: Vec<HIRExpression>,
}

/// HIR Binary Operation
#[derive(Debug, Clone)]
pub struct HIRBinary {
    pub left: Box<HIRExpression>,
    pub operator: BinaryOperator,
    pub right: Box<HIRExpression>,
}

/// HIR Unary Operation
#[derive(Debug, Clone)]
pub struct HIRUnary {
    pub operator: UnaryOperator,
    pub operand: Box<HIRExpression>,
}

/// HIR Member Access
#[derive(Debug, Clone)]
pub struct HIRMember {
    pub object: Box<HIRExpression>,
    pub member: String,
}

/// HIR Index Access
#[derive(Debug, Clone)]
pub struct HIRIndex {
    pub object: Box<HIRExpression>,
    pub index: Box<HIRExpression>,
}

/// HIR Lambda Expression
#[derive(Debug, Clone)]
pub struct HIRLambda {
    pub parameters: Vec<HIRParameter>,
    pub body: Vec<HIRStatement>,
    pub return_type: HIRType,
}

/// HIR Assignment
#[derive(Debug, Clone)]
pub struct HIRAssignment {
    pub target: HIRLValue,
    pub value: HIRExpression,
}

/// HIR Left-hand Value
#[derive(Debug, Clone)]
pub enum HIRLValue {
    Variable(String),
    Member(HIRMember),
    Index(HIRIndex),
}

/// HIR If Statement
#[derive(Debug, Clone)]
pub struct HIRIf {
    pub condition: HIRExpression,
    pub then_branch: Vec<HIRStatement>,
    pub else_branch: Option<Vec<HIRStatement>>,
}

/// HIR While Loop
#[derive(Debug, Clone)]
pub struct HIRWhile {
    pub condition: HIRExpression,
    pub body: Vec<HIRStatement>,
}

/// HIR For Loop
#[derive(Debug, Clone)]
pub struct HIRFor {
    pub variable: String,
    pub iterable: HIRExpression,
    pub body: Vec<HIRStatement>,
}

/// HIR Type System
#[derive(Debug, Clone, PartialEq)]
pub enum HIRType {
    Integer(Option<u8>), // precision bits
    Number(Option<u8>),  // precision bits
    String,
    Boolean,
    List(Box<HIRType>),
    Matrix(Box<HIRType>),
    Function(Vec<HIRType>, Box<HIRType>),
    Class(String),
    Generic(String),
    Any,
}

/// Visibility levels
#[derive(Debug, Clone, PartialEq)]
pub enum Visibility {
    Public,
    Private,
}

/// Binary operators
#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOperator {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Equal,
    NotEqual,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
    And,
    Or,
}

/// Unary operators
#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOperator {
    Plus,
    Minus,
    Not,
}

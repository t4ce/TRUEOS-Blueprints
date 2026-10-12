//! One-shot HTML document and fragment parsing, translated from parse5.
//! Input is decoded Unicode; byte encoding detection belongs to the caller.
extern crate alloc;

pub mod dom;
mod entities;
mod entities_data;
mod parser;
mod parser_data;
pub mod token;
mod tokenizer;

pub use dom::{Document, DocumentMode, Namespace, Node, NodeId, NodeKind};
pub use dom::{ElementPropertyChange, ElementPropertyError, ElementPropertyObserver};
pub use parser::{parse, parse_fragment};
pub use token::Attribute;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub code: String,
    /// Normalized character position; currently zero for tree-recovery errors.
    pub offset: usize,
}

#[derive(Clone, Debug)]
pub struct FragmentContext {
    pub name: String,
    pub namespace: Namespace,
    pub attrs: Vec<Attribute>,
}

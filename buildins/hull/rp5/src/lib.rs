//! One-shot HTML document and fragment parsing, translated from parse5.
//! Input is decoded Unicode; byte encoding detection belongs to the caller.
pub mod dom;
pub mod token;
mod entities;
mod entities_data;
mod parser_data;
mod parser;
mod tokenizer;

pub use dom::{Document, DocumentMode, Namespace, Node, NodeId, NodeKind};
pub use token::Attribute;
pub use parser::{parse, parse_fragment};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub code: String,
    pub offset: usize,
}

#[derive(Clone, Debug)]
pub struct FragmentContext {
    pub name: String,
    pub namespace: Namespace,
    pub attrs: Vec<Attribute>,
}

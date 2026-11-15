//! Indexed KDL 2.0 document tree, navigation API, selector engine, and serializer.

mod document;
mod error;
mod selector;
mod serializer;
mod span;
mod value;

pub use document::{KdlDocument, KdlNode, NodeId, NodeMatcher, NodeRef};
pub use error::{KdlError, ParseDiagnostic, ParseError};
pub use selector::{
    AttributeFilter, Combinator, ComplexSelector, CompoundSelector, FilterKey, FilterOp,
    NodeSelector, NthExpr, PseudoClass, RelativeSelector, Selector, SelectorError, SelectorList,
    TypeMatcher,
};
pub use serializer::{
    arg_to_kdl, doc_node_to_kdl, doc_to_kdl, escape_quoted, format_ident, format_ty, is_bare_ident,
    is_disallowed_ident_char, normalize_ty_input, prop_to_kdl, standalone_node_to_kdl,
    value_to_kdl,
};
pub use span::{LineIndex, Span};
pub use value::{KdlArg, KdlEntry, KdlProp, KdlValue};

/// Parses a KDL 2.0 source string into an indexed [`KdlDocument`].
pub fn parse(source: &str) -> Result<KdlDocument, KdlError> {
    KdlDocument::parse(source)
}

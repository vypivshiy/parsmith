use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::{Deref, Index, IndexMut};

use serde::{Deserialize, Serialize};

use crate::kdl::error::KdlError;
use crate::kdl::selector::{
    select_doc, select_doc_first, select_doc_first_id, select_doc_ids, select_subtree,
    select_subtree_first, select_subtree_first_id, select_subtree_ids, try_node_closest,
    try_node_matches, try_standalone_matches,
};
use crate::kdl::serializer::{
    doc_node_to_kdl, doc_to_kdl, normalize_ty_input, standalone_node_to_kdl,
};
use crate::kdl::span::{LineIndex, Span};
use crate::kdl::value::{KdlArg, KdlProp, KdlValue};

/// Strongly-typed arena index handle for a [`KdlNode`] inside a [`KdlDocument`].
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
pub struct NodeId(pub usize);

impl NodeId {
    /// Returns the 0-based arena index of this node.
    #[inline]
    pub const fn index(self) -> usize {
        self.0
    }
}

impl From<usize> for NodeId {
    #[inline]
    fn from(idx: usize) -> Self {
        Self(idx)
    }
}

impl From<NodeId> for usize {
    #[inline]
    fn from(id: NodeId) -> Self {
        id.0
    }
}

/// An indexed KDL 2.0 node stored inside a [`KdlDocument`] arena (or constructed standalone).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdlNode {
    /// Arena identifier of this node inside its owning [`KdlDocument`].
    pub id: NodeId,
    /// Parent node identifier (`None` for root-level nodes).
    pub parent: Option<NodeId>,
    /// Root-level ancestor identifier (`self.id` for root-level nodes).
    pub root: NodeId,
    /// Depth in the tree (`0` for root-level nodes).
    pub depth: usize,
    /// 0-based position of this node among its siblings.
    pub index_in_parent: usize,
    /// Decoded node name identifier.
    pub name: String,
    /// Source span of the node name identifier.
    pub name_span: Span,
    /// Optional type annotation identifier (without parentheses, e.g. `"item"`).
    pub ty: Option<String>,
    /// Source span of the `(type)` annotation, if present.
    pub type_span: Option<Span>,
    /// Positional arguments in source order.
    pub args: Vec<KdlArg>,
    /// Named properties in source order.
    pub props: Vec<KdlProp>,
    /// Child node identifiers in source order.
    pub children: Vec<NodeId>,
    /// Whether an explicit `{ ... }` block was present in source.
    pub has_children_block: bool,
    /// Source span of the children block contents, if present.
    pub children_block_span: Option<Span>,
    /// When `true` and `children` is empty, `to_kdl()` emits a multi-line `{\n}` block.
    pub empty_children_multiline: bool,
    /// Optional trailing single-line comment (without leading `//`) emitted by `to_kdl()`.
    pub trailing_comment: Option<String>,
    /// Full source span of the node (from `(type)` or `name` through the end of the node).
    pub span: Span,
}

impl KdlNode {
    /// Creates a new synthetic `KdlNode` with the given name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            id: NodeId(0),
            parent: None,
            root: NodeId(0),
            depth: 0,
            index_in_parent: 0,
            name: name.into(),
            name_span: Span::EMPTY,
            ty: None,
            type_span: None,
            args: Vec::new(),
            props: Vec::new(),
            children: Vec::new(),
            has_children_block: false,
            children_block_span: None,
            empty_children_multiline: false,
            trailing_comment: None,
            span: Span::EMPTY,
        }
    }

    /// Builder helper: sets the node's type annotation.
    pub fn with_ty(mut self, ty: impl AsRef<str>) -> Self {
        self.ty = Some(normalize_ty_input(ty.as_ref()));
        self
    }

    /// Builder helper: appends a positional argument.
    pub fn with_arg(mut self, arg: impl Into<KdlArg>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Builder helper: appends a type-annotated positional argument.
    pub fn with_typed_arg(mut self, ty: impl AsRef<str>, value: impl Into<KdlValue>) -> Self {
        self.args.push(KdlArg::typed(ty, value));
        self
    }

    /// Builder helper: sets or replaces a named property.
    pub fn with_prop(mut self, key: impl Into<String>, value: impl Into<KdlValue>) -> Self {
        self.set_prop(key, value);
        self
    }

    /// Builder helper: sets or replaces a type-annotated named property.
    pub fn with_typed_prop(
        mut self,
        key: impl Into<String>,
        ty: impl AsRef<str>,
        value: impl Into<KdlValue>,
    ) -> Self {
        self.set_typed_prop(key, ty, value);
        self
    }

    /// Builder helper: sets a trailing comment emitted after the node header or `{`.
    pub fn with_trailing_comment(mut self, comment: impl Into<String>) -> Self {
        self.trailing_comment = Some(comment.into());
        self
    }

    /// Builder helper: configures whether an empty children block renders as `{\n}`.
    pub fn with_empty_children_multiline(mut self, multiline: bool) -> Self {
        self.empty_children_multiline = multiline;
        if multiline {
            self.has_children_block = true;
        }
        self
    }

    /// Returns the arena [`NodeId`] of this node.
    #[inline]
    pub fn id(&self) -> NodeId {
        self.id
    }

    /// Returns the parent [`NodeId`] of this node, or `None` if it is a root-level node.
    #[inline]
    pub fn parent_id(&self) -> Option<NodeId> {
        self.parent
    }

    /// Returns the root ancestor [`NodeId`] of this node.
    #[inline]
    pub fn root_id(&self) -> NodeId {
        self.root
    }

    /// Returns the depth of this node in the tree (`0` for root-level nodes).
    #[inline]
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Returns the 0-based index of this node among its siblings.
    #[inline]
    pub fn index(&self) -> usize {
        self.index_in_parent
    }

    /// Returns the node's name.
    #[inline]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Sets the node's name.
    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    /// Returns the source span of the node's name identifier.
    #[inline]
    pub fn name_span(&self) -> Span {
        self.name_span
    }

    /// Returns the node's type annotation identifier (without parentheses), if present.
    #[inline]
    pub fn ty(&self) -> Option<&str> {
        self.ty.as_deref()
    }

    /// Alias for [`KdlNode::ty`].
    #[inline]
    pub fn type_annotation(&self) -> Option<&str> {
        self.ty()
    }

    /// Sets or clears the node's type annotation.
    pub fn set_ty(&mut self, ty: Option<impl AsRef<str>>) {
        self.ty = ty.map(|t| normalize_ty_input(t.as_ref()));
    }

    /// Returns the source span of the node's `(type)` annotation, if present.
    #[inline]
    pub fn type_span(&self) -> Option<Span> {
        self.type_span
    }

    /// Returns the full source span of the node.
    #[inline]
    pub fn span(&self) -> Span {
        self.span
    }

    /// Returns the slice of positional arguments.
    #[inline]
    pub fn args(&self) -> &[KdlArg] {
        &self.args
    }

    /// Returns a positional argument by 0-based index.
    #[inline]
    pub fn arg(&self, index: usize) -> Option<&KdlArg> {
        self.args.get(index)
    }

    /// Returns the [`KdlValue`] of a positional argument by 0-based index.
    #[inline]
    pub fn arg_value(&self, index: usize) -> Option<&KdlValue> {
        self.args.get(index).map(|a| &a.value)
    }

    /// Appends a positional argument to this node.
    pub fn add_arg(&mut self, arg: impl Into<KdlArg>) {
        self.args.push(arg.into());
    }

    /// Appends a type-annotated positional argument to this node.
    pub fn add_typed_arg(&mut self, ty: impl AsRef<str>, value: impl Into<KdlValue>) {
        self.args.push(KdlArg::typed(ty, value));
    }

    /// Returns the slice of named properties in source order.
    #[inline]
    pub fn props(&self) -> &[KdlProp] {
        &self.props
    }

    /// Looks up a named property by key (returning the last matching property per KDL 2.0 spec).
    #[inline]
    pub fn prop(&self, key: &str) -> Option<&KdlProp> {
        self.props.iter().rev().find(|p| p.key == key)
    }

    /// Looks up a named property's [`KdlValue`] by key.
    #[inline]
    pub fn get(&self, key: &str) -> Option<&KdlValue> {
        self.prop(key).map(|p| &p.value)
    }

    /// Returns `true` if this node has a named property with `key`.
    #[inline]
    pub fn has_prop(&self, key: &str) -> bool {
        self.props.iter().any(|p| p.key == key)
    }

    /// Sets or replaces a named property on this node.
    pub fn set_prop(&mut self, key: impl Into<String>, value: impl Into<KdlValue>) {
        let key = key.into();
        let value = value.into();
        if let Some(existing) = self.props.iter_mut().rev().find(|p| p.key == key) {
            existing.value = value;
            existing.ty = None;
        } else {
            self.props.push(KdlProp::new(key, value));
        }
    }

    /// Sets or replaces a type-annotated named property on this node.
    pub fn set_typed_prop(
        &mut self,
        key: impl Into<String>,
        ty: impl AsRef<str>,
        value: impl Into<KdlValue>,
    ) {
        let key = key.into();
        let ty_norm = normalize_ty_input(ty.as_ref());
        let value = value.into();
        if let Some(existing) = self.props.iter_mut().rev().find(|p| p.key == key) {
            existing.value = value;
            existing.ty = Some(ty_norm);
        } else {
            self.props.push(KdlProp::typed(key, ty_norm, value));
        }
    }

    /// Removes all properties matching `key`, returning `true` if at least one existed.
    pub fn remove_prop(&mut self, key: &str) -> bool {
        let before = self.props.len();
        self.props.retain(|p| p.key != key);
        self.props.len() < before
    }

    /// Returns the slice of child [`NodeId`]s.
    #[inline]
    pub fn children_ids(&self) -> &[NodeId] {
        &self.children
    }

    /// Returns `true` if this node has at least one child node.
    #[inline]
    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    /// Returns `true` if this node had an explicit `{ ... }` children block or has children.
    #[inline]
    pub fn has_children_block(&self) -> bool {
        self.has_children_block || !self.children.is_empty()
    }

    /// Returns the span of the children block, if present.
    #[inline]
    pub fn children_block_span(&self) -> Option<Span> {
        self.children_block_span
    }

    /// Sets or clears a trailing single-line comment on this node.
    pub fn set_trailing_comment(&mut self, comment: Option<impl Into<String>>) {
        self.trailing_comment = comment.map(Into::into);
    }

    /// Configures whether an empty children block renders as `{\n}`.
    pub fn set_empty_children_multiline(&mut self, multiline: bool) {
        self.empty_children_multiline = multiline;
        if multiline {
            self.has_children_block = true;
        }
    }

    /// Checks whether this standalone node matches the given KDL selector string.
    pub fn matches(&self, selector: &str) -> bool {
        try_standalone_matches(self, selector).unwrap_or(false)
    }

    /// Checks whether this standalone node matches `selector`, returning a [`KdlError`] if `selector` is malformed.
    pub fn try_matches(&self, selector: &str) -> Result<bool, KdlError> {
        try_standalone_matches(self, selector)
    }

    /// Serializes a standalone `KdlNode` (without document children) to KDL 2.0 text.
    ///
    /// For nodes attached to a [`KdlDocument`] with children, use [`NodeRef::to_kdl`] or [`KdlDocument::node_to_kdl`].
    pub fn to_kdl(&self) -> String {
        standalone_node_to_kdl(self, 0, "    ", false)
    }

    /// Serializes a standalone `KdlNode` with custom indentation and `force_children_block` options.
    pub fn to_kdl_with(
        &self,
        indent: usize,
        indent_str: &str,
        force_children_block: bool,
    ) -> String {
        standalone_node_to_kdl(self, indent, indent_str, force_children_block)
    }
}

/// Arena-backed KDL 2.0 document tree with `O(1)` parent/sibling navigation and precomputed line offsets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KdlDocument {
    nodes: Vec<KdlNode>,
    root_ids: Vec<NodeId>,
    line_index: LineIndex,
    span: Span,
}

impl Default for KdlDocument {
    fn default() -> Self {
        Self::new()
    }
}

fn check_escline_comment_in_doc(
    raw_doc: &kdl::KdlDocument,
    source: &str,
    line_index: &LineIndex,
) -> Result<(), KdlError> {
    for raw_node in raw_doc.nodes() {
        if raw_node.children().is_none() {
            if let Some(fmt) = raw_node.format() {
                if let Some(last_bs) = fmt.before_terminator.rfind('\\') {
                    let after_bs = &fmt.before_terminator[last_bs + 1..];
                    if !after_bs.contains("/-") {
                        let node_end = raw_node.span().offset() + raw_node.span().len();
                        let rest = source.get(node_end..).unwrap_or("");
                        let trimmed = rest.trim_start_matches([' ', '\t', '\r', '\n']);
                        if trimmed.starts_with("//") {
                            let ws_len = rest.len() - trimmed.len();
                            let comment_offset = node_end + ws_len;
                            let comment_len = trimmed.find(['\r', '\n']).unwrap_or(trimmed.len());
                            return Err(KdlError::escline_comment(
                                line_index,
                                comment_offset,
                                comment_len,
                            ));
                        }
                    }
                }
            }
        }
        if let Some(children) = raw_node.children() {
            check_escline_comment_in_doc(children, source, line_index)?;
        }
    }
    Ok(())
}

fn normalize_inline_type_annotations(source: &str) -> std::borrow::Cow<'_, str> {
    if !source.contains("(dict)") && !source.contains("(array)") {
        return std::borrow::Cow::Borrowed(source);
    }

    let mut out = String::with_capacity(source.len());
    let mut changed = false;

    for chunk in source.split_inclusive('\n') {
        let trimmed_start = chunk.trim_start_matches([' ', '\t']);
        let leading_len = chunk.len() - trimmed_start.len();
        let leading = &chunk[..leading_len];

        // Match `<ident><ws>((?:dict|array))(\??)` at the start of a statement line
        let ident_len = trimmed_start
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '@' | '?')))
            .unwrap_or(trimmed_start.len());

        if ident_len > 0 {
            let ident = &trimmed_start[..ident_len];
            let after_ident = &trimmed_start[ident_len..];
            let ws_len = after_ident
                .find(|c: char| !matches!(c, ' ' | '\t'))
                .unwrap_or(after_ident.len());

            if ws_len > 0 {
                let ws = &after_ident[..ws_len];
                let after_ws = &after_ident[ws_len..];
                let matched_ann = if after_ws.starts_with("(dict)") {
                    Some("(dict)")
                } else if after_ws.starts_with("(array)") {
                    Some("(array)")
                } else {
                    None
                };

                if let Some(ann) = matched_ann {
                    let after_ann = &after_ws[ann.len()..];
                    let (opt_q, rest) = if let Some(stripped) = after_ann.strip_prefix('?') {
                        ("?", stripped)
                    } else {
                        ("", after_ann)
                    };

                    let rest_trimmed = rest.trim_start_matches([' ', '\t']);
                    let is_bare_modifier = rest_trimmed.starts_with('{')
                        || rest_trimmed.starts_with('@')
                        || rest_trimmed.starts_with("from=")
                        || rest_trimmed.starts_with("path=");

                    if is_bare_modifier {
                        changed = true;
                        out.push_str(leading);
                        out.push_str(ann);
                        out.push_str(ident);
                        out.push_str(opt_q);
                        out.push_str(ws);
                        out.push_str(rest);
                        continue;
                    }
                }
            }
        }

        out.push_str(chunk);
    }

    if changed {
        debug_assert_eq!(out.len(), source.len());
        std::borrow::Cow::Owned(out)
    } else {
        std::borrow::Cow::Borrowed(source)
    }
}

fn extract_type_and_span(
    ty_ident: Option<&kdl::KdlIdentifier>,
    before_after: Option<(&str, &str)>,
    line_index: &LineIndex,
) -> (Option<String>, Option<Span>) {
    let Some(ty_ident) = ty_ident else {
        return (None, None);
    };
    let ident_offset = ty_ident.span().offset();
    let ident_len = ty_ident.span().len();
    let (before_len, after_len) =
        before_after.map_or((0, 0), |(before, after)| (before.len(), after.len()));
    let ty_start = ident_offset.saturating_sub(before_len + 1);
    let ty_len = 1 + before_len + ident_len + after_len + 1;
    (
        Some(ty_ident.value().to_owned()),
        Some(line_index.span(ty_start, ty_len)),
    )
}

impl KdlDocument {
    /// Creates an empty `KdlDocument`.
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            root_ids: Vec::new(),
            line_index: LineIndex::default(),
            span: Span::EMPTY,
        }
    }

    /// Parses a KDL 2.0 source string into an indexed `KdlDocument`.
    pub fn parse(source: &str) -> Result<Self, KdlError> {
        let line_index = LineIndex::new(source);
        let normalized = normalize_inline_type_annotations(source);
        let raw_doc = kdl::KdlDocument::parse_v2(&normalized)
            .map_err(|err| KdlError::from_kdl_error(err, &line_index))?;

        check_escline_comment_in_doc(&raw_doc, &normalized, &line_index)?;

        let doc_span = line_index.span(raw_doc.span().offset(), raw_doc.span().len());
        let mut doc = Self {
            nodes: Vec::new(),
            root_ids: Vec::new(),
            line_index,
            span: doc_span,
        };

        for (idx, raw_node) in raw_doc.nodes().iter().enumerate() {
            let id = doc.convert_raw_node(raw_node, None, None, 0, idx);
            doc.root_ids.push(id);
        }

        Ok(doc)
    }

    fn convert_raw_node(
        &mut self,
        raw_node: &kdl::KdlNode,
        parent: Option<NodeId>,
        root: Option<NodeId>,
        depth: usize,
        index_in_parent: usize,
    ) -> NodeId {
        let id = NodeId(self.nodes.len());
        let root_id = root.unwrap_or(id);

        let name = raw_node.name().value().to_owned();
        let name_span = self.line_index.span(
            raw_node.name().span().offset(),
            raw_node.name().span().len(),
        );

        let (ty, type_span) = extract_type_and_span(
            raw_node.ty(),
            raw_node
                .format()
                .map(|f| (f.before_ty_name.as_str(), f.after_ty_name.as_str())),
            &self.line_index,
        );

        let mut args = Vec::new();
        let mut props = Vec::new();

        for entry in raw_node.entries() {
            let entry_offset = entry.span().offset();
            let entry_len = entry.span().len();
            let entry_end = entry_offset + entry_len;
            let entry_span = self.line_index.span(entry_offset, entry_len);

            let val_len = entry.format().map_or(entry_len, |f| f.value_repr.len());
            let val_offset = entry_end.saturating_sub(val_len);
            let value_span = self.line_index.span(val_offset, val_len);

            let (entry_ty, entry_type_span) = extract_type_and_span(
                entry.ty(),
                entry
                    .format()
                    .map(|f| (f.before_ty_name.as_str(), f.after_ty_name.as_str())),
                &self.line_index,
            );

            let value = KdlValue::from_kdl(entry.value());

            if let Some(key_ident) = entry.name() {
                let key_span = self
                    .line_index
                    .span(key_ident.span().offset(), key_ident.span().len());
                props.push(KdlProp {
                    key: key_ident.value().to_owned(),
                    value,
                    ty: entry_ty,
                    span: entry_span,
                    key_span,
                    value_span,
                    type_span: entry_type_span,
                });
            } else {
                args.push(KdlArg {
                    value,
                    ty: entry_ty,
                    span: entry_span,
                    value_span,
                    type_span: entry_type_span,
                });
            }
        }

        let has_children_block = raw_node.children().is_some();
        let children_block_span = raw_node
            .children()
            .map(|c| self.line_index.span(c.span().offset(), c.span().len()));

        let trailing_ws_len = if raw_node.children().is_none() {
            raw_node.format().map_or(0, |f| f.before_terminator.len())
        } else {
            0
        };
        let node_len = raw_node.span().len().saturating_sub(trailing_ws_len);
        let span = self.line_index.span(raw_node.span().offset(), node_len);

        self.nodes.push(KdlNode {
            id,
            parent,
            root: root_id,
            depth,
            index_in_parent,
            name,
            name_span,
            ty,
            type_span,
            args,
            props,
            children: Vec::new(),
            has_children_block,
            children_block_span,
            empty_children_multiline: false,
            trailing_comment: None,
            span,
        });

        if let Some(children_doc) = raw_node.children() {
            let mut child_ids = Vec::with_capacity(children_doc.nodes().len());
            for (child_idx, raw_child) in children_doc.nodes().iter().enumerate() {
                let child_id =
                    self.convert_raw_node(raw_child, Some(id), Some(root_id), depth + 1, child_idx);
                child_ids.push(child_id);
            }
            self.nodes[id.0].children = child_ids;
        }

        id
    }

    /// Returns the source string from which this document was parsed (or `""` if constructed synthetically).
    #[inline]
    pub fn source(&self) -> &str {
        self.line_index.source()
    }

    /// Returns the precomputed [`LineIndex`] for this document.
    #[inline]
    pub fn line_index(&self) -> &LineIndex {
        &self.line_index
    }

    /// Returns the source span covering the document.
    #[inline]
    pub fn span(&self) -> Span {
        self.span
    }

    /// Returns `true` if the document has no top-level nodes.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.root_ids.is_empty()
    }

    /// Returns the number of top-level nodes in the document.
    #[inline]
    pub fn len(&self) -> usize {
        self.root_ids.len()
    }

    /// Returns the total number of nodes in the document arena.
    #[inline]
    pub fn total_nodes(&self) -> usize {
        self.nodes.len()
    }

    /// Returns the slice of all nodes in the arena.
    #[inline]
    pub fn arena_nodes(&self) -> &[KdlNode] {
        &self.nodes
    }

    /// Returns the slice of top-level [`NodeId`]s.
    #[inline]
    pub fn root_ids(&self) -> &[NodeId] {
        &self.root_ids
    }

    /// Returns top-level nodes as [`NodeRef`] handles.
    pub fn roots(&self) -> Vec<NodeRef<'_>> {
        self.root_ids
            .iter()
            .map(|&id| NodeRef { doc: self, id })
            .collect()
    }

    /// Alias for [`KdlDocument::roots`].
    #[inline]
    pub fn nodes(&self) -> Vec<NodeRef<'_>> {
        self.roots()
    }

    /// Returns a [`NodeRef`] handle for `id`.
    ///
    /// # Panics
    ///
    /// Panics if `id` is out of bounds for this document.
    #[inline]
    pub fn node(&self, id: NodeId) -> NodeRef<'_> {
        assert!(id.0 < self.nodes.len(), "NodeId({}) out of bounds", id.0);
        NodeRef { doc: self, id }
    }

    /// Returns a [`NodeRef`] handle for `id` if it exists in this document.
    #[inline]
    pub fn get_node(&self, id: NodeId) -> Option<NodeRef<'_>> {
        if id.0 < self.nodes.len() {
            Some(NodeRef { doc: self, id })
        } else {
            None
        }
    }

    /// Returns a mutable reference to the [`KdlNode`] at `id`.
    #[inline]
    pub fn node_mut(&mut self, id: NodeId) -> &mut KdlNode {
        &mut self.nodes[id.0]
    }

    /// Iterates all nodes in the document in pre-order (depth-first, parent before children).
    pub fn iter_nodes(&self) -> impl Iterator<Item = NodeRef<'_>> + '_ {
        let mut order = Vec::with_capacity(self.nodes.len());
        let mut stack: Vec<NodeId> = self.root_ids.iter().copied().rev().collect();
        while let Some(id) = stack.pop() {
            order.push(NodeRef { doc: self, id });
            for &child_id in self.nodes[id.0].children.iter().rev() {
                stack.push(child_id);
            }
        }
        order.into_iter()
    }

    // ------------------------------------------------------------------
    // Document-level Navigation by NodeId
    // ------------------------------------------------------------------

    /// Returns the parent [`NodeRef`] of `id`, or `None` if `id` is a root-level node.
    #[inline]
    pub fn parent(&self, id: NodeId) -> Option<NodeRef<'_>> {
        self.nodes[id.0]
            .parent
            .map(|pid| NodeRef { doc: self, id: pid })
    }

    /// Returns the root-level ancestor [`NodeRef`] of `id` (or `id` itself if already a root node).
    #[inline]
    pub fn root(&self, id: NodeId) -> NodeRef<'_> {
        let rid = self.nodes[id.0].root;
        NodeRef { doc: self, id: rid }
    }

    /// Returns the depth of `id` (`0` for root-level nodes).
    #[inline]
    pub fn depth(&self, id: NodeId) -> usize {
        self.nodes[id.0].depth
    }

    /// Returns the 0-based sibling index of `id`.
    #[inline]
    pub fn index(&self, id: NodeId) -> usize {
        self.nodes[id.0].index_in_parent
    }

    /// Returns the child [`NodeRef`]s of `id`.
    pub fn children(&self, id: NodeId) -> Vec<NodeRef<'_>> {
        self.nodes[id.0]
            .children
            .iter()
            .map(|&cid| NodeRef { doc: self, id: cid })
            .collect()
    }

    /// Returns the sibling [`NodeId`] slice containing `id` (including `id` itself).
    #[inline]
    pub fn sibling_ids(&self, id: NodeId) -> &[NodeId] {
        match self.nodes[id.0].parent {
            Some(pid) => &self.nodes[pid.0].children,
            None => &self.root_ids,
        }
    }

    /// Returns the sibling [`NodeRef`]s containing `id` (including `id` itself).
    pub fn siblings(&self, id: NodeId) -> Vec<NodeRef<'_>> {
        self.sibling_ids(id)
            .iter()
            .map(|&sid| NodeRef { doc: self, id: sid })
            .collect()
    }

    /// Returns the immediately preceding sibling [`NodeRef`] of `id`, if any.
    #[inline]
    pub fn prev_sibling(&self, id: NodeId) -> Option<NodeRef<'_>> {
        let idx = self.nodes[id.0].index_in_parent;
        if idx == 0 {
            return None;
        }
        let sibs = self.sibling_ids(id);
        sibs.get(idx - 1).map(|&sid| NodeRef { doc: self, id: sid })
    }

    /// Returns the immediately following sibling [`NodeRef`] of `id`, if any.
    #[inline]
    pub fn next_sibling(&self, id: NodeId) -> Option<NodeRef<'_>> {
        let idx = self.nodes[id.0].index_in_parent;
        let sibs = self.sibling_ids(id);
        sibs.get(idx + 1).map(|&sid| NodeRef { doc: self, id: sid })
    }

    /// Returns all ancestors of `id` starting from its direct parent up to the root node.
    pub fn parents(&self, id: NodeId) -> Vec<NodeRef<'_>> {
        let mut out = Vec::with_capacity(self.nodes[id.0].depth);
        let mut cur = self.nodes[id.0].parent;
        while let Some(pid) = cur {
            out.push(NodeRef { doc: self, id: pid });
            cur = self.nodes[pid.0].parent;
        }
        out
    }

    /// Walks up from `id` (checking `id` first, then ancestors) and returns the first node matching `matcher`.
    pub fn closest<'a>(&'a self, id: NodeId, matcher: impl NodeMatcher<'a>) -> Option<NodeRef<'a>> {
        self.node(id).closest(matcher)
    }

    /// Walks up from `id` (checking `id` first, then ancestors) and returns the first node matching `selector`.
    pub fn try_closest(&self, id: NodeId, selector: &str) -> Result<Option<NodeRef<'_>>, KdlError> {
        self.node(id).try_closest(selector)
    }

    /// Checks whether node `id` matches `matcher`.
    pub fn matches<'a>(&'a self, id: NodeId, matcher: impl NodeMatcher<'a>) -> bool {
        self.node(id).matches(matcher)
    }

    /// Checks whether node `id` matches `selector`, returning a [`KdlError`] if `selector` is malformed.
    pub fn try_matches(&self, id: NodeId, selector: &str) -> Result<bool, KdlError> {
        self.node(id).try_matches(selector)
    }

    // ------------------------------------------------------------------
    // Selector Queries
    // ------------------------------------------------------------------

    /// Queries the entire document using a CSS3-like KDL selector expression, returning matching nodes in pre-order.
    pub fn select(&self, selector: &str) -> Result<Vec<NodeRef<'_>>, KdlError> {
        select_doc(self, selector)
    }

    /// Queries the entire document using a CSS3-like KDL selector expression, returning the first matching node in pre-order.
    pub fn select_first(&self, selector: &str) -> Result<Option<NodeRef<'_>>, KdlError> {
        select_doc_first(self, selector)
    }

    /// Alias for [`KdlDocument::select_first`].
    #[inline]
    pub fn select_one(&self, selector: &str) -> Result<Option<NodeRef<'_>>, KdlError> {
        self.select_first(selector)
    }

    /// Queries the entire document using a CSS3-like KDL selector expression, returning matching [`NodeId`]s in pre-order.
    pub fn select_ids(&self, selector: &str) -> Result<Vec<NodeId>, KdlError> {
        select_doc_ids(self, selector)
    }

    /// Queries the entire document using a CSS3-like KDL selector expression, returning the first matching [`NodeId`] in pre-order.
    pub fn select_first_id(&self, selector: &str) -> Result<Option<NodeId>, KdlError> {
        select_doc_first_id(self, selector)
    }

    // ------------------------------------------------------------------
    // Programmatic Construction & Mutation
    // ------------------------------------------------------------------

    /// Appends a new top-level node with `name` and returns its [`NodeId`].
    pub fn add_root(&mut self, name: impl Into<String>) -> NodeId {
        self.add_node(KdlNode::new(name))
    }

    /// Appends a preconfigured [`KdlNode`] as a top-level node and returns its [`NodeId`].
    pub fn add_node(&mut self, mut node: KdlNode) -> NodeId {
        let id = NodeId(self.nodes.len());
        let index_in_parent = self.root_ids.len();
        node.id = id;
        node.parent = None;
        node.root = id;
        node.depth = 0;
        node.index_in_parent = index_in_parent;
        node.children.clear();
        self.nodes.push(node);
        self.root_ids.push(id);
        id
    }

    /// Inserts a preconfigured [`KdlNode`] as a top-level node at `index` and returns its [`NodeId`].
    pub fn insert_node(&mut self, index: usize, mut node: KdlNode) -> NodeId {
        let id = NodeId(self.nodes.len());
        let insert_pos = index.min(self.root_ids.len());
        node.id = id;
        node.parent = None;
        node.root = id;
        node.depth = 0;
        node.index_in_parent = insert_pos;
        node.children.clear();
        self.nodes.push(node);
        self.root_ids.insert(insert_pos, id);
        for (idx, &rid) in self.root_ids.iter().enumerate().skip(insert_pos + 1) {
            self.nodes[rid.0].index_in_parent = idx;
        }
        id
    }

    /// Appends a new child node with `name` to `parent` and returns the child's [`NodeId`].
    pub fn add_child(&mut self, parent: NodeId, name: impl Into<String>) -> NodeId {
        self.add_child_node(parent, KdlNode::new(name))
    }

    /// Appends a preconfigured [`KdlNode`] as a child of `parent` and returns the child's [`NodeId`].
    pub fn add_child_node(&mut self, parent: NodeId, mut child: KdlNode) -> NodeId {
        let id = NodeId(self.nodes.len());
        let root_id = self.nodes[parent.0].root;
        let depth = self.nodes[parent.0].depth + 1;
        let index_in_parent = self.nodes[parent.0].children.len();

        child.id = id;
        child.parent = Some(parent);
        child.root = root_id;
        child.depth = depth;
        child.index_in_parent = index_in_parent;
        child.children.clear();

        self.nodes.push(child);
        self.nodes[parent.0].children.push(id);
        self.nodes[parent.0].has_children_block = true;
        id
    }

    /// Inserts a preconfigured [`KdlNode`] as a child of `parent` at `index` and returns the child's [`NodeId`].
    pub fn insert_child_node(
        &mut self,
        parent: NodeId,
        index: usize,
        mut child: KdlNode,
    ) -> NodeId {
        let id = NodeId(self.nodes.len());
        let root_id = self.nodes[parent.0].root;
        let depth = self.nodes[parent.0].depth + 1;
        let insert_pos = index.min(self.nodes[parent.0].children.len());

        child.id = id;
        child.parent = Some(parent);
        child.root = root_id;
        child.depth = depth;
        child.index_in_parent = insert_pos;
        child.children.clear();

        self.nodes.push(child);
        self.nodes[parent.0].children.insert(insert_pos, id);
        self.nodes[parent.0].has_children_block = true;

        let sibling_ids = self.nodes[parent.0].children.clone();
        for (idx, sid) in sibling_ids.into_iter().enumerate().skip(insert_pos + 1) {
            self.nodes[sid.0].index_in_parent = idx;
        }
        id
    }

    // ------------------------------------------------------------------
    // Serialization
    // ------------------------------------------------------------------

    /// Serializes this document to formatted KDL 2.0 text (4-space indentation, trailing newline).
    pub fn to_kdl(&self) -> String {
        doc_to_kdl(self, "    ")
    }

    /// Serializes this document to formatted KDL 2.0 text using `indent_str`.
    pub fn to_kdl_with(&self, indent_str: &str) -> String {
        doc_to_kdl(self, indent_str)
    }

    /// Serializes a single node (and its subtree) in this document to formatted KDL 2.0 text.
    pub fn node_to_kdl(&self, id: NodeId) -> String {
        doc_node_to_kdl(self, id, 0, "    ", false)
    }
}

impl Index<NodeId> for KdlDocument {
    type Output = KdlNode;

    #[inline]
    fn index(&self, id: NodeId) -> &Self::Output {
        &self.nodes[id.0]
    }
}

impl IndexMut<NodeId> for KdlDocument {
    #[inline]
    fn index_mut(&mut self, id: NodeId) -> &mut Self::Output {
        &mut self.nodes[id.0]
    }
}

impl fmt::Display for KdlDocument {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_kdl())
    }
}

/// Lightweight, `Copy` DOM-like node handle binding a [`NodeId`] to its owning [`KdlDocument`].
#[derive(Debug, Clone, Copy)]
pub struct NodeRef<'a> {
    doc: &'a KdlDocument,
    id: NodeId,
}

impl<'a> PartialEq for NodeRef<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && std::ptr::eq(self.doc, other.doc)
    }
}

impl<'a> Eq for NodeRef<'a> {}

impl<'a> Hash for NodeRef<'a> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
        (self.doc as *const KdlDocument).hash(state);
    }
}

impl<'a> Deref for NodeRef<'a> {
    type Target = KdlNode;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.doc[self.id]
    }
}

impl<'a> NodeRef<'a> {
    /// Returns the [`NodeId`] of this node.
    #[inline]
    pub fn id(self) -> NodeId {
        self.id
    }

    /// Returns a reference to the owning [`KdlDocument`].
    #[inline]
    pub fn document(self) -> &'a KdlDocument {
        self.doc
    }

    /// Returns the underlying `&'a KdlNode`.
    #[inline]
    pub fn as_node(self) -> &'a KdlNode {
        &self.doc[self.id]
    }

    /// Returns the node's name with document lifetime `'a`.
    #[inline]
    pub fn name(self) -> &'a str {
        &self.doc[self.id].name
    }

    /// Returns the node's name span.
    #[inline]
    pub fn name_span(self) -> Span {
        self.doc[self.id].name_span
    }

    /// Returns the node's type annotation identifier (without parentheses) with lifetime `'a`.
    #[inline]
    pub fn ty(self) -> Option<&'a str> {
        self.doc[self.id].ty.as_deref()
    }

    /// Alias for [`NodeRef::ty`].
    #[inline]
    pub fn type_annotation(self) -> Option<&'a str> {
        self.ty()
    }

    /// Returns the node's `(type)` annotation span, if present.
    #[inline]
    pub fn type_span(self) -> Option<Span> {
        self.doc[self.id].type_span
    }

    /// Returns the full node span.
    #[inline]
    pub fn span(self) -> Span {
        self.doc[self.id].span
    }

    /// Returns the node's positional arguments with lifetime `'a`.
    #[inline]
    pub fn args(self) -> &'a [KdlArg] {
        &self.doc[self.id].args
    }

    /// Returns the positional argument at `index` with lifetime `'a`.
    #[inline]
    pub fn arg(self, index: usize) -> Option<&'a KdlArg> {
        self.doc[self.id].args.get(index)
    }

    /// Returns the node's named properties with lifetime `'a`.
    #[inline]
    pub fn props(self) -> &'a [KdlProp] {
        &self.doc[self.id].props
    }

    /// Looks up a named property by `key` (returning the last matching property per KDL 2.0 spec).
    #[inline]
    pub fn prop(self, key: &str) -> Option<&'a KdlProp> {
        self.doc[self.id].prop(key)
    }

    /// Looks up a named property's [`KdlValue`] by `key`.
    #[inline]
    pub fn get(self, key: &str) -> Option<&'a KdlValue> {
        self.doc[self.id].get(key)
    }

    /// Returns `true` if this node has a named property with `key`.
    #[inline]
    pub fn has_prop(self, key: &str) -> bool {
        self.doc[self.id].has_prop(key)
    }

    /// Returns `true` if this node has at least one child node.
    #[inline]
    pub fn has_children(self) -> bool {
        self.doc[self.id].has_children()
    }

    // ------------------------------------------------------------------
    // DOM-like Navigation
    // ------------------------------------------------------------------

    /// Returns the parent [`NodeRef`], or `None` if this is a root-level node.
    #[inline]
    pub fn parent(self) -> Option<NodeRef<'a>> {
        self.doc.parent(self.id)
    }

    /// Returns the top-level root ancestor [`NodeRef`] of this node (or `self` if already a root node).
    #[inline]
    pub fn root(self) -> NodeRef<'a> {
        self.doc.root(self.id)
    }

    /// Returns the depth of this node in the tree (`0` for root-level nodes).
    #[inline]
    pub fn depth(self) -> usize {
        self.doc.depth(self.id)
    }

    /// Returns the 0-based position of this node among its siblings.
    #[inline]
    pub fn index(self) -> usize {
        self.doc.index(self.id)
    }

    /// Returns the direct child [`NodeRef`]s of this node in source order.
    #[inline]
    pub fn children(self) -> Vec<NodeRef<'a>> {
        self.doc.children(self.id)
    }

    /// Returns the sibling [`NodeRef`]s containing this node (including `self`).
    #[inline]
    pub fn siblings(self) -> Vec<NodeRef<'a>> {
        self.doc.siblings(self.id)
    }

    /// Returns the immediately preceding sibling [`NodeRef`], if any.
    #[inline]
    pub fn prev_sibling(self) -> Option<NodeRef<'a>> {
        self.doc.prev_sibling(self.id)
    }

    /// Returns the immediately following sibling [`NodeRef`], if any.
    #[inline]
    pub fn next_sibling(self) -> Option<NodeRef<'a>> {
        self.doc.next_sibling(self.id)
    }

    /// Returns all ancestors from direct parent up to the root node (empty for root-level nodes).
    #[inline]
    pub fn parents(self) -> Vec<NodeRef<'a>> {
        self.doc.parents(self.id)
    }

    /// Iterates all descendant nodes of `self` in pre-order (depth-first).
    pub fn iter_descendants(self) -> impl Iterator<Item = NodeRef<'a>> + 'a {
        let mut order = Vec::new();
        let mut stack: Vec<NodeId> = self.doc[self.id].children.iter().copied().rev().collect();
        while let Some(id) = stack.pop() {
            order.push(NodeRef { doc: self.doc, id });
            for &child_id in self.doc[id].children.iter().rev() {
                stack.push(child_id);
            }
        }
        order.into_iter()
    }

    /// Returns all descendant nodes of `self` in pre-order (depth-first).
    pub fn descendants(self) -> Vec<NodeRef<'a>> {
        self.iter_descendants().collect()
    }

    // ------------------------------------------------------------------
    // Subtree Selector Queries & Matching
    // ------------------------------------------------------------------

    /// Queries this node's descendant subtree using a CSS3-like KDL selector expression.
    ///
    /// Subtree semantics: only descendants of `self` are searched,
    /// combinators cannot traverse above `self`, and `:root` never matches in a subtree.
    pub fn select(self, selector: &str) -> Result<Vec<NodeRef<'a>>, KdlError> {
        select_subtree(self, selector)
    }

    /// Returns the first descendant in `self`'s subtree matching `selector` in pre-order.
    pub fn select_first(self, selector: &str) -> Result<Option<NodeRef<'a>>, KdlError> {
        select_subtree_first(self, selector)
    }

    /// Alias for [`NodeRef::select_first`].
    #[inline]
    pub fn select_one(self, selector: &str) -> Result<Option<NodeRef<'a>>, KdlError> {
        self.select_first(selector)
    }

    /// Queries this node's descendant subtree using `selector`, returning matching [`NodeId`]s in pre-order.
    pub fn select_ids(self, selector: &str) -> Result<Vec<NodeId>, KdlError> {
        select_subtree_ids(self, selector)
    }

    /// Returns the first descendant [`NodeId`] in `self`'s subtree matching `selector` in pre-order.
    pub fn select_first_id(self, selector: &str) -> Result<Option<NodeId>, KdlError> {
        select_subtree_first_id(self, selector)
    }

    /// Checks whether this node matches `matcher` (either a selector string, compiled [`crate::kdl::Selector`], or a predicate closure `|node: NodeRef| ...`).
    pub fn matches(self, matcher: impl NodeMatcher<'a>) -> bool {
        matcher.matches_node(self)
    }

    /// Checks whether this node matches `selector`, returning a [`KdlError`] if `selector` is malformed.
    pub fn try_matches(self, selector: &str) -> Result<bool, KdlError> {
        try_node_matches(self, selector)
    }

    /// Checks whether this node satisfies predicate `pred`.
    pub fn matches_by(self, pred: impl Fn(NodeRef<'a>) -> bool) -> bool {
        pred(self)
    }

    /// Walks up the ancestor chain starting from `self` and returns the first node matching `matcher`.
    pub fn closest(self, matcher: impl NodeMatcher<'a>) -> Option<NodeRef<'a>> {
        let mut cur = Some(self);
        while let Some(node) = cur {
            if matcher.matches_node(node) {
                return Some(node);
            }
            cur = node.parent();
        }
        None
    }

    /// Walks up the ancestor chain starting from `self` and returns the first node matching `selector`, or a [`KdlError`] if `selector` is malformed.
    pub fn try_closest(self, selector: &str) -> Result<Option<NodeRef<'a>>, KdlError> {
        try_node_closest(self, selector)
    }

    /// Walks up the ancestor chain starting from `self` and returns the first node satisfying `pred`.
    pub fn closest_by(self, pred: impl Fn(NodeRef<'a>) -> bool) -> Option<NodeRef<'a>> {
        let mut cur = Some(self);
        while let Some(node) = cur {
            if pred(node) {
                return Some(node);
            }
            cur = node.parent();
        }
        None
    }

    /// Evaluates a CSS3-like KDL selector string against `self` (returning `false` if `selector` is invalid).
    #[inline]
    pub(crate) fn matches_selector_str(self, selector: &str) -> bool {
        try_node_matches(self, selector).unwrap_or(false)
    }

    // ------------------------------------------------------------------
    // Serialization
    // ------------------------------------------------------------------

    /// Serializes this node and its subtree to formatted KDL 2.0 text (no trailing newline).
    pub fn to_kdl(self) -> String {
        doc_node_to_kdl(self.doc, self.id, 0, "    ", false)
    }

    /// Serializes this node and its subtree with custom indentation and `force_children_block` options.
    pub fn to_kdl_with(
        self,
        indent: usize,
        indent_str: &str,
        force_children_block: bool,
    ) -> String {
        doc_node_to_kdl(self.doc, self.id, indent, indent_str, force_children_block)
    }
}

impl<'a> fmt::Display for NodeRef<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_kdl())
    }
}

/// Trait implemented by node predicates (`Fn(NodeRef<'a>) -> bool`) and selector strings (`&str`, `String`).
pub trait NodeMatcher<'a> {
    /// Returns `true` if `node` matches this predicate or selector.
    fn matches_node(&self, node: NodeRef<'a>) -> bool;
}

impl<'a, F> NodeMatcher<'a> for F
where
    F: Fn(NodeRef<'a>) -> bool,
{
    #[inline]
    fn matches_node(&self, node: NodeRef<'a>) -> bool {
        (self)(node)
    }
}

impl<'a> NodeMatcher<'a> for &str {
    #[inline]
    fn matches_node(&self, node: NodeRef<'a>) -> bool {
        node.matches_selector_str(self)
    }
}

impl<'a> NodeMatcher<'a> for String {
    #[inline]
    fn matches_node(&self, node: NodeRef<'a>) -> bool {
        node.matches_selector_str(self.as_str())
    }
}

impl<'a> NodeMatcher<'a> for &String {
    #[inline]
    fn matches_node(&self, node: NodeRef<'a>) -> bool {
        node.matches_selector_str(self.as_str())
    }
}

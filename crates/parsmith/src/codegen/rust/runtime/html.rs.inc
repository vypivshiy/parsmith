pub type Dom = Rc<RefCell<Document>>;
pub type Node = NodeId;
pub type Nodes = Vec<NodeId>;

pub fn root_id(dom: &Dom) -> NodeId {
    dom.borrow().root().id
}

pub fn parse_doc(input: impl Into<String>) -> (Dom, NodeId) {
    let document = Document::from(input.into());
    let dom = Rc::new(RefCell::new(document));
    let root = root_id(&dom);
    (dom, root)
}

pub fn root_nodes(dom: &Dom) -> Nodes {
    vec![root_id(dom)]
}

pub fn first_node(nodes: &Nodes, context: &str) -> Result<NodeId, RtError> {
    nodes
        .first()
        .copied()
        .ok_or_else(|| RtError::new(context, "value has no selected node"))
}

pub fn node_at(nodes: &Nodes, index: i64, context: &str) -> Result<Nodes, RtError> {
    let pos = if index < 0 {
        nodes.len() as i64 + index
    } else {
        index
    };
    if pos < 0 {
        return Err(RtError::new(context, "index out of bounds"));
    }
    nodes
        .get(pos as usize)
        .copied()
        .map(|id| vec![id])
        .ok_or_else(|| RtError::new(context, "index out of bounds"))
}

fn select_from<'a>(doc: &'a Document, root: NodeId, query: &str) -> Selection<'a> {
    if root == doc.root().id {
        doc.select(query)
    } else {
        Selection::from(NodeRef::new(root, &doc.tree)).select(query)
    }
}

pub fn select(
    dom: &Dom,
    roots: &Nodes,
    queries: &[&str],
    context: &str,
) -> Result<Nodes, RtError> {
    let doc = dom.borrow();
    for query in queries {
        if roots.is_empty() {
            continue;
        }
        for root in roots {
            let sel = select_from(&doc, *root, query);
            if let Some(first) = sel.nodes().first() {
                return Ok(vec![first.id]);
            }
        }
    }
    Err(RtError::new(context, "selector matched no elements"))
}

pub fn select_opt(dom: &Dom, roots: &Nodes, queries: &[&str]) -> Nodes {
    let doc = dom.borrow();
    for query in queries {
        if roots.is_empty() {
            continue;
        }
        for root in roots {
            let sel = select_from(&doc, *root, query);
            if let Some(first) = sel.nodes().first() {
                return vec![first.id];
            }
        }
    }
    Vec::new()
}

pub fn select_all(
    dom: &Dom,
    roots: &Nodes,
    queries: &[&str],
    _context: &str,
) -> Result<Nodes, RtError> {
    Ok(select_many(dom, roots, queries))
}

pub fn select_many(dom: &Dom, roots: &Nodes, queries: &[&str]) -> Nodes {
    let doc = dom.borrow();
    for query in queries {
        let mut out = Vec::new();
        for root in roots {
            out.extend(
                select_from(&doc, *root, query)
                    .nodes()
                    .iter()
                    .map(|n| n.id),
            );
        }
        if !out.is_empty() {
            return out;
        }
    }
    Vec::new()
}

pub fn remove(dom: &Dom, roots: &Nodes) -> Result<Nodes, RtError> {
    let doc = dom.borrow();
    for id in roots {
        NodeRef::new(*id, &doc.tree).remove_from_parent();
    }
    Ok(roots.clone())
}

pub fn css_remove(
    dom: &Dom,
    roots: &Nodes,
    query: &str,
    context: &str,
) -> Result<Nodes, RtError> {
    let removed = select_all(dom, roots, &[query], context)?;
    remove(dom, &removed)?;
    Ok(roots.clone())
}

pub fn text(dom: &Dom, roots: &Nodes, context: &str) -> Result<String, RtError> {
    let doc = dom.borrow();
    roots
        .first()
        .map(|id| NodeRef::new(*id, &doc.tree).text().to_string())
        .ok_or_else(|| RtError::new(context, "value has no selected node"))
}

pub fn text_all(dom: &Dom, roots: &Nodes) -> Vec<String> {
    let doc = dom.borrow();
    roots
        .iter()
        .map(|id| NodeRef::new(*id, &doc.tree).text().to_string())
        .collect()
}

pub fn raw(dom: &Dom, roots: &Nodes, inner: bool, context: &str) -> Result<String, RtError> {
    let doc = dom.borrow();
    roots
        .first()
        .map(|id| {
            let node = NodeRef::new(*id, &doc.tree);
            if inner {
                node.inner_html().to_string()
            } else {
                node.html().to_string()
            }
        })
        .ok_or_else(|| RtError::new(context, "value has no selected node"))
}

pub fn raw_html(dom: &Dom, roots: &Nodes, inner: bool, context: &str) -> Result<String, RtError> {
    raw(dom, roots, inner, context)
}

pub fn raw_all(dom: &Dom, roots: &Nodes, inner: bool) -> Vec<String> {
    let doc = dom.borrow();
    roots
        .iter()
        .map(|id| {
            let node = NodeRef::new(*id, &doc.tree);
            if inner {
                node.inner_html().to_string()
            } else {
                node.html().to_string()
            }
        })
        .collect()
}

pub fn attr(dom: &Dom, roots: &Nodes, names: &[&str], context: &str) -> Result<String, RtError> {
    let doc = dom.borrow();
    let node = roots
        .first()
        .ok_or_else(|| RtError::new(context, "value has no selected node"))?;
    let node = NodeRef::new(*node, &doc.tree);
    names
        .iter()
        .find_map(|name| node.attr(name).map(|v| v.to_string()))
        .ok_or_else(|| RtError::new(context, "attribute not found"))
}

pub fn attr_opt(dom: &Dom, roots: &Nodes, names: &[&str]) -> Option<String> {
    let doc = dom.borrow();
    let node = roots.first()?;
    let node = NodeRef::new(*node, &doc.tree);
    names
        .iter()
        .find_map(|name| node.attr(name).map(|v| v.to_string()))
}

pub fn attr_list(
    dom: &Dom,
    roots: &Nodes,
    names: &[&str],
    context: &str,
) -> Result<Vec<String>, RtError> {
    let doc = dom.borrow();
    let mut out = Vec::with_capacity(roots.len());
    for id in roots {
        let node = NodeRef::new(*id, &doc.tree);
        let val = names
            .iter()
            .find_map(|name| node.attr(name).map(|v| v.to_string()))
            .ok_or_else(|| RtError::new(context, "attribute not found"))?;
        out.push(val);
    }
    Ok(out)
}

pub fn attr_multi(dom: &Dom, roots: &Nodes, names: &[&str]) -> Vec<String> {
    let doc = dom.borrow();
    let mut out = Vec::new();
    for id in roots {
        let node = NodeRef::new(*id, &doc.tree);
        for name in names {
            if let Some(val) = node.attr(name) {
                let s = val.to_string();
                if !s.is_empty() {
                    out.push(s);
                }
            }
        }
    }
    out
}

pub fn attr_all(dom: &Dom, roots: &Nodes, names: &[&str]) -> Vec<String> {
    attr_multi(dom, roots, names)
}

pub fn node_bool(value: &Nodes) -> bool {
    !value.is_empty()
}

pub fn pred_css(dom: &Dom, roots: &Nodes, query: &str) -> bool {
    let doc = dom.borrow();
    roots
        .iter()
        .any(|id| !select_from(&doc, *id, query).is_empty())
}

pub fn pred_attr(dom: &Dom, roots: &Nodes, name: &str) -> Option<String> {
    let doc = dom.borrow();
    roots
        .first()
        .and_then(|id| NodeRef::new(*id, &doc.tree).attr(name).map(|v| v.to_string()))
}

pub fn pred_has_attr(dom: &Dom, roots: &Nodes, names: &[&str]) -> bool {
    let doc = dom.borrow();
    roots.first().is_some_and(|id| {
        let node = NodeRef::new(*id, &doc.tree);
        names.iter().any(|name| node.attr(name).is_some())
    })
}

pub fn pred_text(dom: &Dom, roots: &Nodes) -> String {
    text_all(dom, roots).join("")
}

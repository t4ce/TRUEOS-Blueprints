use crate::{Attribute, ParseError};

pub type NodeId = usize;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Namespace {
    #[default]
    Html,
    Svg,
    MathMl,
}
impl Namespace {
    pub const fn uri(self) -> &'static str {
        match self {
            Self::Html => "http://www.w3.org/1999/xhtml",
            Self::Svg => "http://www.w3.org/2000/svg",
            Self::MathMl => "http://www.w3.org/1998/Math/MathML",
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DocumentMode {
    #[default]
    NoQuirks,
    Quirks,
    LimitedQuirks,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeKind {
    Document,
    Fragment,
    Element {
        name: String,
        namespace: Namespace,
        attrs: Vec<Attribute>,
        template_content: Option<NodeId>,
    },
    Text(String),
    Comment(String),
    Doctype {
        name: String,
        public_id: String,
        system_id: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub kind: NodeKind,
    pub parent: Option<NodeId>,
    pub children: Vec<NodeId>,
}
/// An arena keeps node handles stable through adoption and foster parenting,
/// and avoids recursive drops for deeply nested input.
#[derive(Clone, Debug)]
pub struct Document {
    pub nodes: Vec<Node>,
    pub root: NodeId,
    pub mode: DocumentMode,
    pub errors: Vec<ParseError>,
}
impl Document {
    pub fn new(fragment: bool) -> Self {
        Self {
            nodes: vec![Node {
                kind: if fragment {
                    NodeKind::Fragment
                } else {
                    NodeKind::Document
                },
                parent: None,
                children: vec![],
            }],
            root: 0,
            mode: DocumentMode::NoQuirks,
            errors: vec![],
        }
    }
    pub fn add(&mut self, kind: NodeKind) -> NodeId {
        let id = self.nodes.len();
        self.nodes.push(Node {
            kind,
            parent: None,
            children: vec![],
        });
        id
    }
    pub fn detach(&mut self, child: NodeId) {
        if let Some(parent) = self.nodes[child].parent.take() {
            self.nodes[parent].children.retain(|id| *id != child);
        }
    }
    pub fn append(&mut self, parent: NodeId, child: NodeId) {
        self.detach(child);
        self.nodes[parent].children.push(child);
        self.nodes[child].parent = Some(parent);
    }
    pub fn insert_before(&mut self, parent: NodeId, child: NodeId, before: NodeId) {
        self.detach(child);
        let index = self.nodes[parent]
            .children
            .iter()
            .position(|id| *id == before)
            .unwrap_or(self.nodes[parent].children.len());
        self.nodes[parent].children.insert(index, child);
        self.nodes[child].parent = Some(parent);
    }
    pub fn append_text(&mut self, parent: NodeId, text: &str, before: Option<NodeId>) {
        if text.is_empty() {
            return;
        }
        let index = before
            .and_then(|before| {
                self.nodes[parent]
                    .children
                    .iter()
                    .position(|id| *id == before)
            })
            .unwrap_or(self.nodes[parent].children.len());
        if let Some(previous) = index
            .checked_sub(1)
            .map(|index| self.nodes[parent].children[index])
        {
            if let NodeKind::Text(value) = &mut self.nodes[previous].kind {
                value.push_str(text);
                return;
            }
        }
        let node = self.add(NodeKind::Text(text.into()));
        if let Some(before) = before {
            self.insert_before(parent, node, before);
        } else {
            self.append(parent, node);
        }
    }
    pub fn element(&self, id: NodeId) -> Option<(&str, Namespace, &[Attribute])> {
        match &self.nodes[id].kind {
            NodeKind::Element {
                name,
                namespace,
                attrs,
                ..
            } => Some((name, *namespace, attrs)),
            _ => None,
        }
    }
    pub fn name(&self, id: NodeId) -> &str {
        self.element(id).map_or("", |(name, _, _)| name)
    }
    pub fn namespace(&self, id: NodeId) -> Namespace {
        self.element(id)
            .map_or(Namespace::Html, |(_, namespace, _)| namespace)
    }
    pub fn text_content(&self, id: NodeId) -> String {
        let mut result = String::new();
        let mut pending = vec![id];
        while let Some(id) = pending.pop() {
            if let NodeKind::Text(text) = &self.nodes[id].kind {
                result.push_str(text);
            }
            pending.extend(self.nodes[id].children.iter().rev().copied());
        }
        result
    }
    pub fn title(&self) -> Option<String> {
        self.nodes.iter().enumerate().find_map(|(id, _)| {
            (self.name(id) == "title" && self.namespace(id) == Namespace::Html)
                .then(|| self.text_content(id))
        })
    }
}

use crate::{Attribute, ParseError};
use alloc::collections::BTreeMap;

/// A committed element-property mutation. Reading a property emits no event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElementPropertyChange {
    pub node: NodeId,
    pub property: String,
    pub previous: Option<String>,
    pub value: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ElementPropertyError {
    NotElement,
    EmptyName,
}

pub type ElementPropertyObserver = fn(&Document, &ElementPropertyChange);

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
    element_properties: BTreeMap<String, String>,
}
/// An arena keeps node handles stable through adoption and foster parenting,
/// and avoids recursive drops for deeply nested input.
#[derive(Clone, Debug)]
pub struct Document {
    pub nodes: Vec<Node>,
    pub root: NodeId,
    pub mode: DocumentMode,
    pub errors: Vec<ParseError>,
    property_observers: Vec<ElementPropertyObserver>,
    property_change_count: u64,
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
                element_properties: BTreeMap::new(),
            }],
            root: 0,
            mode: DocumentMode::NoQuirks,
            errors: vec![],
            property_observers: vec![],
            property_change_count: 0,
        }
    }
    pub fn add(&mut self, kind: NodeKind) -> NodeId {
        let id = self.nodes.len();
        self.nodes.push(Node {
            kind,
            parent: None,
            children: vec![],
            element_properties: BTreeMap::new(),
        });
        id
    }

    /// Subscribe before applying element properties. Observers see committed values.
    pub fn observe_element_properties(&mut self, observer: ElementPropertyObserver) {
        self.property_observers.push(observer);
    }

    pub fn element_property(&self, id: NodeId, name: &str) -> Option<&str> {
        self.nodes
            .get(id)?
            .element_properties
            .get(name)
            .map(String::as_str)
    }

    /// Establish a property's initial value without emitting a change event.
    /// An existing value is left intact; later updates use `set_element_property`.
    pub fn initialize_element_property(
        &mut self,
        id: NodeId,
        name: &str,
        value: &str,
    ) -> Result<bool, ElementPropertyError> {
        if name.is_empty() {
            return Err(ElementPropertyError::EmptyName);
        }
        let node = self
            .nodes
            .get_mut(id)
            .filter(|node| matches!(node.kind, NodeKind::Element { .. }))
            .ok_or(ElementPropertyError::NotElement)?;
        match node.element_properties.entry(name.into()) {
            alloc::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(value.into());
                Ok(true)
            }
            alloc::collections::btree_map::Entry::Occupied(_) => Ok(false),
        }
    }

    /// Set or remove any element property; emit one event only if its value changes.
    /// This stores assigned properties, not browser-computed CSS styles.
    pub fn set_element_property(
        &mut self,
        id: NodeId,
        name: &str,
        value: Option<&str>,
    ) -> Result<bool, ElementPropertyError> {
        if name.is_empty() {
            return Err(ElementPropertyError::EmptyName);
        }
        let node = self
            .nodes
            .get_mut(id)
            .filter(|node| matches!(node.kind, NodeKind::Element { .. }))
            .ok_or(ElementPropertyError::NotElement)?;
        if node.element_properties.get(name).map(String::as_str) == value {
            return Ok(false);
        }
        let previous = match value {
            Some(value) => node.element_properties.insert(name.into(), value.into()),
            None => node.element_properties.remove(name),
        };
        let event = ElementPropertyChange {
            node: id,
            property: name.into(),
            previous,
            value: value.map(String::from),
        };
        self.property_change_count += 1;
        for observer in &self.property_observers {
            observer(self, &event);
        }
        Ok(true)
    }

    pub fn element_property_change_count(&self) -> u64 {
        self.property_change_count
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

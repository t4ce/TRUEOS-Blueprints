//! Apply the debug universal selector through rp5's observable property API.
use rp5::{Document, ElementPropertyChange, NodeKind};
use trueos::logl;

pub(super) const VALUE: &str = "1px solid magenta";
pub(super) const CSS: &str =
    ":where(*) { border: 1px solid magenta; top: 0; left: 0; bottom: 0; right: 0; }\n";

pub(super) fn apply(document: &mut Document) {
    document.observe_element_properties(property_changed);
    let mut pending = vec![document.root];
    while let Some(id) = pending.pop() {
        pending.extend(document.nodes[id].children.iter().rev().copied());
        if matches!(document.nodes[id].kind, NodeKind::Element { .. }) {
            let _ = document.initialize_element_property(id, "border", VALUE);
            for edge in ["top", "left", "bottom", "right"] {
                let _ = document.initialize_element_property(id, edge, "0");
            }
        }
    }
}

fn property_changed(document: &Document, event: &ElementPropertyChange) {
    logl::log(
        logl::level::INFO,
        format_args!(
            "hull: {} changed node={} tag={} old={:?} new={:?}",
            event.property,
            event.node,
            document.name(event.node),
            event.previous,
            event.value
        ),
    );
}

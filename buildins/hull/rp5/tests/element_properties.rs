use rp5::{Document, ElementPropertyChange, ElementPropertyError};
use std::sync::Mutex;

static EVENTS: Mutex<Vec<ElementPropertyChange>> = Mutex::new(Vec::new());

#[test]
fn initialization_is_silent_even_with_an_observer_attached() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static CHANGES: AtomicUsize = AtomicUsize::new(0);
    fn observed(_: &Document, _: &ElementPropertyChange) { CHANGES.fetch_add(1, Ordering::Relaxed); }
    let mut document = rp5::parse("<button>Press</button>");
    let button = document.nodes.iter().enumerate().find(|(id, _)| document.name(*id) == "button").unwrap().0;
    document.observe_element_properties(observed);
    assert!(document.initialize_element_property(button, "border", "1px solid magenta").unwrap());
    for edge in ["top", "left", "bottom", "right"] {
        assert!(document.initialize_element_property(button, edge, "0").unwrap());
        assert_eq!(document.element_property(button, edge), Some("0"));
    }
    assert!(!document.initialize_element_property(button, "top", "10px").unwrap());
    assert_eq!(document.element_property(button, "top"), Some("0"));
    assert_eq!(document.element_property_change_count(), 0);
    assert_eq!(CHANGES.load(Ordering::Relaxed), 0);
    assert!(document.set_element_property(button, "top", Some("10px")).unwrap());
    assert_eq!(document.element_property_change_count(), 1);
    assert_eq!(CHANGES.load(Ordering::Relaxed), 1);
}

fn changed(document: &Document, event: &ElementPropertyChange) {
    assert_eq!(
        document.element_property(event.node, &event.property),
        event.value.as_deref()
    );
    EVENTS.lock().unwrap().push(event.clone());
}

#[test]
fn changes_notify_after_commit_but_getters_and_repeated_values_do_not() {
    let mut document = rp5::parse("<button>Press</button>");
    let button = document
        .nodes
        .iter()
        .enumerate()
        .find(|(id, _)| document.name(*id) == "button")
        .unwrap()
        .0;
    document.observe_element_properties(changed);
    assert_eq!(document.element_property(button, "border"), None);
    assert!(
        document
            .set_element_property(button, "border", Some("1px solid magenta"))
            .unwrap()
    );
    assert!(
        !document
            .set_element_property(button, "border", Some("1px solid magenta"))
            .unwrap()
    );
    assert_eq!(
        document.element_property(button, "border"),
        Some("1px solid magenta")
    );
    assert!(
        document
            .set_element_property(button, "border", Some("2px solid red"))
            .unwrap()
    );
    assert!(
        document
            .set_element_property(button, "border", None)
            .unwrap()
    );
    assert!(
        !document
            .set_element_property(button, "border", None)
            .unwrap()
    );
    assert_eq!(document.element_property_change_count(), 3);
    let events = EVENTS.lock().unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].previous, None);
    assert_eq!(events[1].previous.as_deref(), Some("1px solid magenta"));
    assert_eq!(events[2].value, None);
}

#[test]
fn invalid_targets_do_not_emit_events() {
    let mut document = rp5::parse("<p>Hello</p>");
    assert_eq!(
        document.set_element_property(document.root, "border", Some("1px")),
        Err(ElementPropertyError::NotElement)
    );
    assert_eq!(
        document.set_element_property(usize::MAX, "border", Some("1px")),
        Err(ElementPropertyError::NotElement)
    );
    assert_eq!(
        document.set_element_property(document.root, "", Some("x")),
        Err(ElementPropertyError::EmptyName)
    );
    assert_eq!(document.element_property_change_count(), 0);
}

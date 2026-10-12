//! Host comparison driver; serde_json is a development-only dependency.
use rp5::{Document, DocumentMode, FragmentContext, Namespace, NodeId, NodeKind};
use serde_json::{Value, json};

fn canonical(document: &Document, id: NodeId) -> Value {
    let node = &document.nodes[id];
    let children = || {
        node.children
            .iter()
            .map(|id| canonical(document, *id))
            .collect::<Vec<_>>()
    };
    match &node.kind {
        NodeKind::Document => json!({"type":"document", "mode": match document.mode {
            DocumentMode::NoQuirks => "no-quirks", DocumentMode::Quirks => "quirks", DocumentMode::LimitedQuirks => "limited-quirks"
        }, "children":children()}),
        NodeKind::Fragment => json!({"type":"fragment","children":children()}),
        NodeKind::Text(value) => json!({"type":"text","value":value}),
        NodeKind::Comment(value) => json!({"type":"comment","value":value}),
        NodeKind::Doctype {
            name,
            public_id,
            system_id,
        } => json!({"type":"doctype","name":name,"public_id":public_id,"system_id":system_id}),
        NodeKind::Element {
            name,
            namespace,
            attrs,
            template_content,
        } => {
            let mut attrs = attrs.clone();
            attrs.sort_by(|a, b| {
                (&a.namespace, &a.name, &a.prefix).cmp(&(&b.namespace, &b.name, &b.prefix))
            });
            let mut result = json!({"type":"element","name":name,"namespace":namespace.uri(),"attributes":attrs.iter().map(|a|json!({
                "name":a.name,"namespace":a.namespace.as_deref().unwrap_or(""),"prefix":a.prefix.as_deref().unwrap_or(""),"value":a.value
            })).collect::<Vec<_>>(),"children":children()});
            if let Some(content) = template_content {
                result["template_content"] = canonical(document, *content);
            }
            result
        }
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("expected fixture JSON pathname")?;
    let fixtures: Vec<Value> = serde_json::from_slice(&std::fs::read(path)?)?;
    let mut results = vec![];
    for fixture in fixtures {
        let html = fixture["html"].as_str().ok_or("fixture is missing html")?;
        let document = if fixture["mode"] == "fragment" {
            let context = fixture.get("context").map(|context| FragmentContext {
                name: context["tag"].as_str().unwrap_or("template").into(),
                namespace: match context["namespace"].as_str().unwrap_or("html") {
                    "svg" => Namespace::Svg,
                    "mathml" => Namespace::MathMl,
                    _ => Namespace::Html,
                },
                attrs: vec![],
            });
            rp5::parse_fragment(html, context)
        } else {
            rp5::parse(html)
        };
        results.push(json!({"name":fixture["name"],"tree":canonical(&document,document.root)}));
    }
    println!("{}", serde_json::to_string(&results)?);
    Ok(())
}

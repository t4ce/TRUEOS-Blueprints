//! Initial typed Lightning CSS -> Taffy bridge. No CSS string reparsing.
use super::css::Css;
use lightningcss::properties::{
    Property,
    display::{Display as CssDisplay, DisplayInside, DisplayKeyword},
    size::Size as CssSize,
};
use lightningcss::selector::Selector;
use lightningcss::values::length::{
    LengthPercentage as CssLengthPercentage, LengthPercentageOrAuto, LengthValue,
};
use parcel_selectors::parser::Component;
use rp5::{Document, NodeId as HtmlId, NodeKind};
use std::fmt::Write;
use taffy::prelude::*;
use taffy::tree::LayoutOutput;

pub(super) struct Export {
    pub json: String,
    pub svg: String,
}

pub(super) fn compute(
    document: &Document,
    css: &Css,
    viewport: (u32, u32),
) -> Result<Export, Box<dyn std::error::Error>> {
    let mut tree: TaffyTree<String> = TaffyTree::new();
    let mut ids = vec![None; document.nodes.len()];
    let mut styles = vec![Style::default(); document.nodes.len()];
    let mut pending = vec![(document.root, false)];
    while let Some((id, visited)) = pending.pop() {
        if !visited {
            pending.push((id, true));
            pending.extend(
                document.nodes[id]
                    .children
                    .iter()
                    .rev()
                    .map(|&child| (child, false)),
            );
            continue;
        }
        let node = &document.nodes[id];
        let mut style = Style {
            display: Display::Block,
            box_sizing: BoxSizing::ContentBox,
            ..Style::default()
        };
        let taffy_id = match &node.kind {
            NodeKind::Element { name, attrs, .. } => {
                if document.element_property(id, "border").is_some() {
                    style.border = Rect {
                        top: LengthPercentage::from_length(1.0),
                        left: LengthPercentage::from_length(1.0),
                        bottom: LengthPercentage::from_length(1.0),
                        right: LengthPercentage::from_length(1.0),
                    };
                }
                style.inset = Rect {
                    top: LengthPercentageAuto::from_length(0.0),
                    left: LengthPercentageAuto::from_length(0.0),
                    bottom: LengthPercentageAuto::from_length(0.0),
                    right: LengthPercentageAuto::from_length(0.0),
                };
                apply_css(document, id, css, &mut style);
                if matches!(
                    name.as_str(),
                    "head" | "script" | "style" | "meta" | "link" | "title" | "base" | "template"
                ) || attrs.iter().any(|attr| attr.name == "hidden")
                {
                    style.display = Display::None;
                }
                validate_style(&style, id)?;
                let children: Vec<_> = node
                    .children
                    .iter()
                    .filter_map(|&child| ids[child])
                    .collect();
                tree.new_with_children(style.clone(), &children)?
            }
            NodeKind::Text(text) if !text.trim().is_empty() => tree.new_leaf_with_context(
                style.clone(),
                text.split_whitespace().collect::<Vec<_>>().join(" "),
            )?,
            NodeKind::Document | NodeKind::Fragment => {
                style.size.width = Dimension::from_length(viewport.0 as f32);
                validate_style(&style, id)?;
                let children: Vec<_> = node
                    .children
                    .iter()
                    .filter_map(|&child| ids[child])
                    .collect();
                tree.new_with_children(style.clone(), &children)?
            }
            _ => continue,
        };
        styles[id] = style;
        ids[id] = Some(taffy_id);
    }
    let root = ids[document.root].unwrap();
    tree.compute_layout_with_measure(
        root,
        Size {
            width: AvailableSpace::Definite(viewport.0 as f32),
            height: AvailableSpace::Definite(viewport.1 as f32),
        },
        |input, _, text, _| {
            let text = text.map(|text| text.as_str()).unwrap_or("");
            let natural = text.chars().count() as f32 * 8.0;
            let limit =
                input
                    .known_dimensions
                    .width
                    .unwrap_or_else(|| match input.available_space.width {
                        AvailableSpace::Definite(width) => width,
                        AvailableSpace::MinContent => {
                            text.split_whitespace()
                                .map(|word| word.chars().count())
                                .max()
                                .unwrap_or(0) as f32
                                * 8.0
                        }
                        AvailableSpace::MaxContent => natural,
                    });
            let width = input
                .known_dimensions
                .width
                .unwrap_or(natural.min(limit.max(1.0)));
            let height = input.known_dimensions.height.unwrap_or(if natural == 0.0 {
                0.0
            } else {
                (natural / width.max(1.0)).ceil() * 16.0
            });
            LayoutOutput::from_outer_size(Size { width, height })
        },
    )?;
    let mut rows = Vec::new();
    let mut rectangles = Vec::new();
    let mut pending = vec![(document.root, 0.0f32, 0.0f32, false)];
    while let Some((id, parent_x, parent_y, parent_hidden)) = pending.pop() {
        let Some(tid) = ids[id] else { continue };
        let layout = tree.layout(tid)?;
        let left = parent_x + layout.location.x;
        let top = parent_y + layout.location.y;
        let hidden = parent_hidden || styles[id].display == Display::None;
        if matches!(
            document.nodes[id].kind,
            NodeKind::Element { .. } | NodeKind::Text(_)
        ) {
            let tag = if matches!(document.nodes[id].kind, NodeKind::Text(_)) {
                "#text"
            } else {
                document.name(id)
            };
            let rect = if hidden {
                serde_json::Value::Null
            } else {
                serde_json::json!({"left":left,"top":top,"right":left+layout.size.width,"bottom":top+layout.size.height,"width":layout.size.width,"height":layout.size.height})
            };
            rows.push(serde_json::json!({"node":id,"tag":tag,"rect":rect,"hidden":hidden}));
            if !hidden && matches!(document.nodes[id].kind, NodeKind::Element { .. }) {
                rectangles.push((id, left, top, layout.size.width, layout.size.height));
            }
        }
        pending.extend(
            document.nodes[id]
                .children
                .iter()
                .rev()
                .map(|&child| (child, left, top, hidden)),
        );
    }
    let unsupported_rules = css
        .sheets
        .iter()
        .flat_map(|sheet| &sheet.rules.0)
        .filter(|rule| {
            !matches!(
                rule,
                lightningcss::rules::CssRule::Style(_) | lightningcss::rules::CssRule::Import(_)
            )
        })
        .count();
    let json = serde_json::to_string_pretty(
        &serde_json::json!({"engine":"taffy","viewport":{"width":viewport.0,"height":viewport.1},"coordinates":"absolute page pixels; right/bottom are rectangle edges, not CSS insets","limitations":["approximate text: 8px characters, 16px line height; no font shaping or inline formatting","compound type/id/class/universal selectors only; no combinators or dynamic pseudo-classes","no media queries, inheritance, variables, calc, grid track mapping or full browser cascade; em/rem use an estimated 16px font","external sheets retain download-completion order"],"unsupported_rule_count":unsupported_rules,"nodes":rows}),
    )?;
    let extent = tree.layout(root)?.size.height.max(viewport.1 as f32);
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\"><rect width=\"100%\" height=\"100%\" fill=\"white\"/>",
        viewport.0, extent, viewport.0, extent
    );
    for (id, left, top, width, height) in rectangles {
        let _ = write!(
            svg,
            "<rect x=\"{left}\" y=\"{top}\" width=\"{width}\" height=\"{height}\" fill=\"none\" stroke=\"magenta\" stroke-width=\"1\"><title>node {id}</title></rect>"
        );
    }
    svg.push_str("</svg>\n");
    Ok(Export { json, svg })
}

fn matches_selector(document: &Document, id: HtmlId, selector: &Selector<'_>) -> bool {
    let Some((name, _, attrs)) = document.element(id) else {
        return false;
    };
    selector
        .iter_raw_match_order()
        .all(|component| match component {
            Component::ExplicitUniversalType => true,
            Component::LocalName(local) => name == local.name.as_ref(),
            Component::ID(value) => attrs
                .iter()
                .any(|attr| attr.name == "id" && attr.value == value.as_ref()),
            Component::Class(value) => attrs.iter().any(|attr| {
                attr.name == "class"
                    && attr
                        .value
                        .split_ascii_whitespace()
                        .any(|class| class == value.as_ref())
            }),
            Component::Where(selectors) | Component::Is(selectors) => selectors
                .iter()
                .any(|selector| matches_selector(document, id, selector)),
            _ => false,
        })
}

fn apply_css(document: &Document, id: HtmlId, css: &Css, style: &mut Style) {
    let mut declarations = Vec::new();
    let mut order = 0;
    for sheet in &css.sheets {
        for rule in &sheet.rules.0 {
            if let lightningcss::rules::CssRule::Style(rule) = rule {
                if let Some(specificity) = rule
                    .selectors
                    .0
                    .iter()
                    .filter(|selector| matches_selector(document, id, selector))
                    .map(|selector| selector.specificity())
                    .max()
                {
                    for (important, properties) in [
                        (false, &rule.declarations.declarations),
                        (true, &rule.declarations.important_declarations),
                    ] {
                        for property in properties {
                            declarations.push((important, false, specificity, order, property));
                            order += 1;
                        }
                    }
                }
            }
        }
    }
    for (node, attribute) in &css.attributes {
        if *node == id {
            for (important, properties) in [
                (false, &attribute.declarations.declarations),
                (true, &attribute.declarations.important_declarations),
            ] {
                for property in properties {
                    declarations.push((important, true, 0, order, property));
                    order += 1;
                }
            }
        }
    }
    declarations.sort_by_key(|&(important, inline, specificity, order, _)| {
        (important, inline, specificity, order)
    });
    let mut positioned = false;
    for (_, _, _, _, property) in declarations {
        if let Property::Position(value) = property {
            positioned = !matches!(value, lightningcss::properties::position::Position::Static);
        }
        apply_property(style, property);
    }
    if !positioned {
        style.inset = Rect {
            top: LengthPercentageAuto::AUTO,
            left: LengthPercentageAuto::AUTO,
            bottom: LengthPercentageAuto::AUTO,
            right: LengthPercentageAuto::AUTO,
        };
    }
}

fn raw_length(value: &CssLengthPercentage) -> Option<taffy::style::LengthPercentage> {
    match value {
        CssLengthPercentage::Dimension(LengthValue::Px(value)) => {
            Some(taffy::style::LengthPercentage::from_length(*value))
        }
        CssLengthPercentage::Dimension(LengthValue::Rem(value) | LengthValue::Em(value)) => {
            Some(taffy::style::LengthPercentage::from_length(*value * 16.0))
        }
        CssLengthPercentage::Percentage(value) => {
            Some(taffy::style::LengthPercentage::from_percent(value.0))
        }
        _ => None,
    }
}
fn length(value: &LengthPercentageOrAuto) -> Option<taffy::style::LengthPercentage> {
    match value {
        LengthPercentageOrAuto::LengthPercentage(value) => raw_length(value),
        _ => None,
    }
}
fn minimum(value: &CssSize) -> Option<LengthPercentageAuto> {
    match value {
        CssSize::Auto => Some(LengthPercentageAuto::AUTO),
        CssSize::LengthPercentage(value) => {
            auto(&LengthPercentageOrAuto::LengthPercentage(value.clone()))
        }
        _ => None,
    }
}
fn auto(value: &LengthPercentageOrAuto) -> Option<LengthPercentageAuto> {
    match value {
        LengthPercentageOrAuto::Auto => Some(LengthPercentageAuto::AUTO),
        LengthPercentageOrAuto::LengthPercentage(CssLengthPercentage::Percentage(value)) => {
            Some(LengthPercentageAuto::from_percent(value.0))
        }
        LengthPercentageOrAuto::LengthPercentage(CssLengthPercentage::Dimension(
            LengthValue::Px(value),
        )) => Some(LengthPercentageAuto::from_length(*value)),
        LengthPercentageOrAuto::LengthPercentage(CssLengthPercentage::Dimension(
            LengthValue::Rem(value) | LengthValue::Em(value),
        )) => Some(LengthPercentageAuto::from_length(*value * 16.0)),
        _ => None,
    }
}
fn dimension(value: &CssSize) -> Option<Dimension> {
    match value {
        CssSize::Auto => Some(Dimension::AUTO),
        CssSize::LengthPercentage(CssLengthPercentage::Percentage(value)) => {
            Some(Dimension::from_percent(value.0))
        }
        CssSize::LengthPercentage(CssLengthPercentage::Dimension(LengthValue::Px(value))) => {
            Some(Dimension::from_length(*value))
        }
        CssSize::LengthPercentage(CssLengthPercentage::Dimension(
            LengthValue::Rem(value) | LengthValue::Em(value),
        )) => Some(Dimension::from_length(*value * 16.0)),
        _ => None,
    }
}
fn apply_property(style: &mut Style, property: &Property<'_>) {
    macro_rules! set {
        ($target:expr,$value:expr,$convert:ident) => {
            if let Some(value) = $convert($value) {
                $target = value;
            }
        };
    }
    match property {
        Property::Width(value) => set!(style.size.width, value, dimension),
        Property::Height(value) => set!(style.size.height, value, dimension),
        Property::MinWidth(value) => set!(style.min_size.width, value, minimum),
        Property::MinHeight(value) => set!(style.min_size.height, value, minimum),
        Property::Margin(value) => {
            set!(style.margin.top, &value.top, auto);
            set!(style.margin.left, &value.left, auto);
            set!(style.margin.bottom, &value.bottom, auto);
            set!(style.margin.right, &value.right, auto);
        }
        Property::MarginTop(value) => set!(style.margin.top, value, auto),
        Property::MarginLeft(value) => set!(style.margin.left, value, auto),
        Property::MarginBottom(value) => set!(style.margin.bottom, value, auto),
        Property::MarginRight(value) => set!(style.margin.right, value, auto),
        Property::Padding(value) => {
            set!(style.padding.top, &value.top, length);
            set!(style.padding.left, &value.left, length);
            set!(style.padding.bottom, &value.bottom, length);
            set!(style.padding.right, &value.right, length);
        }
        Property::PaddingTop(value) => set!(style.padding.top, value, length),
        Property::PaddingLeft(value) => set!(style.padding.left, value, length),
        Property::PaddingBottom(value) => set!(style.padding.bottom, value, length),
        Property::PaddingRight(value) => set!(style.padding.right, value, length),
        Property::Top(value) => set!(style.inset.top, value, auto),
        Property::Left(value) => set!(style.inset.left, value, auto),
        Property::Bottom(value) => set!(style.inset.bottom, value, auto),
        Property::Right(value) => set!(style.inset.right, value, auto),
        Property::Display(CssDisplay::Keyword(DisplayKeyword::None)) => {
            style.display = Display::None
        }
        Property::Display(CssDisplay::Pair(value)) => {
            style.display = match value.inside {
                DisplayInside::Flex(_) => Display::Flex,
                DisplayInside::Grid => Display::Grid,
                _ => Display::Block,
            }
        }
        Property::Position(value) => {
            style.position = match value {
                lightningcss::properties::position::Position::Absolute
                | lightningcss::properties::position::Position::Fixed => Position::Absolute,
                _ => Position::Relative,
            }
        }
        Property::FlexGrow(value, _) => style.flex_grow = *value,
        Property::FlexShrink(value, _) => style.flex_shrink = *value,
        Property::FlexDirection(value, _) => {
            style.flex_direction = match value {
                lightningcss::properties::flex::FlexDirection::Row => FlexDirection::Row,
                lightningcss::properties::flex::FlexDirection::Column => FlexDirection::Column,
                lightningcss::properties::flex::FlexDirection::RowReverse => {
                    FlexDirection::RowReverse
                }
                lightningcss::properties::flex::FlexDirection::ColumnReverse => {
                    FlexDirection::ColumnReverse
                }
            }
        }
        _ => {}
    }
}

fn validate_style(style: &Style, node: HtmlId) -> Result<(), Box<dyn std::error::Error>> {
    let auto = [
        ("width", style.size.width.into_raw().tag()),
        ("height", style.size.height.into_raw().tag()),
        ("min-width", style.min_size.width.into_raw().tag()),
        ("min-height", style.min_size.height.into_raw().tag()),
        ("max-width", style.max_size.width.into_raw().tag()),
        ("max-height", style.max_size.height.into_raw().tag()),
        ("margin-top", style.margin.top.into_raw().tag()),
        ("margin-left", style.margin.left.into_raw().tag()),
        ("margin-bottom", style.margin.bottom.into_raw().tag()),
        ("margin-right", style.margin.right.into_raw().tag()),
        ("top", style.inset.top.into_raw().tag()),
        ("left", style.inset.left.into_raw().tag()),
        ("bottom", style.inset.bottom.into_raw().tag()),
        ("right", style.inset.right.into_raw().tag()),
    ];
    for (property, tag) in auto {
        if ![
            taffy::CompactLength::LENGTH_TAG,
            taffy::CompactLength::PERCENT_TAG,
            taffy::CompactLength::AUTO_TAG,
        ]
        .contains(&tag)
        {
            return Err(format!(
                "invalid Taffy dimension node={node} property={property} tag={tag}"
            )
            .into());
        }
    }
    Ok(())
}

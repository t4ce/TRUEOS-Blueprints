//! Initial typed Lightning CSS -> Taffy bridge. No CSS string reparsing.
use super::css::Css;
use lightningcss::properties::{
    Property,
    display::{Display as CssDisplay, DisplayInside, DisplayKeyword},
    font::{
        AbsoluteFontSize, FontSize as CssFontSize, LineHeight as CssLineHeight, RelativeFontSize,
    },
    size::Size as CssSize,
};
use lightningcss::selector::Selector;
use lightningcss::values::length::{
    LengthPercentage as CssLengthPercentage, LengthPercentageOrAuto, LengthValue,
};
use parcel_selectors::parser::Component;
use rp5::{Document, NodeId as HtmlId, NodeKind};
use std::collections::HashMap;
use std::fmt::Write;
use taffy::prelude::*;

pub(super) struct Export {
    pub json: String,
    pub svg: String,
    pub line_rects: Vec<[f32; 4]>,
    pub png_size: (u32, u32),
}

pub(super) fn compute(
    document: &Document,
    css: &Css,
    viewport: (u32, u32),
) -> Result<Export, Box<dyn std::error::Error>> {
    let mut tree: TaffyTree<TextMeasureContext> = TaffyTree::new();
    let mut ids: Vec<Vec<NodeId>> = vec![vec![]; document.nodes.len()];
    let mut owners = HashMap::new();
    let mut inline = vec![false; document.nodes.len()];
    let mut contents = vec![false; document.nodes.len()];
    let mut hidden = vec![false; document.nodes.len()];
    let mut styles = vec![Style::default(); document.nodes.len()];
    let mut text_metrics = vec![TextMetrics::default(); document.nodes.len()];
    let mut root_font_size = 11.0;
    let mut pending = vec![document.root];
    while let Some(id) = pending.pop() {
        let node = &document.nodes[id];
        let inherited = node
            .parent
            .map(|parent| text_metrics[parent])
            .unwrap_or_default();
        let mut metrics = inherited;
        inline[id] = matches!(&node.kind, NodeKind::Text(_))
            || document
                .element(id)
                .is_some_and(|(name, _, _)| default_inline(name));
        if document.name(id) == "pre" {
            metrics.whitespace = WhiteSpaceMetric::Pre;
        }
        let mut style = Style {
            display: Display::Block,
            box_sizing: BoxSizing::ContentBox,
            ..Style::default()
        };
        match &node.kind {
            NodeKind::Element { name, attrs, .. } => {
                let context = ResolveContext {
                    viewport,
                    font_size: inherited.font_size,
                    root_font_size,
                };
                apply_css(
                    document,
                    id,
                    css,
                    &mut style,
                    &mut metrics,
                    context,
                    &mut inline[id],
                    &mut contents[id],
                );
                if name == "html" {
                    root_font_size = metrics.font_size;
                }
                if matches!(
                    name.as_str(),
                    "head" | "script" | "style" | "meta" | "link" | "title" | "base" | "template"
                ) || attrs.iter().any(|attr| attr.name == "hidden")
                {
                    style.display = Display::None;
                }
            }
            NodeKind::Document | NodeKind::Fragment => {
                style.size.width = Dimension::from_length(viewport.0 as f32);
            }
            _ => {}
        }
        hidden[id] =
            style.display == Display::None || node.parent.is_some_and(|parent| hidden[parent]);
        styles[id] = style;
        text_metrics[id] = metrics;
        pending.extend(node.children.iter().rev().copied());
    }

    // Anonymous flex line boxes provide a first inline-flow implementation.
    // Word fragments retain their HTML owner while anonymous boxes stay out of the DOM.
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
        match &node.kind {
            NodeKind::Text(text) if !hidden[id] => {
                for text in text_chunks(text, text_metrics[id].whitespace) {
                    let style = Style {
                        display: Display::Block,
                        flex_shrink: 0.0,
                        ..Style::default()
                    };
                    let tid = tree.new_leaf_with_context(
                        style,
                        TextMeasureContext {
                            text,
                            metrics: text_metrics[id],
                        },
                    )?;
                    owners.insert(tid, id);
                    ids[id].push(tid);
                }
            }
            NodeKind::Element { .. } | NodeKind::Document | NodeKind::Fragment => {
                let mut style = styles[id].clone();
                let mut children = Vec::new();
                let mut run = Vec::new();
                let is_line_container = inline[id] && style.display == Display::Block;
                if is_line_container {
                    style.display = Display::Flex;
                    style.flex_wrap = FlexWrap::Wrap;
                    style.flex_shrink = 0.0;
                    style.align_items = Some(AlignItems::Start);
                    style.justify_content = Some(text_metrics[id].align.justify());
                }
                for &child in &node.children {
                    if contents[id] || is_line_container {
                        children.extend(ids[child].iter().copied());
                    } else if style.display != Display::Block {
                        if matches!(document.nodes[child].kind, NodeKind::Text(_)) {
                            run.extend(ids[child].iter().copied());
                        } else {
                            flush_inline_run(
                                &mut tree,
                                &mut children,
                                &mut run,
                                text_metrics[id],
                                false,
                            )?;
                            children.extend(ids[child].iter().copied());
                        }
                    } else if inline[child] {
                        run.extend(ids[child].iter().copied());
                    } else {
                        flush_inline_run(
                            &mut tree,
                            &mut children,
                            &mut run,
                            text_metrics[id],
                            style.display == Display::Block,
                        )?;
                        children.extend(ids[child].iter().copied());
                    }
                }
                flush_inline_run(
                    &mut tree,
                    &mut children,
                    &mut run,
                    text_metrics[id],
                    style.display == Display::Block,
                )?;
                if contents[id] {
                    ids[id] = children;
                } else {
                    let tid = tree.new_with_children(style, &children)?;
                    owners.insert(tid, id);
                    ids[id].push(tid);
                }
            }
            _ => {}
        }
    }
    let root = ids[document.root][0];
    tree.compute_layout_with_measure(
        root,
        Size {
            width: AvailableSpace::Definite(viewport.0 as f32),
            height: AvailableSpace::Definite(viewport.1 as f32),
        },
        |known_dimensions, available_space, _, context, _| {
            let fallback = TextMetrics::default();
            let (text, metrics) = context
                .as_deref()
                .map(|context| (context.text.as_str(), context.metrics))
                .unwrap_or(("", fallback));
            let natural = if matches!(
                metrics.whitespace,
                WhiteSpaceMetric::Pre | WhiteSpaceMetric::PreWrap
            ) {
                text.split('\n')
                    .map(|line| estimate_text_width(line, metrics.font_size))
                    .fold(0.0, f32::max)
            } else {
                estimate_text_width(text, metrics.font_size)
            };
            let limit = known_dimensions
                .width
                .unwrap_or_else(|| match available_space.width {
                    AvailableSpace::Definite(width) => width,
                    AvailableSpace::MinContent => text
                        .split_whitespace()
                        .map(|word| estimate_text_width(word, metrics.font_size))
                        .fold(0.0f32, f32::max),
                    AvailableSpace::MaxContent => natural,
                });
            let width = known_dimensions
                .width
                .unwrap_or(natural.min(limit.max(1.0)));
            let height = known_dimensions.height.unwrap_or(if natural == 0.0 {
                0.0
            } else {
                (match metrics.whitespace {
                    WhiteSpaceMetric::Pre => text.split('\n').count() as f32,
                    WhiteSpaceMetric::NoWrap => 1.0,
                    WhiteSpaceMetric::PreWrap => text
                        .split('\n')
                        .map(|line| estimated_line_count(line, width, metrics.font_size).max(1.0))
                        .sum(),
                    WhiteSpaceMetric::Normal => {
                        estimated_line_count(text, width, metrics.font_size)
                    }
                }) * metrics.line_height.used(metrics.font_size)
            });
            Size { width, height }
        },
    )?;
    let mut fragments: Vec<Vec<BoxRect>> = vec![vec![]; document.nodes.len()];
    let mut text_svg = String::new();
    let mut pending = vec![(root, 0.0f32, 0.0f32)];
    while let Some((tid, parent_x, parent_y)) = pending.pop() {
        let box_layout = tree.layout(tid)?;
        let left = parent_x + box_layout.location.x;
        let top = parent_y + box_layout.location.y;
        if let Some(&id) = owners.get(&tid) {
            if !hidden[id] && !contents[id] {
                fragments[id].push(BoxRect {
                    left,
                    top,
                    width: box_layout.size.width,
                    height: box_layout.size.height,
                });
                if let Some(context) = tree.get_node_context(tid) {
                    let line_height = context.metrics.line_height.used(context.metrics.font_size);
                    for (line_index, line) in visual_lines(context, box_layout.size.width)
                        .iter()
                        .enumerate()
                    {
                        let y = top
                            + line_index as f32 * line_height
                            + (line_height - context.metrics.font_size) / 2.0
                            + context.metrics.font_size * 0.8;
                        let length = estimate_text_width(line, context.metrics.font_size);
                        let _ = write!(
                            text_svg,
                            "<text x=\"{left}\" y=\"{y}\" font-size=\"{}\" font-family=\"monospace\" xml:space=\"preserve\" textLength=\"{length}\" lengthAdjust=\"spacingAndGlyphs\">{}</text>",
                            context.metrics.font_size,
                            xml_escape(line)
                        );
                    }
                }
            }
        }
        pending.extend(
            tree.children(tid)?
                .into_iter()
                .rev()
                .map(|child| (child, left, top)),
        );
    }
    let mut rows = Vec::new();
    let mut rectangles = Vec::new();
    let mut pending = vec![document.root];
    while let Some(id) = pending.pop() {
        if matches!(
            document.nodes[id].kind,
            NodeKind::Element { .. } | NodeKind::Text(_)
        ) {
            let tag = if matches!(document.nodes[id].kind, NodeKind::Text(_)) {
                "#text"
            } else {
                document.name(id)
            };
            let rect = BoxRect::bounds(&fragments[id]);
            rows.push(serde_json::json!({"node":id,"tag":tag,"rect":rect.as_ref().map(BoxRect::json),"hidden":hidden[id],"display_contents":contents[id],"text_metrics":{"font_size":text_metrics[id].font_size,"advance":text_metrics[id].font_size*6.0/11.0,"line_height":text_metrics[id].line_height.used(text_metrics[id].font_size)},"fragments":fragments[id].iter().map(BoxRect::json).collect::<Vec<_>>() }));
            if matches!(document.nodes[id].kind, NodeKind::Element { .. }) {
                if let Some(rect) = rect {
                    rectangles.push((id, rect.left, rect.top, rect.width, rect.height));
                }
            }
        }
        pending.extend(document.nodes[id].children.iter().rev().copied());
    }
    let unsupported_rules = css
        .sheets
        .iter()
        .flat_map(|sheet| &sheet.rules.0)
        .filter(|rule| {
            !matches!(
                rule,
                lightningcss::rules::CssRule::Style(_)
                    | lightningcss::rules::CssRule::Import(_)
                    | lightningcss::rules::CssRule::Media(_)
            )
        })
        .count();
    let mut unused = std::collections::BTreeSet::new();
    for sheet in &css.sheets {
        unused_properties(&sheet.rules.0, &mut unused);
    }
    for (_, attribute) in &css.attributes {
        for property in attribute
            .declarations
            .declarations
            .iter()
            .chain(&attribute.declarations.important_declarations)
        {
            let name = property.property_id().name().to_owned();
            if !used_for_layout(&name) {
                unused.insert(name);
            }
        }
    }
    let json = serde_json::to_string_pretty(
        &serde_json::json!({"engine":"taffy","engine_version":"0.7.7","viewport":{"width":viewport.0,"height":viewport.1},"units":"layout units; CSS lengths currently map 1:1 for compatibility; SVG uses the same numeric coordinates","coordinates":"absolute layout units; right/bottom are rectangle edges, not CSS insets","text_metrics":"synthetic monospace cell metrics: 6 layout units advance by 11 layout units line height at 11-unit default font-size, scaled by inherited CSS font-size and line-height; no font shaping","limitations":["text widths and wrapping use synthetic monospace metrics; no font shaping","dynamic pseudo-classes, pseudo-elements, :has and namespace selectors are not implemented","media conditions cover viewport sizes and orientation; device preference queries, variables, calc, grid tracks, cascade layers and supports conditions are not implemented","inline flow uses approximate flex line boxes; nested inline boxes may wrap as a single item; no complete browser default stylesheet"],"unsupported_rule_count":unsupported_rules,"properties_not_used_for_layout":unused,"nodes":rows}),
    )?;
    let extent = tree.layout(root)?.size.height.max(viewport.1 as f32);
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\"><rect width=\"100%\" height=\"100%\" fill=\"white\"/>",
        viewport.0, extent, viewport.0, extent
    );
    let line_rects = rectangles
        .iter()
        .map(|&(_, x, y, w, h)| [x, y, w, h])
        .collect();
    for (id, left, top, width, height) in rectangles {
        let _ = write!(
            svg,
            "<rect x=\"{left}\" y=\"{top}\" width=\"{width}\" height=\"{height}\" fill=\"none\" stroke=\"magenta\" stroke-width=\"1\"><title>node {id}</title></rect>"
        );
    }
    svg.push_str(&text_svg);
    svg.push_str("</svg>\n");
    Ok(Export {
        json,
        svg,
        line_rects,
        png_size: (viewport.0, extent.ceil() as u32),
    })
}

fn matches_selector(document: &Document, id: HtmlId, selector: &Selector<'_>) -> bool {
    use lightningcss::selector::PseudoClass;
    use parcel_selectors::parser::Combinator;
    let components: Vec<_> = selector.iter_raw_match_order().collect();
    let mut pending = vec![(id, 0)];
    while let Some((id, start)) = pending.pop() {
        let Some((name, namespace, attrs)) = document.element(id) else {
            continue;
        };
        let attr = |name: &str| {
            attrs
                .iter()
                .find(|attr| attr.namespace.is_none() && attr.name == name)
                .map(|attr| attr.value.as_str())
        };
        let mut matched = true;
        for (offset, component) in components.iter().enumerate().skip(start) {
            if let Component::Combinator(combinator) = component {
                let next = offset + 1;
                match combinator {
                    Combinator::Child => {
                        if let Some(parent) = document.nodes[id]
                            .parent
                            .filter(|&parent| document.element(parent).is_some())
                        {
                            pending.push((parent, next));
                        }
                    }
                    Combinator::Descendant => {
                        let mut parent = document.nodes[id].parent;
                        while let Some(id) = parent {
                            if document.element(id).is_some() {
                                pending.push((id, next));
                            }
                            parent = document.nodes[id].parent;
                        }
                    }
                    Combinator::NextSibling | Combinator::LaterSibling => {
                        if let Some(parent) = document.nodes[id].parent {
                            let siblings = &document.nodes[parent].children;
                            if let Some(index) = siblings.iter().position(|&sibling| sibling == id)
                            {
                                for &sibling in siblings[..index]
                                    .iter()
                                    .rev()
                                    .filter(|&&sibling| document.element(sibling).is_some())
                                {
                                    pending.push((sibling, next));
                                    if matches!(combinator, Combinator::NextSibling) {
                                        break;
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
                matched = false;
                break;
            }
            let ok=match component {
                Component::ExplicitUniversalType|Component::ExplicitAnyNamespace=>true,
                Component::LocalName(local)=>name==local.name.as_ref(),
                Component::ID(value)=>attr("id")==Some(value.as_ref()),
                Component::Class(value)=>attr("class").is_some_and(|classes|classes.split_ascii_whitespace().any(|class|class==value.as_ref())),
                Component::AttributeInNoNamespaceExists{local_name,..}=>attr(local_name.as_ref()).is_some(),
                Component::AttributeInNoNamespace{local_name,operator,value,case_sensitivity,never_matches,..}=>!never_matches && attr(local_name.as_ref()).is_some_and(|actual|operator.eval_str(actual,value.as_ref(),case_sensitivity.to_unconditional(namespace==rp5::Namespace::Html))),
                Component::Where(selectors)|Component::Is(selectors)=>selectors.iter().any(|selector|matches_selector(document,id,selector)),
                Component::Negation(selectors)=>!selectors.iter().any(|selector|matches_selector(document,id,selector)),
                Component::Root=>document.nodes[id].parent==Some(document.root),
                Component::Empty=>document.nodes[id].children.iter().all(|&child|matches!(&document.nodes[child].kind,NodeKind::Comment(_)) || matches!(&document.nodes[child].kind,NodeKind::Text(text) if text.is_empty())),
                Component::Nth(nth)=>{
                    let siblings:Vec<_>=document.nodes[id].parent.map(|parent|document.nodes[parent].children.iter().copied().filter(|&child|document.element(child).is_some() && (!nth.ty.is_of_type() || (document.name(child)==name && document.namespace(child)==namespace))).collect()).unwrap_or_default();
                    if nth.ty.is_only(){siblings.len()==1}else if let Some(index)=siblings.iter().position(|&child|child==id){
                        let position=if nth.ty.is_from_end(){siblings.len()-index}else{index+1} as i32;
                        let delta=position-nth.b;
                        if nth.a==0 {delta==0}else{delta%nth.a==0 && delta/nth.a>=0}
                    }else{false}
                }
                Component::NonTSPseudoClass(pseudo)=>match pseudo {
                    PseudoClass::Link|PseudoClass::AnyLink(_)=>matches!(name,"a"|"area"|"link") && attr("href").is_some(),
                    PseudoClass::Disabled=>attr("disabled").is_some(),
                    PseudoClass::Enabled=>matches!(name,"button"|"input"|"select"|"textarea"|"option"|"optgroup") && attr("disabled").is_none(),
                    PseudoClass::Checked=>attr("checked").is_some() || (name=="option" && attr("selected").is_some()),
                    _=>false,
                },
                _=>false,
            };
            if !ok {
                matched = false;
                break;
            }
        }
        if matched {
            return true;
        }
    }
    false
}

#[derive(Clone, Copy)]
struct TextMetrics {
    font_size: f32,
    line_height: LineHeightMetric,
    align: TextAlignMetric,
    whitespace: WhiteSpaceMetric,
}

struct TextMeasureContext {
    text: String,
    metrics: TextMetrics,
}

#[derive(Clone, Copy)]
enum LineHeightMetric {
    Normal,
    Number(f32),
    Absolute(f32),
}

impl LineHeightMetric {
    fn used(self, font_size: f32) -> f32 {
        match self {
            Self::Normal => font_size,
            Self::Number(value) => font_size * value,
            Self::Absolute(value) => value,
        }
    }
}

impl Default for TextMetrics {
    fn default() -> Self {
        Self {
            font_size: 11.0,
            line_height: LineHeightMetric::Normal,
            align: TextAlignMetric::Start,
            whitespace: WhiteSpaceMetric::Normal,
        }
    }
}

pub(super) fn estimate_text_width(text: &str, font_size: f32) -> f32 {
    text.chars().count() as f32 * font_size * (6.0 / 11.0)
}

fn estimated_line_count(text: &str, width: f32, font_size: f32) -> f32 {
    let advance = font_size * (6.0 / 11.0);
    if advance <= 0.0 {
        return 0.0;
    }
    let columns = (width / advance).floor().max(1.0) as usize;
    let mut lines = 0usize;
    let mut used = 0usize;
    for word in text.split(is_css_space).filter(|word| !word.is_empty()) {
        let count = word.chars().count();
        if used > 0 && used + 1 + count > columns {
            lines += 1;
            used = 0;
        }
        if count > columns {
            lines += (count - 1) / columns;
            used = (count - 1) % columns + 1;
        } else {
            if used > 0 {
                used += 1;
            }
            used += count;
        }
    }
    (lines + usize::from(used > 0)) as f32
}

fn apply_css(
    document: &Document,
    id: HtmlId,
    css: &Css,
    style: &mut Style,
    text_metrics: &mut TextMetrics,
    context: ResolveContext,
    inline: &mut bool,
    contents: &mut bool,
) {
    let mut declarations = Vec::new();
    let mut order = 0;
    if css.roots.is_empty() {
        for index in 0..css.sheets.len() {
            collect_sheet(
                css,
                index,
                document,
                id,
                context,
                &mut declarations,
                &mut order,
                &mut Vec::new(),
            );
        }
    } else {
        for root in &css.roots {
            if root
                .media
                .as_ref()
                .is_some_and(|media| !media_matches(media, context))
            {
                continue;
            }
            let index = match &root.source {
                super::css::SheetReference::Embedded(index) => Some(*index),
                super::css::SheetReference::External(url) => css.external_sheets.get(url).copied(),
            };
            if let Some(index) = index {
                collect_sheet(
                    css,
                    index,
                    document,
                    id,
                    context,
                    &mut declarations,
                    &mut order,
                    &mut Vec::new(),
                );
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
    let mut border_visible = [true; 4];
    let mut font_size_winner: Option<&CssFontSize> = None;
    let mut line_height_winner: Option<&CssLineHeight> = None;
    for (_, _, _, _, property) in &declarations {
        if let Property::Position(value) = property {
            positioned = !matches!(value, lightningcss::properties::position::Position::Static);
        }
        match property {
            Property::FontSize(value) => font_size_winner = Some(value),
            Property::LineHeight(value) => line_height_winner = Some(value),
            Property::Display(CssDisplay::Pair(value)) => {
                *inline = matches!(
                    value.outside,
                    lightningcss::properties::display::DisplayOutside::Inline
                );
                *contents = false;
            }
            Property::Display(CssDisplay::Keyword(DisplayKeyword::Contents)) => {
                *contents = true;
            }
            Property::TextAlign(value) => {
                use lightningcss::properties::text::TextAlign;
                text_metrics.align = match value {
                    TextAlign::Center => TextAlignMetric::Center,
                    TextAlign::Right | TextAlign::End => TextAlignMetric::End,
                    TextAlign::MatchParent => text_metrics.align,
                    _ => TextAlignMetric::Start,
                };
            }
            Property::WhiteSpace(value) => {
                use lightningcss::properties::text::WhiteSpace;
                text_metrics.whitespace = match value {
                    WhiteSpace::Pre => WhiteSpaceMetric::Pre,
                    WhiteSpace::PreWrap | WhiteSpace::BreakSpaces => WhiteSpaceMetric::PreWrap,
                    WhiteSpace::NoWrap => WhiteSpaceMetric::NoWrap,
                    _ => WhiteSpaceMetric::Normal,
                };
            }
            Property::Font(value) => {
                font_size_winner = Some(&value.size);
                line_height_winner = Some(&value.line_height);
            }
            _ => {}
        }
    }
    // Resolve every relative value against the inherited size, independent of
    // declaration iteration order. Font shorthand competes at its cascade position.
    if let Some(size) =
        font_size_winner.and_then(|value| font_size(value, text_metrics.font_size, context))
    {
        text_metrics.font_size = size;
    }
    if let Some(height) =
        line_height_winner.and_then(|value| line_height(value, text_metrics.font_size, context))
    {
        text_metrics.line_height = height;
    }
    let context = ResolveContext {
        font_size: text_metrics.font_size,
        ..context
    };
    for (_, _, _, _, property) in declarations {
        use lightningcss::properties::border::LineStyle;
        let visible = |value: &LineStyle| !matches!(value, LineStyle::None | LineStyle::Hidden);
        match property {
            Property::Border(value) => border_visible = [visible(&value.style); 4],
            Property::BorderTop(value) => border_visible[0] = visible(&value.style),
            Property::BorderRight(value) => border_visible[1] = visible(&value.style),
            Property::BorderBottom(value) => border_visible[2] = visible(&value.style),
            Property::BorderLeft(value) => border_visible[3] = visible(&value.style),
            Property::BorderStyle(value) => {
                border_visible = [
                    visible(&value.top),
                    visible(&value.right),
                    visible(&value.bottom),
                    visible(&value.left),
                ]
            }
            Property::BorderTopStyle(value) => border_visible[0] = visible(value),
            Property::BorderRightStyle(value) => border_visible[1] = visible(value),
            Property::BorderBottomStyle(value) => border_visible[2] = visible(value),
            Property::BorderLeftStyle(value) => border_visible[3] = visible(value),
            _ => {}
        }
        apply_property(style, property, context);
    }
    let zero = taffy::style::LengthPercentage::from_length(0.0);
    if !border_visible[0] {
        style.border.top = zero;
    }
    if !border_visible[1] {
        style.border.right = zero;
    }
    if !border_visible[2] {
        style.border.bottom = zero;
    }
    if !border_visible[3] {
        style.border.left = zero;
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

fn font_size(value: &CssFontSize, parent: f32, context: ResolveContext) -> Option<f32> {
    let size = match value {
        CssFontSize::Length(value) => match value {
            CssLengthPercentage::Dimension(value) => scalar(
                value,
                ResolveContext {
                    font_size: parent,
                    ..context
                },
            )?,
            CssLengthPercentage::Percentage(value) => parent * value.0,
            _ => return None,
        },
        CssFontSize::Absolute(value) => match value {
            AbsoluteFontSize::XXSmall => 9.0,
            AbsoluteFontSize::XSmall => 10.0,
            AbsoluteFontSize::Small => 13.0,
            AbsoluteFontSize::Medium => 16.0,
            AbsoluteFontSize::Large => 18.0,
            AbsoluteFontSize::XLarge => 24.0,
            AbsoluteFontSize::XXLarge => 32.0,
            AbsoluteFontSize::XXXLarge => 48.0,
        },
        CssFontSize::Relative(value) => match value {
            RelativeFontSize::Larger => parent * 1.2,
            RelativeFontSize::Smaller => parent / 1.2,
        },
    };
    size.is_finite().then_some(size.max(0.0))
}

fn line_height(
    value: &CssLineHeight,
    font_size: f32,
    context: ResolveContext,
) -> Option<LineHeightMetric> {
    let metric = match value {
        CssLineHeight::Normal => LineHeightMetric::Normal,
        CssLineHeight::Number(value) => LineHeightMetric::Number(*value),
        CssLineHeight::Length(CssLengthPercentage::Dimension(value)) => {
            LineHeightMetric::Absolute(scalar(
                value,
                ResolveContext {
                    font_size,
                    ..context
                },
            )?)
        }
        CssLineHeight::Length(CssLengthPercentage::Percentage(value)) => {
            LineHeightMetric::Absolute(font_size * value.0)
        }
        CssLineHeight::Length(_) => return None,
    };
    let valid = match metric {
        LineHeightMetric::Normal => true,
        LineHeightMetric::Number(value) | LineHeightMetric::Absolute(value) => value.is_finite(),
    };
    valid.then_some(metric)
}

#[derive(Clone, Copy)]
struct ResolveContext {
    viewport: (u32, u32),
    font_size: f32,
    root_font_size: f32,
}

fn scalar(value: &LengthValue, context: ResolveContext) -> Option<f32> {
    use LengthValue::*;
    let (w, h) = (context.viewport.0 as f32, context.viewport.1 as f32);
    let result = match value {
        Px(v) => *v,
        Em(v) => *v * context.font_size,
        Rem(v) => *v * context.root_font_size,
        Vw(v) | Svw(v) | Lvw(v) | Dvw(v) => *v * w / 100.0,
        Vh(v) | Svh(v) | Lvh(v) | Dvh(v) => *v * h / 100.0,
        Vmin(v) | Svmin(v) | Lvmin(v) | Dvmin(v) => *v * w.min(h) / 100.0,
        Vmax(v) | Svmax(v) | Lvmax(v) | Dvmax(v) => *v * w.max(h) / 100.0,
        In(v) => *v * 96.0,
        Cm(v) => *v * 96.0 / 2.54,
        Mm(v) => *v * 96.0 / 25.4,
        Pt(v) => *v * 96.0 / 72.0,
        Pc(v) => *v * 16.0,
        _ => return None,
    };
    result.is_finite().then_some(result)
}
fn raw_length(
    value: &CssLengthPercentage,
    context: ResolveContext,
) -> Option<taffy::style::LengthPercentage> {
    match value {
        CssLengthPercentage::Dimension(value) => {
            scalar(value, context).map(taffy::style::LengthPercentage::from_length)
        }
        CssLengthPercentage::Percentage(value) => {
            Some(taffy::style::LengthPercentage::from_percent(value.0))
        }
        _ => None,
    }
}
fn length(
    value: &LengthPercentageOrAuto,
    context: ResolveContext,
) -> Option<taffy::style::LengthPercentage> {
    match value {
        LengthPercentageOrAuto::LengthPercentage(value) => raw_length(value, context),
        _ => None,
    }
}
fn auto(value: &LengthPercentageOrAuto, context: ResolveContext) -> Option<LengthPercentageAuto> {
    match value {
        LengthPercentageOrAuto::Auto => Some(LengthPercentageAuto::AUTO),
        LengthPercentageOrAuto::LengthPercentage(CssLengthPercentage::Dimension(value)) => {
            scalar(value, context).map(LengthPercentageAuto::from_length)
        }
        LengthPercentageOrAuto::LengthPercentage(CssLengthPercentage::Percentage(value)) => {
            Some(LengthPercentageAuto::from_percent(value.0))
        }
        _ => None,
    }
}
fn dimension(value: &CssSize, context: ResolveContext) -> Option<Dimension> {
    match value {
        CssSize::Auto => Some(Dimension::AUTO),
        CssSize::LengthPercentage(CssLengthPercentage::Dimension(value)) => {
            scalar(value, context).map(Dimension::from_length)
        }
        CssSize::LengthPercentage(CssLengthPercentage::Percentage(value)) => {
            Some(Dimension::from_percent(value.0))
        }
        _ => None,
    }
}
fn minimum(value: &CssSize, context: ResolveContext) -> Option<Dimension> {
    dimension(value, context)
}
fn maximum(
    value: &lightningcss::properties::size::MaxSize,
    context: ResolveContext,
) -> Option<Dimension> {
    use lightningcss::properties::size::MaxSize;
    match value {
        MaxSize::None => Some(Dimension::AUTO),
        MaxSize::LengthPercentage(value) => {
            dimension(&CssSize::LengthPercentage(value.clone()), context)
        }
        _ => None,
    }
}
fn border_width(
    value: &lightningcss::properties::border::BorderSideWidth,
    context: ResolveContext,
) -> Option<taffy::style::LengthPercentage> {
    use lightningcss::properties::border::BorderSideWidth::*;
    use lightningcss::values::length::Length;
    let width = match value {
        Thin => 1.0,
        Medium => 3.0,
        Thick => 5.0,
        Length(Length::Value(value)) => scalar(value, context)?,
        _ => return None,
    };
    Some(taffy::style::LengthPercentage::from_length(width))
}
fn border<const P: u8>(
    value: &lightningcss::properties::border::GenericBorder<
        lightningcss::properties::border::LineStyle,
        P,
    >,
    context: ResolveContext,
) -> Option<taffy::style::LengthPercentage> {
    use lightningcss::properties::border::LineStyle;
    if matches!(value.style, LineStyle::None | LineStyle::Hidden) {
        Some(taffy::style::LengthPercentage::from_length(0.0))
    } else {
        border_width(&value.width, context)
    }
}
fn apply_property(style: &mut Style, property: &Property<'_>, context: ResolveContext) {
    macro_rules! set {
        ($target:expr,$value:expr,$convert:ident) => {
            if let Some(value) = $convert($value, context) {
                $target = value;
            }
        };
    }
    match property {
        Property::MaxWidth(value) => set!(style.max_size.width, value, maximum),
        Property::MaxHeight(value) => set!(style.max_size.height, value, maximum),
        Property::BoxSizing(value, _) => {
            style.box_sizing = match value {
                lightningcss::properties::size::BoxSizing::BorderBox => BoxSizing::BorderBox,
                _ => BoxSizing::ContentBox,
            }
        }
        Property::Border(value) => {
            if let Some(width) = border(value, context) {
                style.border = Rect {
                    top: width,
                    left: width,
                    bottom: width,
                    right: width,
                };
            }
        }
        Property::BorderTop(value) => set!(style.border.top, value, border),
        Property::BorderLeft(value) => set!(style.border.left, value, border),
        Property::BorderBottom(value) => set!(style.border.bottom, value, border),
        Property::BorderRight(value) => set!(style.border.right, value, border),
        Property::BorderTopWidth(value) => set!(style.border.top, value, border_width),
        Property::BorderLeftWidth(value) => set!(style.border.left, value, border_width),
        Property::BorderBottomWidth(value) => set!(style.border.bottom, value, border_width),
        Property::BorderRightWidth(value) => set!(style.border.right, value, border_width),
        Property::BorderWidth(value) => {
            set!(style.border.top, &value.top, border_width);
            set!(style.border.left, &value.left, border_width);
            set!(style.border.bottom, &value.bottom, border_width);
            set!(style.border.right, &value.right, border_width);
        }
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

type Declaration<'a> = (bool, bool, u32, usize, &'a Property<'static>);
fn collect_sheet<'a>(
    css: &'a Css,
    index: usize,
    document: &Document,
    id: HtmlId,
    context: ResolveContext,
    out: &mut Vec<Declaration<'a>>,
    order: &mut usize,
    active: &mut Vec<usize>,
) {
    if active.contains(&index) {
        return;
    }
    let Some(sheet) = css.sheets.get(index) else {
        return;
    };
    active.push(index);
    collect_rules(
        &sheet.rules.0,
        sheet.sources.first().map(String::as_str).unwrap_or(""),
        css,
        document,
        id,
        context,
        out,
        order,
        active,
    );
    active.pop();
}
fn collect_rules<'a>(
    rules: &'a [lightningcss::rules::CssRule<'static>],
    origin: &str,
    css: &'a Css,
    document: &Document,
    id: HtmlId,
    context: ResolveContext,
    out: &mut Vec<Declaration<'a>>,
    order: &mut usize,
    active: &mut Vec<usize>,
) {
    use lightningcss::rules::CssRule;
    for rule in rules {
        match rule {
            CssRule::Style(rule) => {
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
                            out.push((important, false, specificity, *order, property));
                            *order += 1;
                        }
                    }
                }
            }
            CssRule::Media(rule) if media_matches(&rule.query, context) => collect_rules(
                &rule.rules.0,
                origin,
                css,
                document,
                id,
                context,
                out,
                order,
                active,
            ),
            CssRule::Import(rule)
                if rule.supports.is_none()
                    && rule.layer.is_none()
                    && media_matches(&rule.media, context) =>
            {
                if let Ok(mut url) = url::Url::parse(origin).and_then(|base| base.join(&rule.url)) {
                    url.set_fragment(None);
                    if let Some(&index) = css.external_sheets.get(url.as_str()) {
                        collect_sheet(css, index, document, id, context, out, order, active);
                    }
                }
            }
            _ => {}
        }
    }
}
fn media_matches(
    media: &lightningcss::media_query::MediaList<'_>,
    context: ResolveContext,
) -> bool {
    use lightningcss::media_query::{MediaType, Qualifier};
    media.media_queries.is_empty()
        || media.media_queries.iter().any(|query| {
            let result = matches!(query.media_type, MediaType::All | MediaType::Screen)
                && query
                    .condition
                    .as_ref()
                    .is_none_or(|condition| media_condition(condition, context));
            if matches!(query.qualifier, Some(Qualifier::Not)) {
                !result
            } else {
                result
            }
        })
}
fn media_condition(
    condition: &lightningcss::media_query::MediaCondition<'_>,
    context: ResolveContext,
) -> bool {
    use lightningcss::media_query::{MediaCondition, Operator};
    match condition {
        MediaCondition::Not(condition) => !media_condition(condition, context),
        MediaCondition::Operation {
            operator,
            conditions,
        } => match operator {
            Operator::And => conditions
                .iter()
                .all(|condition| media_condition(condition, context)),
            Operator::Or => conditions
                .iter()
                .any(|condition| media_condition(condition, context)),
        },
        MediaCondition::Feature(feature) => media_feature(feature, context),
        _ => false,
    }
}
fn media_feature(
    feature: &lightningcss::media_query::MediaFeature<'_>,
    context: ResolveContext,
) -> bool {
    use lightningcss::media_query::{
        MediaFeatureComparison as Comparison, MediaFeatureId as Id, MediaFeatureName as Name,
        MediaFeatureValue as Value, QueryFeature,
    };
    let compare = |actual: f32, operator: Comparison, value: &Value<'_>| {
        let wanted = match value {
            Value::Length(lightningcss::values::length::Length::Value(value)) => scalar(
                value,
                ResolveContext {
                    font_size: 11.0,
                    root_font_size: 11.0,
                    ..context
                },
            ),
            Value::Number(value) => Some(*value),
            Value::Ratio(value) => Some(value.0 / value.1),
            _ => None,
        };
        wanted.is_some_and(|wanted| match operator {
            Comparison::Equal => actual == wanted,
            Comparison::LessThan => actual < wanted,
            Comparison::LessThanEqual => actual <= wanted,
            Comparison::GreaterThan => actual > wanted,
            Comparison::GreaterThanEqual => actual >= wanted,
        })
    };
    let actual = |name: &Name<'_, Id>| match name {
        Name::Standard(Id::Width) => Some(context.viewport.0 as f32),
        Name::Standard(Id::Height) => Some(context.viewport.1 as f32),
        Name::Standard(Id::AspectRatio) => {
            Some(context.viewport.0 as f32 / context.viewport.1.max(1) as f32)
        }
        _ => None,
    };
    match feature {
        QueryFeature::Plain {
            name: Name::Standard(Id::Orientation),
            value: Value::Ident(value),
        } => match value.as_ref() {
            "landscape" => context.viewport.0 > context.viewport.1,
            "portrait" => context.viewport.0 <= context.viewport.1,
            _ => false,
        },
        QueryFeature::Plain { name, value } => {
            actual(name).is_some_and(|value_actual| compare(value_actual, Comparison::Equal, value))
        }
        QueryFeature::Range {
            name,
            operator,
            value,
        } => actual(name).is_some_and(|value_actual| compare(value_actual, *operator, value)),
        QueryFeature::Interval {
            name,
            start,
            start_operator,
            end,
            end_operator,
        } => actual(name).is_some_and(|value_actual| {
            let inverse = match start_operator {
                Comparison::LessThan => Comparison::GreaterThan,
                Comparison::LessThanEqual => Comparison::GreaterThanEqual,
                Comparison::GreaterThan => Comparison::LessThan,
                Comparison::GreaterThanEqual => Comparison::LessThanEqual,
                Comparison::Equal => Comparison::Equal,
            };
            compare(value_actual, inverse, start) && compare(value_actual, *end_operator, end)
        }),
        QueryFeature::Boolean { name } => actual(name).is_some_and(|value| value != 0.0),
    }
}

#[derive(Clone, Copy)]
enum TextAlignMetric {
    Start,
    Center,
    End,
}
impl TextAlignMetric {
    fn justify(self) -> JustifyContent {
        match self {
            Self::Start => JustifyContent::Start,
            Self::Center => JustifyContent::Center,
            Self::End => JustifyContent::End,
        }
    }
}
#[derive(Clone, Copy)]
enum WhiteSpaceMetric {
    Normal,
    NoWrap,
    Pre,
    PreWrap,
}
fn default_inline(tag: &str) -> bool {
    matches!(
        tag,
        "a" | "abbr"
            | "b"
            | "bdi"
            | "bdo"
            | "br"
            | "button"
            | "cite"
            | "code"
            | "del"
            | "em"
            | "i"
            | "img"
            | "input"
            | "ins"
            | "kbd"
            | "label"
            | "mark"
            | "q"
            | "s"
            | "samp"
            | "select"
            | "small"
            | "span"
            | "strong"
            | "sub"
            | "sup"
            | "textarea"
            | "time"
            | "u"
            | "var"
    )
}
fn text_chunks(text: &str, whitespace: WhiteSpaceMetric) -> Vec<String> {
    if matches!(
        whitespace,
        WhiteSpaceMetric::Pre | WhiteSpaceMetric::PreWrap
    ) {
        return if text.is_empty() {
            vec![]
        } else {
            vec![text.into()]
        };
    }
    let words: Vec<_> = text
        .split(is_css_space)
        .filter(|word| !word.is_empty())
        .collect();
    if matches!(whitespace, WhiteSpaceMetric::NoWrap) {
        return if words.is_empty() {
            vec![]
        } else {
            vec![words.join(" ")]
        };
    }
    if words.is_empty() {
        return if text.is_empty() {
            vec![]
        } else {
            vec![" ".into()]
        };
    }
    let mut result = Vec::new();
    if text.starts_with(is_css_space) {
        result.push(" ".into());
    }
    for (index, word) in words.iter().enumerate() {
        let mut word = word.to_string();
        if index + 1 < words.len() || text.ends_with(is_css_space) {
            word.push(' ');
        }
        result.push(word);
    }
    result
}
fn flush_inline_run(
    tree: &mut TaffyTree<TextMeasureContext>,
    children: &mut Vec<NodeId>,
    run: &mut Vec<NodeId>,
    metrics: TextMetrics,
    full_width: bool,
) -> Result<(), taffy::TaffyError> {
    if run.is_empty() {
        return Ok(());
    }
    let style = Style {
        display: Display::Flex,
        flex_wrap: if matches!(
            metrics.whitespace,
            WhiteSpaceMetric::NoWrap | WhiteSpaceMetric::Pre
        ) {
            FlexWrap::NoWrap
        } else {
            FlexWrap::Wrap
        },
        align_items: Some(AlignItems::Start),
        align_content: Some(AlignContent::Start),
        justify_content: Some(metrics.align.justify()),
        size: Size {
            width: if full_width {
                Dimension::from_percent(1.0)
            } else {
                Dimension::AUTO
            },
            height: Dimension::AUTO,
        },
        ..Style::default()
    };
    children.push(tree.new_with_children(style, run)?);
    run.clear();
    Ok(())
}
#[derive(Clone, Copy)]
struct BoxRect {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
}
impl BoxRect {
    fn json(&self) -> serde_json::Value {
        serde_json::json!({"left":self.left,"top":self.top,"right":self.left+self.width,"bottom":self.top+self.height,"width":self.width,"height":self.height})
    }
    fn bounds(rects: &[Self]) -> Option<Self> {
        let first = *rects.first()?;
        Some(rects.iter().skip(1).fold(first, |a, b| {
            let left = a.left.min(b.left);
            let top = a.top.min(b.top);
            Self {
                left,
                top,
                width: (a.left + a.width).max(b.left + b.width) - left,
                height: (a.top + a.height).max(b.top + b.height) - top,
            }
        }))
    }
}
fn xml_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn visual_lines(context: &TextMeasureContext, width: f32) -> Vec<String> {
    let advance = context.metrics.font_size * (6.0 / 11.0);
    let columns = if advance > 0.0 {
        (width / advance).floor().max(1.0) as usize
    } else {
        usize::MAX
    };
    let nowrap = matches!(
        context.metrics.whitespace,
        WhiteSpaceMetric::Pre | WhiteSpaceMetric::NoWrap
    );
    let mut lines = Vec::new();
    for line in context.text.split('\n') {
        if nowrap || line.is_empty() {
            lines.push(line.into());
            continue;
        }
        let chars: Vec<_> = line.chars().collect();
        for chunk in chars.chunks(columns) {
            lines.push(chunk.iter().collect());
        }
    }
    lines
}

fn used_for_layout(name: &str) -> bool {
    matches!(
        name,
        "width"
            | "height"
            | "min-width"
            | "min-height"
            | "max-width"
            | "max-height"
            | "box-sizing"
            | "margin"
            | "margin-top"
            | "margin-right"
            | "margin-bottom"
            | "margin-left"
            | "padding"
            | "padding-top"
            | "padding-right"
            | "padding-bottom"
            | "padding-left"
            | "border"
            | "border-top"
            | "border-right"
            | "border-bottom"
            | "border-left"
            | "border-width"
            | "border-style"
            | "border-top-width"
            | "border-right-width"
            | "border-bottom-width"
            | "border-left-width"
            | "border-top-style"
            | "border-right-style"
            | "border-bottom-style"
            | "border-left-style"
            | "position"
            | "top"
            | "right"
            | "bottom"
            | "left"
            | "display"
            | "font"
            | "font-size"
            | "line-height"
            | "text-align"
            | "white-space"
            | "flex-grow"
            | "flex-shrink"
            | "flex-direction"
    )
}
fn unused_properties(
    rules: &[lightningcss::rules::CssRule<'static>],
    unused: &mut std::collections::BTreeSet<String>,
) {
    use lightningcss::rules::CssRule;
    for rule in rules {
        match rule {
            CssRule::Style(rule) => {
                for property in rule
                    .declarations
                    .declarations
                    .iter()
                    .chain(&rule.declarations.important_declarations)
                {
                    let name = property.property_id().name().to_owned();
                    if !used_for_layout(&name) {
                        unused.insert(name);
                    }
                }
                unused_properties(&rule.rules.0, unused);
            }
            CssRule::Media(rule) => unused_properties(&rule.rules.0, unused),
            _ => {}
        }
    }
}

fn is_css_space(character: char) -> bool {
    matches!(character, ' ' | '\t' | '\n' | '\r' | '\u{000c}')
}

#![allow(dead_code)]
extern crate self as trueos;
pub mod logl {
    pub mod level {
        pub const INFO: u32 = 1;
        pub const ERROR: u32 = 2;
    }
    pub fn log(_: u32, _: std::fmt::Arguments<'_>) {}
}
pub mod netfs {
    use std::cell::RefCell;
    #[derive(Default)]
    struct Network {
        requests: Vec<(u32, String, Option<Vec<u8>>)>,
        next: u32,
    }
    thread_local! {static NET:RefCell<Network>=RefCell::new(Network::default());}
    pub fn fetch_bytes(url: &[u8]) -> Result<u32, i32> {
        NET.with(|net| {
            let mut net = net.borrow_mut();
            net.next += 1;
            let id = net.next;
            net.requests
                .push((id, String::from_utf8(url.to_vec()).unwrap(), None));
            Ok(id)
        })
    }
    pub fn fetch_bytes_result_len(id: u32) -> Result<usize, i32> {
        NET.with(|net| {
            net.borrow()
                .requests
                .iter()
                .find(|(operation, _, _)| *operation == id)
                .and_then(|(_, _, body)| body.as_ref())
                .map(Vec::len)
                .ok_or(-8)
        })
    }
    pub fn fetch_bytes_read(id: u32) -> Result<Vec<u8>, i32> {
        NET.with(|net| {
            net.borrow()
                .requests
                .iter()
                .find(|(operation, _, _)| *operation == id)
                .and_then(|(_, _, body)| body.clone())
                .ok_or(-8)
        })
    }
    pub fn fetch_bytes_discard(id: u32) -> i32 {
        NET.with(|net| {
            net.borrow_mut()
                .requests
                .retain(|(operation, _, _)| *operation != id)
        });
        0
    }
    pub fn complete(url: &str, body: &str) {
        NET.with(|net| {
            let mut net = net.borrow_mut();
            let request = net
                .requests
                .iter_mut()
                .find(|(_, request, _)| request == url)
                .unwrap();
            request.2 = Some(body.as_bytes().to_vec());
        });
    }
}
#[path = "../../../src/css.rs"]
mod css;
#[path = "../../../src/layout.rs"]
mod layout;
#[path = "../../../src/viewport.rs"]
mod viewport;
#[cfg(test)]
mod tests {
    use super::*;
    use static_self::IntoOwned;
    fn run(html: &str, css_text: &str) -> (rp5::Document, serde_json::Value) {
        let (doc, output) = export(html, css_text);
        (doc, serde_json::from_str(&output.json).unwrap())
    }
    fn export(html: &str, css_text: &str) -> (rp5::Document, layout::Export) {
        let doc = rp5::parse(html);
        let mut css = css::Css::default();
        css.add_sheet(
            lightningcss::stylesheet::StyleSheet::parse(css_text, Default::default())
                .unwrap()
                .into_owned(),
        );
        for id in 0..doc.nodes.len() {
            if let Some((_, _, attrs)) = doc.element(id) {
                if let Some(attr) = attrs.iter().find(|attr| attr.name == "style") {
                    css.add_attribute(
                        id,
                        lightningcss::stylesheet::StyleAttribute::parse(
                            &attr.value,
                            Default::default(),
                        )
                        .unwrap()
                        .into_owned(),
                    );
                }
            }
        }
        let output = layout::compute(&doc, &css, (1280, 720)).unwrap();
        assert!(output.svg.starts_with("<svg"));
        (doc, output)
    }
    fn node<'a>(
        doc: &rp5::Document,
        result: &'a serde_json::Value,
        html_id: &str,
    ) -> &'a serde_json::Value {
        let id = doc
            .nodes
            .iter()
            .enumerate()
            .find(|(id, _)| {
                doc.element(*id).is_some_and(|(_, _, attrs)| {
                    attrs
                        .iter()
                        .any(|attr| attr.name == "id" && attr.value == html_id)
                })
            })
            .unwrap()
            .0;
        result["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["node"] == id)
            .unwrap()
    }
    fn text_rect<'a>(
        doc: &rp5::Document,
        result: &'a serde_json::Value,
        html_id: &str,
    ) -> &'a serde_json::Value {
        let parent = doc
            .nodes
            .iter()
            .enumerate()
            .find(|(id, _)| {
                doc.element(*id).is_some_and(|(_, _, attrs)| {
                    attrs
                        .iter()
                        .any(|attr| attr.name == "id" && attr.value == html_id)
                })
            })
            .unwrap()
            .0;
        let child = doc.nodes[parent]
            .children
            .iter()
            .copied()
            .find(|id| matches!(doc.nodes[*id].kind, rp5::NodeKind::Text(_)))
            .unwrap();
        result["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["node"] == child)
            .unwrap()
    }
    #[test]
    fn text_runs_and_svg_share_baselines_widths_and_escaped_unicode() {
        let doc = rp5::parse("<div>A&amp;Ω\nB&lt;</div><span class='hide'>invisible</span>");
        let mut css = css::Css::default();
        css.add_sheet(
            lightningcss::stylesheet::StyleSheet::parse(
                "div{font-size:22px;line-height:30px;white-space:pre}.hide{display:none}",
                Default::default(),
            )
            .unwrap()
            .into_owned(),
        );
        let output = layout::compute(&doc, &css, (1280, 720)).unwrap();
        assert_eq!(output.text_runs.len(), 2);
        assert_eq!(output.svg.matches("<text ").count(), 2);
        for run in &output.text_runs {
            assert_eq!(run.size_px, 22.0);
            assert_eq!(run.advance_px, layout::estimate_text_width(&run.text, 22.0));
            assert!(output.svg.contains(&format!(
                "<text x=\"{}\" y=\"{}\" font-size=\"{}\"",
                run.x, run.baseline_y, run.size_px,
            )));
            assert!(
                output
                    .svg
                    .contains(&format!("textLength=\"{}\"", run.advance_px))
            );
        }
        assert_eq!(output.text_runs[0].text, "A&Ω");
        assert_eq!(output.text_runs[1].text, "B<");
        assert!(
            (output.text_runs[1].baseline_y - output.text_runs[0].baseline_y - 30.0).abs() < 0.001
        );
        assert!(output.svg.contains("A&amp;Ω</text>"));
        assert!(output.svg.contains("B&lt;</text>"));
    }
    #[test]
    fn text_color_inherits_and_uses_the_cascade_without_changing_geometry() {
        let doc = rp5::parse("<div><span>A</span><b>B</b><i>C</i></div>");
        let mut css = css::Css::default();
        css.add_sheet(
            lightningcss::stylesheet::StyleSheet::parse(
            "div{color:#123456}b{color:red!important}b{color:blue}i{color:red;color:currentColor}",
                Default::default(),
            )
            .unwrap()
            .into_owned(),
        );
        let output = layout::compute(&doc, &css, (1280, 720)).unwrap();
        let colors: Vec<_> = output.text_runs.iter().map(|run| run.color).collect();
        assert_eq!(
            colors,
            [
                [0x12, 0x34, 0x56, 255],
                [255, 0, 0, 255],
                [0x12, 0x34, 0x56, 255]
            ]
        );
        assert!(output.svg.contains("fill=\"#123456\""));
        assert!(output.svg.contains("fill=\"#ff0000\""));
    }
    #[test]
    fn zero_size_subtree_and_whitespace_do_not_emit_invalid_or_empty_ink_runs() {
        let (doc, output) = export(
            "<div id='zero'>zero<span>inherited zero</span></div><div id='space'>   </div><div>ink</div>",
            "#zero{font-size:0}#space{white-space:pre}",
        );
        assert_eq!(
            output
                .text_runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<Vec<_>>(),
            ["ink"]
        );
        assert_eq!(output.svg.matches("<text ").count(), 1);
        assert!(
            output
                .text_runs
                .iter()
                .all(|run| run.size_px.is_finite() && run.size_px > 0.0)
        );
        let json = serde_json::from_str(&output.json).unwrap();
        assert_eq!(text_rect(&doc, &json, "zero")["rect"]["height"], 0.0);
        assert_eq!(text_rect(&doc, &json, "space")["rect"]["height"], 11.0);
        assert_eq!(text_rect(&doc, &json, "space")["rect"]["width"], 18.0);
    }
    #[test]
    fn pre_wrap_preserves_space_columns_and_reserves_every_emitted_line() {
        let (doc, output) = export(
            "<div id='wrapped'>a        b</div><div id='next'>next</div>",
            "#wrapped{white-space:pre-wrap;width:12px}",
        );
        let json = serde_json::from_str(&output.json).unwrap();
        let wrapped = &text_rect(&doc, &json, "wrapped")["rect"];
        assert_eq!(wrapped["height"], 55.0);
        assert!(
            node(&doc, &json, "next")["rect"]["top"].as_f64().unwrap()
                >= wrapped["bottom"].as_f64().unwrap()
        );
        let a = output
            .text_runs
            .iter()
            .find(|run| run.text.starts_with('a'))
            .unwrap();
        let b = output
            .text_runs
            .iter()
            .find(|run| run.text.ends_with('b'))
            .unwrap();
        assert_eq!(b.baseline_y - a.baseline_y, 44.0);
        assert!(
            output
                .text_runs
                .iter()
                .all(|run| !run.text.trim().is_empty())
        );
    }
    #[test]
    fn normal_trailing_space_reserves_advance_without_a_phantom_line() {
        let (doc, output) = export(
            "<div id='wrapped'>aa </div><div id='next'>next</div>",
            "#wrapped{width:12px}",
        );
        let json = serde_json::from_str(&output.json).unwrap();
        assert_eq!(text_rect(&doc, &json, "wrapped")["rect"]["height"], 11.0);
        assert_eq!(output.text_runs[0].text, "aa");
        assert_eq!(output.text_runs[0].advance_px, 12.0);
        assert!(
            output
                .text_runs
                .iter()
                .all(|run| !run.text.trim().is_empty())
        );
    }
    #[test]
    fn zero_opacity_fade_slides_keep_layout_but_only_the_current_slide_paints() {
        let (doc, output) = export(
            "<div data-animation-type='fade'><div data-wrapper><div><h2 id='old'>Old headline</h2><h2 id='current' data-current>Current headline</h2></div></div></div>",
            "[data-animation-type=fade] [data-wrapper]>div>*{position:absolute;opacity:0}[data-animation-type=fade] [data-wrapper]>div>*[data-current]{opacity:1;z-index:1}",
        );
        let json = serde_json::from_str(&output.json).unwrap();
        for id in ["old", "current"] {
            assert!(!node(&doc, &json, id)["rect"].is_null());
        }
        assert_eq!(node(&doc, &json, "old")["paint_visible"], false);
        assert_eq!(node(&doc, &json, "current")["paint_visible"], true);
        let ink = output
            .text_runs
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>();
        assert_eq!(ink, "Currentheadline");
        let old_id = node(&doc, &json, "old")["node"].as_u64().unwrap();
        let current_id = node(&doc, &json, "current")["node"].as_u64().unwrap();
        assert!(
            !output
                .svg
                .contains(&format!("<title>node {old_id}</title>"))
        );
        assert!(
            output
                .svg
                .contains(&format!("<title>node {current_id}</title>"))
        );
    }
    #[test]
    fn zero_opacity_ancestor_cannot_be_overridden_but_own_cascade_can() {
        let (doc, output) = export(
            "<div id='zero'><span id='child'>Hidden</span></div><div id='winner'>Visible</div>",
            "#zero{opacity:0}#child{opacity:1}#winner{opacity:0;opacity:1}",
        );
        let json = serde_json::from_str(&output.json).unwrap();
        assert_eq!(node(&doc, &json, "child")["paint_visible"], false);
        assert_eq!(node(&doc, &json, "winner")["paint_visible"], true);
        assert!(!node(&doc, &json, "zero")["rect"].is_null());
        assert_eq!(
            output
                .text_runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<Vec<_>>(),
            ["Visible"]
        );
    }
    #[test]
    fn inherited_visibility_can_be_overridden_by_a_visible_child() {
        let (doc, output) = export(
            "<div id='hidden'>Hidden<span>Inherited</span><span id='visible'>Shown</span></div><div id='collapse'>Collapsed</div>",
            "#hidden{visibility:hidden}#visible{visibility:visible}#collapse{visibility:collapse}",
        );
        let json = serde_json::from_str(&output.json).unwrap();
        assert_eq!(node(&doc, &json, "hidden")["paint_visible"], false);
        assert_eq!(node(&doc, &json, "visible")["paint_visible"], true);
        assert_eq!(node(&doc, &json, "collapse")["paint_visible"], false);
        assert!(!node(&doc, &json, "collapse")["rect"].is_null());
        assert_eq!(
            output
                .text_runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<Vec<_>>(),
            ["Shown"]
        );
    }
    #[test]
    fn default_text_uses_six_by_eleven_cell() {
        assert_eq!(layout::estimate_text_width("xx", 11.0), 12.0);
        let (doc, json) = run("<div id='p' style='width:18px'>xx</div>", "");
        assert_eq!(text_rect(&doc, &json, "p")["rect"]["height"], 11.0);
    }
    #[test]
    fn inherited_font_size_scales_advance_and_normal_line_height() {
        assert_eq!(layout::estimate_text_width("x", 22.0), 12.0);
        let (doc, json) = run(
            "<div id='p' style='width:26px'><span id='c'>xx</span></div>",
            "#p{font-size:22px}",
        );
        assert_eq!(text_rect(&doc, &json, "c")["rect"]["height"], 22.0);
    }
    #[test]
    fn font_shorthand_sets_size_and_numeric_line_height() {
        let (doc, json) = run(
            "<div id='p' style='width:26px'><span id='c'>xx</span></div>",
            "#p{font:22px/2 monospace}",
        );
        assert_eq!(text_rect(&doc, &json, "c")["rect"]["height"], 44.0);
    }
    #[test]
    fn inherited_absolute_and_numeric_line_height_survive_child_font_size() {
        let (doc, json) = run(
            "<div id='p' style='width:26px'><span id='a'>xx</span><span id='b'>xx</span></div>",
            "#p{font-size:22px;line-height:30px}#a{font-size:11px}#b{font-size:22px}",
        );
        assert_eq!(text_rect(&doc, &json, "a")["rect"]["height"], 30.0);
        let (doc, json) = run(
            "<div id='p' style='width:26px'><span id='c'>xx</span></div>",
            "#p{font-size:11px;line-height:1.5}#c{font-size:22px}",
        );
        assert_eq!(text_rect(&doc, &json, "c")["rect"]["height"], 33.0);
    }
    #[test]
    fn relative_font_size_resolves_against_inherited_size_and_cascade_winner() {
        let (doc, json) = run(
            "<div id='p' style='width:12px'><span id='c' class='x'>x</span></div>",
            "#p{font-size:22px}#c{font-size:2em;line-height:50px}.x{font-size:smaller}",
        );
        assert_eq!(text_rect(&doc, &json, "c")["rect"]["height"], 50.0);
    }
    #[test]
    fn wrapping_uses_available_content_width() {
        let (doc, json) = run("<div id='p' style='width:18px'>aa aa aa</div>", "");
        let r = &text_rect(&doc, &json, "p")["rect"];
        assert_eq!(r["height"], 33.0);
    }
    #[test]
    fn typed_inline_dimensions_include_padding_and_border() {
        let (doc, json) = run(
            "<div id='box' style='width:200px;height:50px;padding:10px;border:1px solid red'></div>",
            "",
        );
        let r = &node(&doc, &json, "box")["rect"];
        assert_eq!(r["width"], 222.0);
        assert_eq!(r["height"], 72.0);
        assert_eq!(
            r["right"].as_f64().unwrap() - r["left"].as_f64().unwrap(),
            222.0
        );
    }
    #[test]
    fn none_hides_descendants() {
        let (doc, json) = run(
            "<div id='outer'><div id='child'>Text</div></div>",
            "#outer{display:none}",
        );
        assert!(node(&doc, &json, "outer")["rect"].is_null());
        assert!(node(&doc, &json, "child")["rect"].is_null());
    }
    #[test]
    fn important_stylesheet_beats_normal_inline() {
        let (doc, json) = run(
            "<div id='box' class='box' style='width:200px'></div>",
            "div{width:30px}.box{width:50px}#box{width:100px!important}",
        );
        assert_eq!(node(&doc, &json, "box")["rect"]["width"], 100.0);
    }
    #[test]
    fn flex_places_children_horizontally() {
        let (doc, json) = run(
            "<div style='display:flex;width:300px'><div id='a' style='width:100px;height:20px'></div><div id='b' style='width:100px;height:20px'></div></div>",
            "",
        );
        let a = &node(&doc, &json, "a")["rect"];
        let b = &node(&doc, &json, "b")["rect"];
        assert_eq!(a["top"], b["top"]);
        assert!(b["left"].as_f64().unwrap() > a["left"].as_f64().unwrap());
    }
    #[test]
    fn compact_dimensions_preserve_auto_length_and_zero() {
        use taffy::prelude::*;
        use taffy::style::{ExpandedDimension, ExpandedLengthPercentageAuto};
        assert_eq!(
            LengthPercentageAuto::from_length(0.0).expand(),
            ExpandedLengthPercentageAuto::Length(0.0)
        );
        assert_eq!(
            LengthPercentageAuto::AUTO.expand(),
            ExpandedLengthPercentageAuto::Auto
        );
        assert_eq!(
            Dimension::from_percent(0.5).expand(),
            ExpandedDimension::Percent(0.5)
        );
        let styles: Vec<Style> = vec![Style::default(); 16];
        for style in styles {
            assert_eq!(
                style.margin.top.expand(),
                ExpandedLengthPercentageAuto::Length(0.0)
            );
            assert!(style.size.height.is_auto());
        }
    }

    #[test]
    fn viewport_units_resolve_against_both_axes() {
        let (doc, json) = run(
            "<div id='box' style='width:50vw;height:25vh;box-sizing:border-box;padding:1vmin'></div>",
            "",
        );
        let rect = &node(&doc, &json, "box")["rect"];
        assert_eq!(rect["width"], 640.0);
        assert_eq!(rect["height"], 180.0);
        let (doc, json) = run(
            "<div id='box' style='width:10vmax;height:10vmin;box-sizing:border-box'></div>",
            "",
        );
        let rect = &node(&doc, &json, "box")["rect"];
        assert_eq!(rect["width"], 128.0);
        assert_eq!(rect["height"], 72.0);
    }
    #[test]
    fn maximum_sizes_and_box_sizing_are_applied() {
        let (doc, json) = run(
            "<div id='box' style='width:500px;height:500px;max-width:100px;max-height:80px;box-sizing:border-box'></div>",
            "",
        );
        let rect = &node(&doc, &json, "box")["rect"];
        assert_eq!(rect["width"], 100.0);
        assert_eq!(rect["height"], 80.0);
    }
    #[test]
    fn authored_border_changes_box_dimensions() {
        let (doc, json) = run(
            "<div id='box' style='width:100px;border:3px solid red'></div>",
            "",
        );
        assert_eq!(node(&doc, &json, "box")["rect"]["width"], 106.0);
        let (doc, json) = run(
            "<div id='box' style='width:100px;border:none;border-width:10px'></div>",
            "",
        );
        assert_eq!(node(&doc, &json, "box")["rect"]["width"], 100.0);
    }
    #[test]
    fn complex_selectors_match_ancestors_attributes_and_nth_child() {
        let (doc, json) = run(
            "<section class='panel'><div></div><div id='box' data-x='yes'></div></section>",
            ".panel > div[data-x='yes']:not(.skip):nth-child(2){width:123px}",
        );
        assert_eq!(node(&doc, &json, "box")["rect"]["width"], 123.0);
        let (doc, json) = run(
            "<section><i></i><div id='box'></div></section>",
            "section i + div{width:99px}",
        );
        assert_eq!(node(&doc, &json, "box")["rect"]["width"], 99.0);
    }
    #[test]
    fn viewport_media_queries_and_print_filtering() {
        let (doc, json) = run(
            "<div id='box'></div>",
            "#box{width:20px}@media screen and (min-width:1000px) and (orientation:landscape){#box{width:100px}}@media print{#box{width:999px}}",
        );
        assert_eq!(node(&doc, &json, "box")["rect"]["width"], 100.0);
        let (doc, json) = run(
            "<div id='box'></div>",
            "@media (600px < width < 1400px){#box{width:88px}}",
        );
        assert_eq!(node(&doc, &json, "box")["rect"]["width"], 88.0);
    }
    #[test]
    fn inline_spans_and_links_share_a_line() {
        let (doc, json) = run(
            "<div style='width:200px'><span id='a'>xx</span><a id='b'>xx</a></div>",
            "",
        );
        let a = &node(&doc, &json, "a")["rect"];
        let b = &node(&doc, &json, "b")["rect"];
        assert_eq!(a["top"], b["top"]);
        assert_eq!(a["right"], b["left"]);
    }
    #[test]
    fn inherited_text_alignment_centers_the_inline_run() {
        let (doc, json) = run(
            "<div id='box' style='width:100px;text-align:center'>xx</div>",
            "",
        );
        let rect = &node(&doc, &json, "box")["rect"];
        let text = &text_rect(&doc, &json, "box")["rect"];
        assert_eq!(
            text["left"].as_f64().unwrap(),
            rect["left"].as_f64().unwrap() + 44.0
        );
    }
    #[test]
    fn html_source_order_wins_over_download_completion_order() {
        let doc = rp5::parse(
            "<link rel='stylesheet' href='/a.css'><link rel='stylesheet' href='/b.css'><div id='box'></div>",
        );
        let mut css = css::Css::default();
        css.parse_document(&doc, "https://order.example/");
        netfs::complete("https://order.example/b.css", "#box{width:200px}");
        css.poll_downloads();
        netfs::complete("https://order.example/a.css", "#box{width:100px}");
        css.poll_downloads();
        let export = layout::compute(&doc, &css, (1280, 720)).unwrap();
        let json: serde_json::Value = serde_json::from_str(&export.json).unwrap();
        assert_eq!(node(&doc, &json, "box")["rect"]["width"], 200.0);
    }
    #[test]
    fn imports_precede_the_importing_sheets_declarations() {
        let doc =
            rp5::parse("<style>@import '/a.css';#box{width:200px}</style><div id='box'></div>");
        let mut css = css::Css::default();
        css.parse_document(&doc, "https://imports.example/");
        netfs::complete("https://imports.example/a.css", "#box{width:100px}");
        css.poll_downloads();
        let export = layout::compute(&doc, &css, (1280, 720)).unwrap();
        let json: serde_json::Value = serde_json::from_str(&export.json).unwrap();
        assert_eq!(node(&doc, &json, "box")["rect"]["width"], 200.0);
    }
    #[test]
    fn example_page_gets_180_units_of_top_padding_and_a_width_limit() {
        let (doc, json) = run(
            "<body id='box'><p id='text'>Example page text</p></body>",
            "body{font:16px/1.6 monospace;max-width:26em;margin:auto;padding:25vh 2em 2em;text-align:center}",
        );
        let body = &node(&doc, &json, "box")["rect"];
        let paragraph = &node(&doc, &json, "text")["rect"];
        assert_eq!(body["width"], 480.0);
        assert!(paragraph["top"].as_f64().unwrap() >= 180.0);
    }
    #[test]
    fn preformatted_text_preserves_line_breaks() {
        let (doc, json) = run("<pre id='box'>aa\n bb</pre>", "");
        assert_eq!(text_rect(&doc, &json, "box")["rect"]["height"], 22.0);
    }
    #[test]
    fn index_keys_the_subject_instead_of_its_ancestor() {
        let (doc, json) = run(
            "<section id='ancestor' class='parent'><div id='box' class='child'></div></section>",
            "#ancestor > .child{width:83px}.parent #box{height:47px}",
        );
        assert_eq!(node(&doc, &json, "box")["rect"]["width"], 83.0);
        assert_eq!(node(&doc, &json, "box")["rect"]["height"], 47.0);
    }
    #[test]
    fn grouped_candidate_selectors_keep_maximum_specificity_and_order() {
        let (doc, json) = run(
            "<div id='box' class='a b a'></div>",
            ".a,#box,.b{width:83px}.a.b{width:42px}#box{height:47px}#box{height:51px}",
        );
        assert_eq!(node(&doc, &json, "box")["rect"]["width"], 83.0);
        assert_eq!(node(&doc, &json, "box")["rect"]["height"], 51.0);
    }
    #[test]
    fn functional_and_universal_selectors_remain_candidates() {
        let (doc, json) = run(
            "<div id='box' class='a'></div>",
            "*{height:25px}:is(.a,.b){width:83px}:not(.other){min-height:47px}",
        );
        assert_eq!(node(&doc, &json, "box")["rect"]["width"], 83.0);
        assert_eq!(node(&doc, &json, "box")["rect"]["height"], 47.0);
    }
    #[test]
    fn attribute_buckets_still_check_values() {
        let (doc, json) = run(
            "<div id='box' data-size='wide'></div>",
            "[data-size]{height:47px}[data-size=wide]{width:83px}[data-size=narrow]{width:42px}",
        );
        assert_eq!(node(&doc, &json, "box")["rect"]["width"], 83.0);
        assert_eq!(node(&doc, &json, "box")["rect"]["height"], 47.0);
    }
    #[test]
    fn prepared_media_rules_are_reselected_for_each_viewport() {
        let doc = rp5::parse(
            "<style>#box{width:83px}@media(max-width:600px){#box{width:42px}}</style><div id='box'></div>",
        );
        let mut css = css::Css::default();
        css.parse_document(&doc, "https://viewport.example/");
        for (width, expected) in [(1280, 83.0), (400, 42.0), (1280, 83.0)] {
            let export = layout::compute(&doc, &css, (width, 720)).unwrap();
            let json = serde_json::from_str(&export.json).unwrap();
            assert_eq!(node(&doc, &json, "box")["rect"]["width"], expected);
        }
    }
    #[test]
    fn static_rejection_does_not_prune_negated_or_alternative_pseudos() {
        let (doc, json) = run(
            "<div id='box' class='a'></div>",
            ":hover{width:42px}#box::before{width:42px}:not(:hover){width:83px}:is(:hover,.a){height:47px}",
        );
        assert_eq!(node(&doc, &json, "box")["rect"]["width"], 83.0);
        assert_eq!(node(&doc, &json, "box")["rect"]["height"], 47.0);
    }
    #[test]
    fn cached_sibling_positions_ignore_text_and_comments() {
        let (doc, json) = run(
            "<section><div id='first'></div>words<!-- ignored --><span id='middle'></span><div id='last'></div></section>",
            "div:first-of-type{width:83px}div:last-of-type{width:42px}span+div{height:47px}div~div{min-height:61px}",
        );
        assert_eq!(node(&doc, &json, "first")["rect"]["width"], 83.0);
        assert_eq!(node(&doc, &json, "last")["rect"]["width"], 42.0);
        assert_eq!(node(&doc, &json, "last")["rect"]["height"], 61.0);
    }
    #[test]
    fn cached_nth_positions_handle_reverse_and_negative_sequences() {
        let (doc, json) = run(
            "<section><div id='first'></div><span id='middle'></span><div id='last'></div></section>",
            ":nth-child(-n+2){height:47px}div:nth-last-of-type(2){width:83px}:nth-last-child(1){width:42px}",
        );
        assert_eq!(node(&doc, &json, "first")["rect"]["height"], 47.0);
        assert_eq!(node(&doc, &json, "middle")["rect"]["height"], 47.0);
        assert_eq!(node(&doc, &json, "last")["rect"]["height"], 0.0);
        assert_eq!(node(&doc, &json, "first")["rect"]["width"], 83.0);
        assert_eq!(node(&doc, &json, "last")["rect"]["width"], 42.0);
    }
}

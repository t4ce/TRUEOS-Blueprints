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
#[cfg(test)]
mod tests {
    use super::*;
    use static_self::IntoOwned;
    fn run(html: &str, css_text: &str) -> (rp5::Document, serde_json::Value) {
        let doc = rp5::parse(html);
        let mut css = css::Css::default();
        css.sheets.push(
            lightningcss::stylesheet::StyleSheet::parse(css_text, Default::default())
                .unwrap()
                .into_owned(),
        );
        for id in 0..doc.nodes.len() {
            if let Some((_, _, attrs)) = doc.element(id) {
                if let Some(attr) = attrs.iter().find(|attr| attr.name == "style") {
                    css.attributes.push((
                        id,
                        lightningcss::stylesheet::StyleAttribute::parse(
                            &attr.value,
                            Default::default(),
                        )
                        .unwrap()
                        .into_owned(),
                    ));
                }
            }
        }
        let output = layout::compute(&doc, &css, (1280, 720)).unwrap();
        assert!(output.svg.starts_with("<svg"));
        (doc, serde_json::from_str(&output.json).unwrap())
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
    fn enum_dimensions_preserve_auto_length_and_zero() {
        use taffy::prelude::*;
        assert_eq!(
            LengthPercentageAuto::from_length(0.0),
            LengthPercentageAuto::Length(0.0)
        );
        assert_eq!(LengthPercentageAuto::AUTO, LengthPercentageAuto::Auto);
        assert_eq!(Dimension::from_percent(0.5), Dimension::Percent(0.5));
        let styles: Vec<Style> = vec![Style::default(); 16];
        for style in styles {
            assert_eq!(style.margin.top, LengthPercentageAuto::Length(0.0));
            assert_eq!(style.size.height, Dimension::Auto);
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
}

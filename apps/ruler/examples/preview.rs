//! Native design preview using exactly the Blueprint's projected scene geometry.
use std::{env, fmt::Write as _, fs, io, path::PathBuf};

use ruler::{Metrics, Mode, Ruler, Scene};

const WIDTH: u32 = 1240;
const HEIGHT: u32 = 900;

fn main() -> io::Result<()> {
    let path = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/trueos-ruler-preview.svg"));
    let metrics = Metrics::new(96.0 / 25.4, 96.0 / 25.4).unwrap();
    let mut horizontal = Ruler::new(metrics, 1100, 650);
    horizontal.half_length_mm = 80.0;
    let mut rotated = horizontal.clone();
    rotated.set_angle(35.0_f32.to_radians(), false);
    let mut circle = horizontal.clone();
    circle.mode = Mode::Circular;
    circle.radius_mm = 40.0;
    circle.angle = -15.0_f32.to_radians();
    circle.radians = true;

    let mut svg = String::with_capacity(180_000);
    writeln!(svg, r##"<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{HEIGHT}" viewBox="0 0 {WIDTH} {HEIGHT}">"##).unwrap();
    svg.push_str("<title>TRUEOS Ruler — shared geometry preview</title>\n");
    svg.push_str("<desc>Linear, rotated, and circular views rendered directly from the Ruler scene model at 96 DPI. This design preview is not a physical calibration or a screenshot.</desc>\n");
    let mm = metrics.px_per_mm_x;
    writeln!(svg, r##"<defs>
  <pattern id="millimeter" width="{mm:.5}" height="{mm:.5}" patternUnits="userSpaceOnUse"><path d="M {mm:.5} 0 H 0 V {mm:.5}" fill="none" stroke="#aec3c8" stroke-opacity="0.21" stroke-width="0.5"/></pattern>
  <pattern id="centimeter" width="{cm:.5}" height="{cm:.5}" patternUnits="userSpaceOnUse"><rect width="100%" height="100%" fill="url(#millimeter)"/><path d="M {cm:.5} 0 H 0 V {cm:.5}" fill="none" stroke="#aec3c8" stroke-opacity="0.36" stroke-width="0.8"/></pattern>
</defs>"##, cm = mm * 10.0).unwrap();
    svg.push_str(r##"<rect width="1240" height="900" fill="#f3f6f6"/>
<g font-family="Inter, DejaVu Sans, sans-serif">
<text x="44" y="48" font-size="12" font-weight="600" letter-spacing="3" fill="#006c70">TRUEOS / INSTRUMENTS</text>
<text x="42" y="99" font-size="39" font-weight="500" letter-spacing="-1.4" fill="#202d32">A little room for precision.</text>
<text x="44" y="127" font-size="14" fill="#64777f">Glass, physical graduations, and labels that always face you.</text>
<rect x="40" y="154" width="1160" height="174" rx="12" fill="#e9f0f0"/>
<rect x="40" y="154" width="1160" height="174" rx="12" fill="url(#centimeter)"/>
<text x="60" y="182" font-size="11" font-weight="600" letter-spacing="1.4" fill="#687d85">01 / STRAIGHT</text>
<text x="884" y="235" font-size="16" fill="#283d45">Your scale. Both directions.</text>
<text x="884" y="259" font-size="12" fill="#64777f">Millimeters above, inches below.</text>
<text x="884" y="279" font-size="12" fill="#64777f">Move zero to any graduation.</text>
<rect x="40" y="352" width="680" height="480" rx="12" fill="#e9f0f0"/>
<rect x="40" y="352" width="680" height="480" rx="12" fill="url(#centimeter)"/>
<text x="60" y="381" font-size="11" font-weight="600" letter-spacing="1.4" fill="#687d85">02 / ROTATE</text>
<path d="M 110 452 L 651 776 M 112 775 L 647 456" fill="none" stroke="#006c70" stroke-opacity="0.13" stroke-width="1.5"/>
<rect x="740" y="352" width="460" height="480" rx="12" fill="#e9f0f0"/>
<rect x="740" y="352" width="460" height="480" rx="12" fill="url(#centimeter)"/>
<text x="760" y="381" font-size="11" font-weight="600" letter-spacing="1.4" fill="#687d85">03 / THE π TOOL</text>
<path d="M 770 613 H 1170 M 970 411 V 801" fill="none" stroke="#006c70" stroke-opacity="0.15" stroke-width="1"/>
"##);
    render_centered(&mut svg, &horizontal, 442.0, 249.0);
    render_centered(&mut svg, &rotated, 380.0, 605.0);
    render_centered(&mut svg, &circle, 970.0, 606.0);
    svg.push_str(r##"<text x="60" y="809" font-size="12" fill="#64777f">35° rotation / upright numbers / the same physical scale</text>
<text x="760" y="809" font-size="12" fill="#64777f">Wheel to resize. Read degrees or multiples of π.</text>
<text x="44" y="864" font-size="11" fill="#6c8087">SHARED SCENE PREVIEW · 96 DPI REFERENCE · VECTOR OUTPUT</text>
<text x="1198" y="864" text-anchor="end" font-size="11" fill="#6c8087">Physical size comes from your display metrics at runtime.</text>
</g>
</svg>
"##);
    fs::write(&path, svg)?;
    println!("{}", path.display());
    Ok(())
}

fn render_centered(svg: &mut String, ruler: &Ruler, x: f32, y: f32) {
    let view = ruler.view();
    writeln!(
        svg,
        "<g transform=\"translate({:.4} {:.4})\">",
        x - view.center.x,
        y - view.center.y
    )
    .unwrap();
    render_scene(svg, &ruler.scene(&view));
    svg.push_str("</g>\n");
}

fn render_scene(svg: &mut String, scene: &Scene) {
    for quad in &scene.quads {
        svg.push_str("<polygon points=\"");
        for p in quad.corners {
            write!(svg, "{:.4},{:.4} ", p.x, p.y).unwrap();
        }
        writeln!(svg, "\" {}/>", paint(quad.color)).unwrap();
    }
    for label in &scene.labels {
        writeln!(svg, "<text x=\"{:.4}\" y=\"{:.4}\" text-anchor=\"middle\" dominant-baseline=\"central\" font-size=\"{:.4}\" {}>{}</text>",
            label.center.x, label.center.y, label.font_pixels, paint(label.color), escape(&label.text)).unwrap();
    }
}

fn paint(rgba: u32) -> String {
    let [r, g, b, a] = rgba.to_le_bytes();
    format!(
        "fill=\"#{r:02x}{g:02x}{b:02x}\" fill-opacity=\"{:.5}\"",
        a as f32 / 255.0
    )
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

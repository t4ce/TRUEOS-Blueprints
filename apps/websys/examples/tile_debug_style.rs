use file_system::TRUEOS_TILE_DEBUG_FRAME_0;

fn main() {
    let frame = TRUEOS_TILE_DEBUG_FRAME_0;
    let center_left = frame.cell(5, 5).expect("R5C5 should exist");
    let center_right = frame.cell(5, 6).expect("R5C6 should exist");

    println!("frame: {}", frame.name);
    println!(
        "grid: {}x{} cells, {}x{} px each",
        frame.columns, frame.rows, frame.cell_width_px, frame.cell_height_px
    );
    println!(
        "bottom-center cells: {}={} and {}={}",
        center_left.label(),
        center_left.fill.to_hex_rgb(),
        center_right.label(),
        center_right.fill.to_hex_rgb()
    );
    println!();
    println!("{}", frame.to_css_custom_properties("trueos-tile"));
}

// trueos-blueprint: features = ["ui4-scene"]

use trueos::ui4_scene::{Error, Frame};
use trueos::logl;

fn main() {
    if let Err(error) = run() {
        logl::log(logl::level::ERROR, format_args!("hull: {error:?}"));
    }
}

fn run() -> Result<(), Error> {
    let mut frame = Frame::open(80, 64, 640, 480)?;
    frame.wait_blank_until_closed(0)
}

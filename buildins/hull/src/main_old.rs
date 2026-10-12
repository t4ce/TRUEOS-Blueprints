use trueos::ui4_scene::{Error, Frame, rgba};
use trueos::logl;

fn main() {
    if let Err(error) = run() {
        logl::log(logl::level::ERROR, format_args!("hull: {error:?}"));
    }
}

fn run() -> Result<(), Error> {
    let mut frame = Frame::open_default()?;
    frame.wait_blank_until_closed(rgba(35, 39, 39, 255))
}

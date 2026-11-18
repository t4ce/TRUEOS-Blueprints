use std::process::ExitCode;

const USAGE: &str = "lilyvfx [preview.controls capture.bgra]";
const DEFAULT_INPUT: &str = "common/lilyvfx/preview.controls";
const DEFAULT_OUTPUT: &str = "common/lilyvfx/native.bgra";

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let input_arg = args.next();
    let output_arg = args.next();
    let (input, output) = match (input_arg.as_deref(), output_arg.as_deref(), args.next()) {
        (None, None, None) => (DEFAULT_INPUT, DEFAULT_OUTPUT),
        (Some("--help" | "-h"), None, None) => {
            println!("{USAGE}\nDefault paths: {DEFAULT_INPUT} {DEFAULT_OUTPUT}");
            return ExitCode::SUCCESS;
        }
        (Some(input), Some(output), None) => (input, output),
        _ => {
            eprintln!("Usage: {USAGE}");
            return ExitCode::FAILURE;
        }
    };
    if input == output {
        eprintln!("lilyvfx: input and output must have different paths");
        return ExitCode::FAILURE;
    }
    match capture(input, output) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("lilyvfx: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
fn capture(input: &str, output: &str) -> Result<(), String> {
    use v::bp_abi::trueos_cabi_lilyvfx_background_v1;
    use v::vfs_async::{block_on, read_file, write_file};

    const CONTROL_BYTES: usize = 33 * 4;
    const CELL_BYTES: usize = 256 * 256 * 4;
    const MODE_COUNT: usize = 12;
    const ERR_BUSY: i32 = -16;
    const BUSY_RETRIES: usize = 100;
    const RETRY_DELAY_MS: u64 = 20;

    let controls = block_on(read_file(input.as_bytes()))
        .map_err(|status| format!("read {input:?} failed ({status})"))?;
    if controls.len() != MODE_COUNT * CONTROL_BYTES {
        return Err(format!(
            "{input:?} must contain twelve 132-byte control pages"
        ));
    }
    for (index, page) in controls.chunks_exact(CONTROL_BYTES).enumerate() {
        let mode = u32::from_le_bytes(page[12..16].try_into().unwrap());
        if mode != index as u32 + 2 {
            return Err(format!(
                "{input:?} must contain shader IDs 2 through 13 in order"
            ));
        }
    }

    let mut capture = vec![0; MODE_COUNT * CELL_BYTES];
    let cells = controls
        .chunks_exact(CONTROL_BYTES)
        .zip(capture.chunks_exact_mut(CELL_BYTES));
    for (index, (page, pixels)) in cells.enumerate() {
        let mut retries = 0;
        let status = loop {
            // Both buffers are live for the call and match the v1 wire sizes.
            let status = unsafe {
                trueos_cabi_lilyvfx_background_v1(
                    page.as_ptr(),
                    page.len(),
                    pixels.as_mut_ptr(),
                    pixels.len(),
                )
            };
            if status != ERR_BUSY || retries == BUSY_RETRIES {
                break status;
            }
            retries += 1;
            v::vsys::sleep_ms(RETRY_DELAY_MS);
        };
        if status != 0 {
            return Err(format!(
                "native capture failed for shader ID {} ({status})",
                index + 2
            ));
        }
    }
    block_on(write_file(output.as_bytes(), &capture))
        .map_err(|status| format!("write {output:?} failed ({status})"))?;
    println!("lilyvfx: saved {MODE_COUNT} background cells to {output}");
    Ok(())
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
fn capture(_input: &str, _output: &str) -> Result<(), String> {
    Err(
        "native background capture requires a TRUEOS kernel with the lilyvfx capture service"
            .into(),
    )
}

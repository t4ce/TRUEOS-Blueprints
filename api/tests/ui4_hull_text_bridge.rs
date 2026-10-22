use std::sync::Mutex;
use trueos::ui4_winit::{Error, HullTextRun, SceneTarget};
use v::bp_abi::TrueosUi4HullTextRunV1;

#[derive(Debug)]
struct Draw {
    text: String,
    x: f32,
    advance: f32,
    column_offset: u32,
}
static DRAWS: Mutex<Vec<Vec<Draw>>> = Mutex::new(Vec::new());

#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_write(_: u32, _: *const u8, _: usize) {}
#[unsafe(no_mangle)]
pub extern "C" fn trueos_cabi_blueprint_shutdown(_: *const u8, _: usize) -> i32 {
    0
}
#[unsafe(no_mangle)]
pub unsafe extern "C" fn trueos_cabi_ui4_hull_text_runs_v1(
    window: u32,
    runs: *const TrueosUi4HullTextRunV1,
    count: usize,
) -> i32 {
    assert_eq!(window, 42);
    assert!(count <= 2_048);
    let runs = unsafe { std::slice::from_raw_parts(runs, count) };
    assert!(runs.iter().map(|r| r.text_len).sum::<usize>() <= 64 * 1024);
    let draws = runs
        .iter()
        .map(|run| {
            assert!(run.text_len <= 4_096);
            let bytes = unsafe { std::slice::from_raw_parts(run.text, run.text_len) };
            Draw {
                text: std::str::from_utf8(bytes).unwrap().into(),
                x: run.x,
                advance: run.advance_px,
                column_offset: run.column_offset,
            }
        })
        .collect();
    DRAWS.lock().unwrap().push(draws);
    0
}

#[test]
fn large_pages_and_utf8_lines_keep_bounded_batches_and_original_advances() {
    let mut target = SceneTarget::for_window(42, 1280, 720).unwrap();
    let run = HullTextRun {
        text: "x",
        x: 12.,
        baseline_y: 34.,
        size_px: 24.,
        advance_px: 5.,
        rgba: u32::MAX,
    };
    target.draw_hull_text_runs(&vec![run; 2_049]).unwrap();
    let pages = std::mem::take(&mut *DRAWS.lock().unwrap());
    assert_eq!(pages.iter().map(Vec::len).collect::<Vec<_>>(), [2_048, 1]);

    let text = "aé日🦀".repeat(10_000);
    target
        .draw_hull_text_runs(&[HullTextRun {
            text: &text,
            advance_px: 800.,
            ..run
        }])
        .unwrap();
    let pages = std::mem::take(&mut *DRAWS.lock().unwrap());
    assert!(pages.len() > 1);
    let chunks: Vec<_> = pages.into_iter().flatten().collect();
    assert_eq!(
        chunks.iter().map(|c| c.text.as_str()).collect::<String>(),
        text
    );
    let mut x = run.x;
    for chunk in &chunks {
        assert!((chunk.x - x).abs() < 0.01);
        assert_eq!(chunk.column_offset, 0);
        x += chunk.advance;
    }
    assert!((x - run.x - 800.).abs() < 0.01);

    target
        .draw_hull_text_runs(&[HullTextRun {
            text: &text,
            advance_px: 0.,
            ..run
        }])
        .unwrap();
    let pages = std::mem::take(&mut *DRAWS.lock().unwrap());
    let mut columns = 0;
    for chunk in pages.into_iter().flatten() {
        assert_eq!(chunk.x, run.x);
        assert_eq!(chunk.advance, 0.);
        assert_eq!(chunk.column_offset as usize, columns);
        columns += chunk.text.chars().count();
    }
    assert_eq!(columns, text.chars().count());

    assert_eq!(
        target.draw_hull_text_runs(&[
            run,
            HullTextRun {
                size_px: f32::NAN,
                ..run
            }
        ]),
        Err(Error::Invalid)
    );
    assert!(DRAWS.lock().unwrap().is_empty());
}

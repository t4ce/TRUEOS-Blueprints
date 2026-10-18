#!/usr/bin/env python3
"""Exercise skybox's production resize handoff with a recording UI4 frame."""
from pathlib import Path
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]
source = (ROOT / "apps/skybox/src/main.rs").read_text()
handler = source[source.index("fn handle_resize_events("):source.index("fn handle_command(")]
harness = r'''
use std::collections::VecDeque;
#[derive(Debug, PartialEq, Clone, Copy)] enum Ui4Error { Busy, Invalid }
struct ResizeEvent { width: u32, height: u32 }
struct Frame {
    events: VecDeque<ResizeEvent>,
    requests: Vec<(u32, u32)>,
    failure: Option<Ui4Error>,
}
impl Frame {
    fn take_resize_event(&mut self) -> Result<Option<ResizeEvent>, Ui4Error> {
        Ok(self.events.pop_front())
    }
    fn resize(&mut self, w: u32, h: u32) -> Result<(), Ui4Error> {
        self.requests.push((w, h));
        self.failure.map_or(Ok(()), Err)
    }
}
struct Layout { x: i32, y: i32, width: u32, height: u32 }
__HANDLER__
#[test] fn dock_free_restore_and_repeated_extent_request_a_redraw() {
    let mut frame = Frame { events: VecDeque::new(), requests: vec![], failure: None };
    let mut layout = Layout { x: 64, y: 64, width: 768, height: 512 };
    let mut rgba = vec![0; 768 * 512 * 4];
    let mut pending = None;
    assert_eq!(handle_resize_events(&mut frame, &mut layout, &mut rgba, &mut pending), Ok(false));
    for (width, height) in [(1280, 1440), (923, 637), (768, 512), (768, 512), (32, 48)] {
        frame.events.push_back(ResizeEvent { width, height });
        assert_eq!(handle_resize_events(&mut frame, &mut layout, &mut rgba, &mut pending), Ok(true));
        assert_eq!((layout.width, layout.height), (width, height));
        assert_eq!(rgba.len(), width as usize * height as usize * 4);
        assert_eq!((layout.x, layout.y), (64, 64));
        assert_eq!(pending, None);
    }
}
#[test] fn busy_keeps_old_buffers_and_newest_target_wins() {
    let mut frame = Frame { events: VecDeque::from([ResizeEvent { width: 100, height: 80 }]),
        requests: vec![], failure: Some(Ui4Error::Busy) };
    let mut layout = Layout { x: 64, y: 64, width: 64, height: 64 };
    let mut rgba = vec![42; 64 * 64 * 4];
    let mut pending = None;
    assert_eq!(handle_resize_events(&mut frame, &mut layout, &mut rgba, &mut pending), Ok(false));
    assert_eq!(pending, Some((100, 80)));
    assert_eq!((layout.width, layout.height), (64, 64));
    assert_eq!(rgba, vec![42; 64 * 64 * 4]);
    frame.events.extend([ResizeEvent { width: 120, height: 90 }, ResizeEvent { width: 140, height: 100 }]);
    frame.failure = None;
    assert_eq!(handle_resize_events(&mut frame, &mut layout, &mut rgba, &mut pending), Ok(true));
    assert_eq!(frame.requests, [(100, 80), (140, 100)]);
    assert_eq!((layout.width, layout.height), (140, 100));
    frame.events.push_back(ResizeEvent { width: 160, height: 120 });
    frame.failure = Some(Ui4Error::Invalid);
    assert_eq!(handle_resize_events(&mut frame, &mut layout, &mut rgba, &mut pending), Err(Ui4Error::Invalid));
    assert_eq!((layout.width, layout.height), (140, 100));
}
'''
with tempfile.TemporaryDirectory(prefix="skybox-resize-") as tmp:
    path = Path(tmp)
    (path / "test.rs").write_text(harness.replace("__HANDLER__", handler))
    subprocess.run(["rustc", "--edition=2024", "--test", str(path / "test.rs"), "-o", str(path / "test")], check=True)
    subprocess.run([str(path / "test")], check=True)

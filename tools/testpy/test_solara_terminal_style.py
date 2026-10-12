#!/usr/bin/env python3
"""Render Solara's production navigator into the terminal emulator on the host."""
from pathlib import Path
import os
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[2]


def main():
    source = (ROOT / 'apps/solara/src/native_tui.rs').read_text()
    render = source[source.index('    fn render('):source.index('    pub fn tick(')]
    width = source[source.index('    fn field_width('):source.index('    fn paint(')]
    harness = '''
use std::io::{self,Write};
use crossterm::{queue,cursor::{MoveTo,Show},terminal::{Clear,ClearType},style::{Color,ResetColor,SetBackgroundColor}};
use terminal_style::{CONTROL_ROWS,CONTROL_BACKGROUND_RGB};
use navigation::{Address,Bookmarks};
mod watch_media {
@WATCH_HELPERS@
}
struct Navigator {address:Address,bookmarks:Bookmarks,status:String,columns:u16,rows:u16}
impl Navigator {
'''+width+render+'''}
#[test] fn control_rows_and_default_body_survive_selection_and_resize() {
    use trueos_terminal::TerminalColor;
    crossterm::style::force_color_output(true);
    for columns in [40,80,120] {
        for selected in [false,true] {
            let mut address=Address::default();address.insert("joinpeertube.org/");address.selected=selected;
            let bookmarks=Bookmarks::from_startup(r#"{"solara":{"bookmarks":[{"label":"PeerTube long label clipped to its column", "url":"https://joinpeertube.org/long/url/path"}]}}"#).unwrap();
            let ui=Navigator {address,bookmarks,status:"Bookmark selected".into(),columns,rows:12};
            let mut bytes=Vec::new();ui.render(&mut bytes).unwrap();
            let mut screen=trueos_terminal::Terminal::new(columns as usize,12);
            screen.feed(b"\\x1b[?7l");
            screen.feed(&bytes);
            for row in 0..12 {for col in 0..columns as usize {
                let cell=&screen.cells()[row*columns as usize+col];
                let expected=if row<3 {TerminalColor::Rgb {red:16,green:16,blue:16}} else {TerminalColor::Default};
                assert_eq!(cell.style.background,expected,"row {row}, column {col}, selected {selected}");
            }}
            let row=|y:usize|screen.cells()[y*columns as usize..(y+1)*columns as usize].iter().map(|c|c.glyph).collect::<String>();
            assert!(row(0).trim().is_empty());assert!(row(2).trim().is_empty());
            assert!(row(1).starts_with("[HTTPS] joinpeertube.org/"));
            let width=(columns as usize-4)/2;
            let table=row(3);
            assert_eq!(&table[..3]," 1 ");
            assert_eq!(&table[3..3+width],format!("{:<width$}","PeerTube long label clipped to its column".chars().take(width).collect::<String>()));
            assert_eq!(&table[4+width..4+2*width],format!("{:<width$}", "https://joinpeertube.org/long/url/path".chars().take(width).collect::<String>()));
            assert!(row(4).trim().is_empty());
            // Redraw onto an existing resized screen, so old content is erased.
            let mut resized=ui;resized.columns=60;resized.bookmarks.entries.clear();
            screen.resize(60,12);let mut bytes=Vec::new();resized.render(&mut bytes).unwrap();screen.feed(&bytes);
            assert!(screen.cells()[180..240].iter().all(|c|c.glyph==' '));
        }
    }
}
'''
    watch = (ROOT/'apps/solara/src/watch_media.rs').read_text()
    helpers = 'use url::Url;\n'+watch[watch.index('pub fn is_watch('):watch.index('fn attr')]
    harness = harness.replace('@WATCH_HELPERS@', helpers)
    harness = 'extern crate self as solara;\nmod video_page {pub fn is_video_url(_: &url::Url)->bool {false}}\n'+harness
    harness = f'#![allow(dead_code)]\n#[path="{ROOT}/api/src/terminal_style.rs"] mod terminal_style;\n#[path="{ROOT}/apps/solara/src/navigation.rs"] pub mod navigation;\n#[cfg(feature="check-native")]\n#[path="{ROOT}/apps/solara/src/native_tui.rs"] mod native_tui;\n'+harness
    with tempfile.TemporaryDirectory(prefix='solara-terminal-style-') as directory:
        folder=Path(directory)
        (folder/'src').mkdir()
        (folder/'src/lib.rs').write_text(harness)
        (folder/'Cargo.toml').write_text(f'''[package]
name="solara-terminal-style-check"
version="0.1.0"
edition="2024"
[features]
check-native=[]
[dependencies]
crossterm={{path="{ROOT}/vendor/crossterm-0.29.0-trueos",default-features=false,features=["events","bracketed-paste"]}}
trueos={{path="{ROOT}/api"}}
url="2"
serde_json="1"
trueos-terminal={{path="{ROOT.parent}/TRUEOS/crates/trueos-terminal"}}
[patch.crates-io]
mio={{path="{ROOT}/vendor/mio-1.2.0"}}
signal-hook-mio={{path="{ROOT}/vendor/signal-hook-mio-0.2.5-trueos"}}
rustix={{path="{ROOT}/vendor/rustix-1.1.4-trueos"}}
[workspace]
''')
        subprocess.run(['cargo','check','--offline','--features','check-native','--manifest-path',str(folder/'Cargo.toml')],cwd=folder,env=dict(os.environ,CARGO_TARGET_DIR='/tmp/solara-terminal-style-target'),check=True)
        subprocess.run(['cargo','test','--offline','--manifest-path',str(folder/'Cargo.toml')],cwd=folder,env=dict(os.environ,CARGO_TARGET_DIR='/tmp/solara-terminal-style-target'),check=True)


if __name__ == '__main__':
    main()

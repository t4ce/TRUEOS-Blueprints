use std::{
    collections::VecDeque,
    io::{self, Write},
    sync::{Arc, Mutex},
};

#[derive(Debug, Default, Clone)]
pub struct LogBuffer {
    inner: Arc<Mutex<LogState>>,
}

#[derive(Debug, Default)]
struct LogState {
    lines: VecDeque<String>,
    pending: String,
}

impl LogBuffer {
    pub fn recent_lines(&self, count: usize) -> Vec<String> {
        self.inner
            .lock()
            .unwrap()
            .lines
            .iter()
            .rev()
            .take(count)
            .cloned()
            .collect()
    }

    fn push_line(state: &mut LogState, line: String) {
        const MAX_LINES: usize = 2000;
        state.lines.push_back(line);
        if state.lines.len() > MAX_LINES {
            state.lines.pop_front();
        }
    }
}

impl Write for LogBuffer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        io::stdout().write_all(buf)?;

        let text = String::from_utf8_lossy(buf);
        let mut state = self.inner.lock().unwrap();
        state.pending.push_str(&text);
        while let Some(newline) = state.pending.find('\n') {
            let line = state.pending[..newline].to_owned();
            Self::push_line(&mut state, line);
            state.pending.drain(..=newline);
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> { io::stdout().flush() }
}

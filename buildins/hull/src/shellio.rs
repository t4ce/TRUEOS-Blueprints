//! Input and frame diagnostics for the attached VM minishell.

/// Distinguishes draw attempts, completed frames, publication, and backpressure.
#[derive(Default)]
pub(super) struct FrameCounter {
    f: u64,
    attempts: u64,
    published: u64,
    busy: u64,
    stage: &'static str,
}

impl FrameCounter {
    pub fn attempt(&mut self) {
        self.attempts += 1;
        self.stage = "draw";
    }
    pub fn produced(&mut self, reason: &str, width: u32, height: u32, gpu: bool) {
        self.f += 1;
        trueos::vshell::linef(format_args!(
            "hull: f={} attempts={} published={} busy={} reason={} viewport={}x{} gpu={}",
            self.f, self.attempts, self.published, self.busy, reason, width, height, gpu,
        ));
    }
    pub fn publishing(&mut self) {
        self.stage = "publish";
    }
    pub fn published(&mut self) {
        self.published += 1;
        trueos::vshell::linef(format_args!(
            "hull: published={} f={}",
            self.published, self.f
        ));
    }
    pub fn busy(&mut self) {
        self.busy += 1;
        // Keep backpressure diagnostics from flooding the shell/compositor.
        if self.busy.is_power_of_two() {
            trueos::vshell::linef(format_args!(
                "hull: busy={} stage={} f={} attempts={} published={}",
                self.busy, self.stage, self.f, self.attempts, self.published,
            ));
        }
    }
    pub fn resizing(&mut self) {
        self.stage = "resize";
    }
}

#[derive(Default)]
pub(super) struct ShellInput {
    bytes: Vec<u8>,
    overflow: bool,
}

impl ShellInput {
    pub fn push(&mut self, byte: u8) -> Option<Result<String, &'static str>> {
        match byte {
            b'\r' | b'\n' => {
                let bytes = std::mem::take(&mut self.bytes);
                if std::mem::take(&mut self.overflow) {
                    return Some(Err("URL is too long"));
                }
                if bytes.is_empty() {
                    return None;
                }
                Some(String::from_utf8(bytes).map_err(|_| "URL must be valid UTF-8"))
            }
            3 | 21 => {
                self.bytes.clear();
                self.overflow = false;
                None
            }
            8 | 127 => {
                self.bytes.pop();
                None
            }
            0x20..=0xff => {
                if self.bytes.len() < 4096 && !self.overflow {
                    self.bytes.push(byte);
                } else {
                    self.overflow = true;
                }
                None
            }
            _ => None,
        }
    }
}

pub(super) fn page_url(line: &str) -> Result<String, &'static str> {
    let line = line.trim();
    if line.chars().any(char::is_whitespace) {
        return Err("enter one HTTPS URL per line");
    }
    let mut url = url::Url::parse(line)
        .map_err(|_| "enter an HTTPS URL, for example https://example.com/")?;
    if url.scheme() != "https" || url.host_str().is_none() {
        return Err("enter an HTTPS URL, for example https://example.com/");
    }
    url.set_fragment(None);
    Ok(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_keeps_partial_lines_and_handles_crlf() {
        let mut input = ShellInput::default();
        for byte in b"https://example.com/" {
            assert!(input.push(*byte).is_none());
        }
        assert_eq!(input.push(b'\r'), Some(Ok("https://example.com/".into())));
        assert!(input.push(b'\n').is_none());
    }
    #[test]
    fn long_line_is_rejected_and_next_url_is_accepted() {
        let mut input = ShellInput::default();
        for _ in 0..4097 {
            input.push(b'x');
        }
        assert!(input.push(b'\n').unwrap().is_err());
        input.push(b'a');
        assert_eq!(input.push(b'\n'), Some(Ok("a".into())));
    }
    #[test]
    fn only_valid_https_urls_are_queued() {
        assert_eq!(
            page_url(" https://example.com/path?q=1#part "),
            Ok("https://example.com/path?q=1".into())
        );
        for input in [
            "http://example.com/",
            "file:///tmp/test",
            "https://example.com/ extra",
            "not a url",
        ] {
            assert!(page_url(input).is_err());
        }
    }
}

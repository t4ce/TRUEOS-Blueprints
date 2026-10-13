// Input-boundary handling adapted from Codex's pinned Crossterm fork.
use crate::event::{sys::unix::parse::parse_event, InputDiscardStatus, InternalEvent};
use std::collections::VecDeque;

const BRACKETED_PASTE_START: &[u8] = b"\x1b[200~";
const BRACKETED_PASTE_END: &[u8] = b"\x1b[201~";

#[derive(Debug)]
pub(super) struct Parser {
    buffer: Vec<u8>,
    internal_events: VecDeque<InternalEvent>,
    discarded_sequence: Option<DiscardedSequence>,
}

#[derive(Debug, Clone, Copy)]
enum DiscardedSequence {
    PasteStart(usize),
    PasteBody(usize),
    OscBody,
    OscEscape,
    Csi,
    X10Mouse(usize),
    Ss3,
}

impl Default for Parser {
    fn default() -> Self {
        Parser {
            // This buffer is used for -> 1 <- ANSI escape sequence. Are we
            // aware of any ANSI escape sequence that is bigger? Can we make
            // it smaller?
            //
            // Probably not worth spending more time on this as "there's a plan"
            // to use the anes crate parser.
            buffer: Vec::with_capacity(256),
            // TTY_BUFFER_SIZE is 1_024 bytes. How many ANSI escape sequences can
            // fit? What is an average sequence length? Let's guess here
            // and say that the average ANSI escape sequence length is 8 bytes. Thus
            // the buffer size should be 1024/8=128 to avoid additional allocations
            // when processing large amounts of data.
            //
            // There's no need to make it bigger, because when you look at the `try_read`
            // method implementation, all events are consumed before the next TTY_BUFFER
            // is processed -> events pushed.
            internal_events: VecDeque::with_capacity(128),
            discarded_sequence: None,
        }
    }
}

impl Parser {
    pub(super) fn discard_buffered_input(&mut self) -> InputDiscardStatus {
        if self.discarded_sequence.is_none() {
            if self.buffer.starts_with(BRACKETED_PASTE_START) {
                let matched = (1..BRACKETED_PASTE_END.len())
                    .rev()
                    .find(|&matched| self.buffer.ends_with(&BRACKETED_PASTE_END[..matched]))
                    .unwrap_or(0);
                self.discarded_sequence = Some(DiscardedSequence::PasteBody(matched));
            } else if !self.buffer.is_empty() && BRACKETED_PASTE_START.starts_with(&self.buffer) {
                self.discarded_sequence = Some(DiscardedSequence::PasteStart(self.buffer.len()));
            } else if self.buffer.starts_with(b"\x1b]") {
                self.discarded_sequence = Some(if self.buffer.ends_with(b"\x1b") {
                    DiscardedSequence::OscEscape
                } else {
                    DiscardedSequence::OscBody
                });
            } else if self.buffer.starts_with(b"\x1b[M") {
                self.discarded_sequence = Some(DiscardedSequence::X10Mouse(6 - self.buffer.len()));
            } else if self.buffer.starts_with(b"\x1b[") {
                self.discarded_sequence = Some(DiscardedSequence::Csi);
            } else if self.buffer.starts_with(b"\x1bO") {
                self.discarded_sequence = Some(DiscardedSequence::Ss3);
            }
        }
        self.buffer.clear();
        self.internal_events.clear();
        match self.discarded_sequence {
            Some(DiscardedSequence::PasteStart(_) | DiscardedSequence::PasteBody(_)) => {
                InputDiscardStatus::BracketedPasteInProgress
            }
            Some(_) => InputDiscardStatus::ControlSequenceInProgress,
            None => InputDiscardStatus::Complete,
        }
    }

    pub(super) fn advance(&mut self, buffer: &[u8], more: bool) {
        for (idx, byte) in buffer.iter().enumerate() {
            if let Some(discarded_sequence) = self.discarded_sequence {
                self.discarded_sequence = match discarded_sequence {
                    DiscardedSequence::PasteStart(matched)
                        if *byte == BRACKETED_PASTE_START[matched] =>
                    {
                        if matched + 1 == BRACKETED_PASTE_START.len() {
                            Some(DiscardedSequence::PasteBody(0))
                        } else {
                            Some(DiscardedSequence::PasteStart(matched + 1))
                        }
                    }
                    DiscardedSequence::PasteStart(_) if *byte == BRACKETED_PASTE_START[0] => {
                        Some(DiscardedSequence::PasteStart(1))
                    }
                    DiscardedSequence::PasteStart(1) if *byte == b']' => {
                        Some(DiscardedSequence::OscBody)
                    }
                    DiscardedSequence::PasteStart(1) if *byte == b'O' => {
                        Some(DiscardedSequence::Ss3)
                    }
                    DiscardedSequence::PasteStart(2) if *byte == b'M' => {
                        Some(DiscardedSequence::X10Mouse(3))
                    }
                    DiscardedSequence::PasteStart(2) if *byte == b'[' => {
                        Some(DiscardedSequence::Csi)
                    }
                    DiscardedSequence::PasteStart(matched)
                        if matched >= 2 && !(0x40..=0x7e).contains(byte) =>
                    {
                        Some(DiscardedSequence::Csi)
                    }
                    DiscardedSequence::PasteStart(_) => None,
                    DiscardedSequence::PasteBody(matched)
                        if *byte == BRACKETED_PASTE_END[matched] =>
                    {
                        if matched + 1 == BRACKETED_PASTE_END.len() {
                            None
                        } else {
                            Some(DiscardedSequence::PasteBody(matched + 1))
                        }
                    }
                    DiscardedSequence::PasteBody(_) if *byte == BRACKETED_PASTE_END[0] => {
                        Some(DiscardedSequence::PasteBody(1))
                    }
                    DiscardedSequence::PasteBody(_) => Some(DiscardedSequence::PasteBody(0)),
                    DiscardedSequence::OscBody | DiscardedSequence::OscEscape
                        if *byte == b'\x07' =>
                    {
                        None
                    }
                    DiscardedSequence::OscEscape if *byte == b'\\' => None,
                    DiscardedSequence::OscBody | DiscardedSequence::OscEscape
                        if *byte == b'\x1b' =>
                    {
                        Some(DiscardedSequence::OscEscape)
                    }
                    DiscardedSequence::OscBody | DiscardedSequence::OscEscape => {
                        Some(DiscardedSequence::OscBody)
                    }
                    DiscardedSequence::Csi | DiscardedSequence::Ss3 if *byte == b'\x1b' => {
                        Some(DiscardedSequence::PasteStart(1))
                    }
                    DiscardedSequence::Csi | DiscardedSequence::Ss3
                        if (0x40..=0x7e).contains(byte) =>
                    {
                        None
                    }
                    DiscardedSequence::Csi => Some(DiscardedSequence::Csi),
                    DiscardedSequence::X10Mouse(remaining) if remaining > 1 => {
                        Some(DiscardedSequence::X10Mouse(remaining - 1))
                    }
                    DiscardedSequence::X10Mouse(_) => None,
                    DiscardedSequence::Ss3 => Some(DiscardedSequence::Ss3),
                };
                continue;
            }
            let more = idx + 1 < buffer.len() || more;

            self.buffer.push(*byte);

            match parse_event(&self.buffer, more) {
                Ok(Some(ie)) => {
                    self.internal_events.push_back(ie);
                    self.buffer.clear();
                }
                Ok(None) => {
                    let completed_osc = self.buffer.starts_with(b"\x1b]")
                        && (*byte == b'\x07'
                            || (*byte == b'\\'
                                && self.buffer.get(self.buffer.len().saturating_sub(2))
                                    == Some(&b'\x1b')));
                    let completed_csi = self.buffer.len() > 2
                        && self.buffer.starts_with(b"\x1b[")
                        && (0x40..=0x7e).contains(byte)
                        && !self.buffer.starts_with(BRACKETED_PASTE_START)
                        && !self.buffer.starts_with(b"\x1b[[")
                        && !self.buffer.starts_with(b"\x1b[M");
                    if completed_osc || completed_csi {
                        self.buffer.clear();
                    }
                }
                Err(_) => {
                    // Event can't be parsed (not enough parameters, parameter is not a number, ...).
                    // Clear the buffer and continue with another sequence.
                    self.buffer.clear();
                }
            }
        }
    }
}

impl Iterator for Parser {
    type Item = InternalEvent;

    fn next(&mut self) -> Option<Self::Item> {
        self.internal_events.pop_front()
    }
}

#[cfg(test)]
mod discard_tests {
    use super::Parser;
    use crate::event::{Event, InputDiscardStatus, InternalEvent, KeyCode};
    #[cfg(feature = "bracketed-paste")]
    #[test]
    fn discarded_bracketed_paste_suppresses_delayed_input_until_its_end() {
        let mut parser = Parser::default();
        parser.advance(b"\x1b[200~unfinished", true);
        assert_eq!(parser.next(), None);

        assert_eq!(
            parser.discard_buffered_input(),
            InputDiscardStatus::BracketedPasteInProgress
        );
        parser.advance(b"1y\r\x1b[20", true);
        assert_eq!(parser.next(), None);
        assert_eq!(
            parser.discard_buffered_input(),
            InputDiscardStatus::BracketedPasteInProgress
        );
        parser.advance(b"1~n", false);
        assert_eq!(
            parser.next(),
            Some(InternalEvent::Event(Event::Key(KeyCode::Char('n').into())))
        );
        assert_eq!(
            parser.discard_buffered_input(),
            InputDiscardStatus::Complete
        );
    }

    #[test]
    fn discarded_partial_paste_marker_preserves_its_remaining_boundary() {
        let mut parser = Parser::default();
        parser.advance(b"\x1b[2", true);

        assert_eq!(
            parser.discard_buffered_input(),
            InputDiscardStatus::BracketedPasteInProgress
        );
        parser.advance(b"00~1y\r\x1b[201~n", false);

        assert_eq!(
            parser.next(),
            Some(InternalEvent::Event(Event::Key(KeyCode::Char('n').into())))
        );
    }

    #[test]
    fn discarded_partial_mouse_sequence_preserves_all_remaining_payload_bytes() {
        for sequence in [b"\x1b[M @y".as_slice(), b"\x1b[M\x1b@y".as_slice()] {
            for boundary in 1..sequence.len() {
                let mut parser = Parser::default();
                parser.advance(&sequence[..boundary], true);
                assert_ne!(
                    parser.discard_buffered_input(),
                    InputDiscardStatus::Complete
                );

                let remainder = &sequence[boundary..];
                for (index, byte) in remainder.iter().enumerate() {
                    parser.advance(std::slice::from_ref(byte), false);
                    assert_eq!(parser.next(), None);
                    assert_eq!(
                        parser.discard_buffered_input() == InputDiscardStatus::Complete,
                        index + 1 == remainder.len(),
                    );
                }

                parser.advance(b"n", false);
                assert_eq!(
                    parser.next(),
                    Some(InternalEvent::Event(Event::Key(KeyCode::Char('n').into())))
                );
            }
        }
    }

    #[test]
    fn completed_unsupported_control_sequences_do_not_remain_buffered() {
        for sequence in [
            b"\x1b]52;c;ignored\x07".as_slice(),
            b"\x1b]52;c;ignored\x1b\\".as_slice(),
            b"\x1b[?1h".as_slice(),
            b"\x1b[?1;2$y".as_slice(),
        ] {
            let mut parser = Parser::default();
            for byte in sequence {
                parser.advance(std::slice::from_ref(byte), true);
            }
            assert!(parser.buffer.is_empty());
            assert_eq!(
                parser.discard_buffered_input(),
                InputDiscardStatus::Complete
            );
            parser.advance(b"n", false);

            assert_eq!(
                parser.next(),
                Some(InternalEvent::Event(Event::Key(KeyCode::Char('n').into())))
            );
        }
    }

    #[test]
    fn completed_control_sequence_preserves_a_following_incomplete_boundary() {
        for (incomplete, continuation, status) in [
            (
                b"\x1b[2".as_slice(),
                b"00~1y\r\x1b[201~n".as_slice(),
                InputDiscardStatus::BracketedPasteInProgress,
            ),
            (
                b"\x1b]10;unfinished".as_slice(),
                b"1y\r\x07n".as_slice(),
                InputDiscardStatus::ControlSequenceInProgress,
            ),
        ] {
            let mut parser = Parser::default();
            let mut input = b"\x1b]52;c;ignored\x07".to_vec();
            input.extend_from_slice(incomplete);
            parser.advance(&input, true);
            assert_eq!(parser.discard_buffered_input(), status);
            parser.advance(continuation, false);

            assert_eq!(
                parser.next(),
                Some(InternalEvent::Event(Event::Key(KeyCode::Char('n').into())))
            );
        }
    }

    #[test]
    fn legacy_mouse_and_function_key_sequences_remain_recognized() {
        let mut parser = Parser::default();
        parser.advance(b"\x1b[[A", false);
        assert_eq!(
            parser.next(),
            Some(InternalEvent::Event(Event::Key(KeyCode::F(1).into())))
        );

        parser.advance(b"\x1b[M !!", false);
        assert!(matches!(
            parser.next(),
            Some(InternalEvent::Event(Event::Mouse(_)))
        ));
    }

    #[test]
    fn discarded_double_bracket_csi_suppresses_its_delayed_suffix() {
        for (suffix, completion) in [
            (b"[A".as_slice(), b"".as_slice()),
            (b"[B".as_slice(), b"".as_slice()),
            (b"[C".as_slice(), b"".as_slice()),
            (b"[D".as_slice(), b"".as_slice()),
            (b"[E".as_slice(), b"".as_slice()),
            (b"[y".as_slice(), b"".as_slice()),
            (b"[1".as_slice(), b"A".as_slice()),
            (b"[1y".as_slice(), b"".as_slice()),
        ] {
            let mut sequence = b"\x1b[".to_vec();
            sequence.extend_from_slice(suffix);
            for boundary in 1..sequence.len().min(4) {
                let mut parser = Parser::default();
                parser.advance(&sequence[..boundary], true);
                assert_ne!(
                    parser.discard_buffered_input(),
                    InputDiscardStatus::Complete
                );

                parser.advance(&sequence[boundary..], true);
                assert_eq!(parser.next(), None);
                if !completion.is_empty() {
                    assert_eq!(
                        parser.discard_buffered_input(),
                        InputDiscardStatus::ControlSequenceInProgress
                    );
                    parser.advance(completion, true);
                }
                assert_eq!(
                    parser.discard_buffered_input(),
                    InputDiscardStatus::Complete
                );

                parser.advance(b"n", false);
                assert_eq!(
                    parser.next(),
                    Some(InternalEvent::Event(Event::Key(KeyCode::Char('n').into())))
                );
            }
        }
    }

    #[test]
    fn discarded_osc_suppresses_delayed_input_until_its_terminator() {
        for terminator in [b"\x07".as_slice(), b"\x1b\\".as_slice()] {
            let mut parser = Parser::default();
            parser.advance(b"\x1b]10;unfinished", true);
            assert_eq!(
                parser.discard_buffered_input(),
                InputDiscardStatus::ControlSequenceInProgress
            );

            parser.advance(b"1y\r", true);
            assert!(parser.buffer.is_empty());
            assert_eq!(parser.next(), None);
            for byte in terminator {
                parser.advance(std::slice::from_ref(byte), true);
            }
            parser.advance(b"n", false);

            assert_eq!(
                parser.next(),
                Some(InternalEvent::Event(Event::Key(KeyCode::Char('n').into())))
            );
            assert_eq!(
                parser.discard_buffered_input(),
                InputDiscardStatus::Complete
            );
        }
    }

    #[test]
    fn discarded_control_sequences_suppress_delayed_input_until_the_final_byte() {
        for prefix in [b"\x1b[?".as_slice(), b"\x1bO".as_slice()] {
            let mut parser = Parser::default();
            parser.advance(prefix, true);
            assert_eq!(
                parser.discard_buffered_input(),
                InputDiscardStatus::ControlSequenceInProgress
            );

            parser.advance(b"1\r", true);
            assert!(parser.buffer.is_empty());
            assert_eq!(parser.next(), None);
            parser.advance(b"An", false);

            assert_eq!(
                parser.next(),
                Some(InternalEvent::Event(Event::Key(KeyCode::Char('n').into())))
            );
        }
    }

    #[test]
    fn ambiguous_paste_prefix_remains_quarantined_as_a_control_sequence() {
        let mut parser = Parser::default();
        parser.advance(b"\x1b[2", true);
        assert_eq!(
            parser.discard_buffered_input(),
            InputDiscardStatus::BracketedPasteInProgress
        );

        parser.advance(b"1\r", true);
        assert_eq!(parser.next(), None);
        assert_eq!(
            parser.discard_buffered_input(),
            InputDiscardStatus::ControlSequenceInProgress
        );
        parser.advance(b"~n", false);

        assert_eq!(
            parser.next(),
            Some(InternalEvent::Event(Event::Key(KeyCode::Char('n').into())))
        );
    }

    #[test]
    fn discarded_sequences_preserve_a_buffered_terminator_prefix() {
        for (prefix, suffix, status) in [
            (
                b"\x1b]10;unfinished\x1b".as_slice(),
                b"\\n".as_slice(),
                InputDiscardStatus::ControlSequenceInProgress,
            ),
            (
                b"\x1b[200~unfinished\x1b[20".as_slice(),
                b"1~n".as_slice(),
                InputDiscardStatus::BracketedPasteInProgress,
            ),
        ] {
            let mut parser = Parser::default();
            parser.advance(prefix, true);
            assert_eq!(parser.discard_buffered_input(), status);
            parser.advance(suffix, false);

            assert_eq!(
                parser.next(),
                Some(InternalEvent::Event(Event::Key(KeyCode::Char('n').into())))
            );
        }
    }

    #[cfg(feature = "bracketed-paste")]
    #[test]
    fn discarded_paste_uses_constant_buffer_space() {
        let mut parser = Parser::default();
        parser.advance(b"\x1b[200~", true);
        parser.discard_buffered_input();

        parser.advance(&[b'y'; 1_024], true);

        assert!(parser.buffer.is_empty());
        assert_eq!(parser.next(), None);
    }
}

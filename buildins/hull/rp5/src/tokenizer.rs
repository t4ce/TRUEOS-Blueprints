//! Direct one-shot translation of parse5's tokenizer states and input preprocessing.
//! See `../../p5/src/tokenizer/` for the upstream state machine and MIT license.
use std::collections::VecDeque;

use crate::ParseError;
use crate::token::{Attribute, DoctypeToken, TagToken, Token};

const EOF: u32 = 0x11_0000;
const NULL: u32 = 0;
const TAB: u32 = 9;
const LF: u32 = 10;
const FF: u32 = 12;
const SPACE: u32 = 32;
const BANG: u32 = 33;
const QUOTE: u32 = 34;
const AMP: u32 = 38;
const APOS: u32 = 39;
const DASH: u32 = 45;
const SLASH: u32 = 47;
const LT: u32 = 60;
const EQ: u32 = 61;
const GT: u32 = 62;
const QUESTION: u32 = 63;
const BRACKET: u32 = 93;
const GRAVE: u32 = 96;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextMode {
    Data,
    Rcdata,
    Rawtext,
    ScriptData,
    Plaintext,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Data,
    Rcdata,
    Rawtext,
    ScriptData,
    Plaintext,
    TagOpen,
    EndTagOpen,
    TagName,
    RcdataLessThanSign,
    RcdataEndTagOpen,
    RcdataEndTagName,
    RawtextLessThanSign,
    RawtextEndTagOpen,
    RawtextEndTagName,
    ScriptDataLessThanSign,
    ScriptDataEndTagOpen,
    ScriptDataEndTagName,
    ScriptDataEscapeStart,
    ScriptDataEscapeStartDash,
    ScriptDataEscaped,
    ScriptDataEscapedDash,
    ScriptDataEscapedDashDash,
    ScriptDataEscapedLessThanSign,
    ScriptDataEscapedEndTagOpen,
    ScriptDataEscapedEndTagName,
    ScriptDataDoubleEscapeStart,
    ScriptDataDoubleEscaped,
    ScriptDataDoubleEscapedDash,
    ScriptDataDoubleEscapedDashDash,
    ScriptDataDoubleEscapedLessThanSign,
    ScriptDataDoubleEscapeEnd,
    BeforeAttributeName,
    AttributeName,
    AfterAttributeName,
    BeforeAttributeValue,
    AttributeValueDoubleQuoted,
    AttributeValueSingleQuoted,
    AttributeValueUnquoted,
    AfterAttributeValueQuoted,
    SelfClosingStartTag,
    BogusComment,
    MarkupDeclarationOpen,
    CommentStart,
    CommentStartDash,
    Comment,
    CommentLessThanSign,
    CommentLessThanSignBang,
    CommentLessThanSignBangDash,
    CommentLessThanSignBangDashDash,
    CommentEndDash,
    CommentEnd,
    CommentEndBang,
    Doctype,
    BeforeDoctypeName,
    DoctypeName,
    AfterDoctypeName,
    AfterDoctypePublicKeyword,
    BeforeDoctypePublicIdentifier,
    DoctypePublicIdentifierDoubleQuoted,
    DoctypePublicIdentifierSingleQuoted,
    AfterDoctypePublicIdentifier,
    BetweenDoctypePublicAndSystemIdentifiers,
    AfterDoctypeSystemKeyword,
    BeforeDoctypeSystemIdentifier,
    DoctypeSystemIdentifierDoubleQuoted,
    DoctypeSystemIdentifierSingleQuoted,
    AfterDoctypeSystemIdentifier,
    BogusDoctype,
    CdataSection,
    CdataSectionBracket,
    CdataSectionEnd,
}

pub struct Tokenizer {
    input: Vec<char>,
    pos: usize,
    cp: u32,
    reconsume: bool,
    state: State,
    last_start_tag_name: String,
    in_foreign_node: bool,
    tag: TagToken,
    end_tag: bool,
    attr: Option<Attribute>,
    duplicate_attr: bool,
    comment: String,
    doctype: DoctypeToken,
    temporary_buffer: String,
    pending_chars: String,
    pending_kind: u8,
    queue: VecDeque<Token>,
    ended: bool,
    pub errors: Vec<ParseError>,
}

impl Tokenizer {
    pub fn new(html: &str) -> Self {
        let mut input = Vec::with_capacity(html.len());
        let mut errors = Vec::new();
        let mut skip_lf = false;
        for ch in html.chars() {
            if ch == '\n' && skip_lf {
                skip_lf = false;
                continue;
            }
            skip_lf = ch == '\r';
            let ch = if skip_lf { '\n' } else { ch };
            let cp = ch as u32;
            let code = if is_control(cp) {
                Some("control-character-in-input-stream")
            } else if is_noncharacter(cp) {
                Some("noncharacter-in-input-stream")
            } else {
                None
            };
            if let Some(code) = code {
                errors.push(ParseError {
                    code: code.into(),
                    offset: input.len(),
                });
            }
            input.push(ch);
        }
        Self {
            input,
            pos: 0,
            cp: EOF,
            reconsume: false,
            state: State::Data,
            last_start_tag_name: String::new(),
            in_foreign_node: false,
            tag: TagToken::default(),
            end_tag: false,
            attr: None,
            duplicate_attr: false,
            comment: String::new(),
            doctype: DoctypeToken::default(),
            temporary_buffer: String::new(),
            pending_chars: String::new(),
            pending_kind: 0,
            queue: VecDeque::new(),
            ended: false,
            errors,
        }
    }

    pub fn set_mode(&mut self, mode: TextMode, last_start_tag_name: &str) {
        self.state = match mode {
            TextMode::Data => State::Data,
            TextMode::Rcdata => State::Rcdata,
            TextMode::Rawtext => State::Rawtext,
            TextMode::ScriptData => State::ScriptData,
            TextMode::Plaintext => State::Plaintext,
        };
        self.last_start_tag_name = last_start_tag_name.to_ascii_lowercase();
    }

    pub fn set_in_foreign_node(&mut self, value: bool) {
        self.in_foreign_node = value;
    }

    pub fn next_token(&mut self) -> Token {
        loop {
            if let Some(token) = self.queue.pop_front() {
                return token;
            }
            if self.ended {
                return Token::Eof;
            }
            let cp = if self.reconsume {
                self.reconsume = false;
                self.cp
            } else {
                let cp = self.input.get(self.pos).map_or(EOF, |ch| *ch as u32);
                if cp != EOF {
                    self.pos += 1;
                }
                self.cp = cp;
                cp
            };
            self.step(cp);
        }
    }

    fn err(&mut self, code: &str) {
        self.errors.push(ParseError {
            code: code.into(),
            offset: if self.cp == EOF {
                self.pos
            } else {
                self.pos.saturating_sub(1)
            },
        });
    }
    fn again(&mut self, state: State) {
        self.state = state;
        self.reconsume = true;
    }
    fn flush_chars(&mut self) {
        if self.pending_chars.is_empty() {
            return;
        }
        let chars = std::mem::take(&mut self.pending_chars);
        self.queue.push_back(match self.pending_kind {
            1 => Token::Whitespace(chars),
            2 => Token::NullCharacter(chars),
            _ => Token::Character(chars),
        });
    }
    fn emit_cp(&mut self, cp: u32) {
        let kind = if whitespace(cp) {
            1
        } else if cp == NULL {
            2
        } else {
            0
        };
        if kind != self.pending_kind {
            self.flush_chars();
        }
        self.pending_kind = kind;
        self.pending_chars.push(character(cp));
    }
    fn emit_chars(&mut self, chars: &str) {
        // Explicit tokenizer strings contain no nulls or HTML whitespace.
        if self.pending_kind != 0 {
            self.flush_chars();
        }
        self.pending_kind = 0;
        self.pending_chars.push_str(chars);
    }
    fn emit(&mut self, token: Token) {
        self.flush_chars();
        self.queue.push_back(token);
    }
    fn eof(&mut self) {
        self.emit(Token::Eof);
        self.ended = true;
    }
    fn create_tag(&mut self, end_tag: bool) {
        self.tag = TagToken::default();
        self.end_tag = end_tag;
        self.attr = None;
        self.duplicate_attr = false;
    }
    fn create_attr(&mut self, initial: &str) {
        self.finish_attr();
        self.attr = Some(Attribute {
            name: initial.into(),
            ..Attribute::default()
        });
        self.duplicate_attr = false;
    }
    fn leave_attr_name(&mut self) {
        if let Some(attr) = &self.attr {
            self.duplicate_attr = self.tag.attrs.iter().any(|old| old.name == attr.name);
            if self.duplicate_attr {
                self.err("duplicate-attribute");
            }
        }
    }
    fn finish_attr(&mut self) {
        if let Some(attr) = self.attr.take() {
            if !self.duplicate_attr {
                self.tag.attrs.push(attr);
            }
        }
        self.duplicate_attr = false;
    }
    fn emit_tag(&mut self) {
        self.finish_attr();
        let tag = std::mem::take(&mut self.tag);
        if self.end_tag {
            if !tag.attrs.is_empty() {
                self.err("end-tag-with-attributes");
            }
            if tag.self_closing {
                self.err("end-tag-with-trailing-solidus");
            }
            self.emit(Token::EndTag(tag));
        } else {
            self.last_start_tag_name = tag.name.clone();
            self.emit(Token::StartTag(tag));
        }
    }
    fn emit_comment(&mut self) {
        let data = std::mem::take(&mut self.comment);
        self.emit(Token::Comment(data));
    }
    fn emit_doctype(&mut self) {
        let token = std::mem::take(&mut self.doctype);
        self.emit(Token::Doctype(token));
    }
    fn doctype_eof(&mut self) {
        self.err("eof-in-doctype");
        self.doctype.force_quirks = true;
        self.emit_doctype();
        self.eof();
    }
    fn comment_eof(&mut self) {
        self.err("eof-in-comment");
        self.emit_comment();
        self.eof();
    }
    fn tag_eof(&mut self) {
        self.err("eof-in-tag");
        self.eof();
    }
    fn null_error(&mut self) {
        self.err("unexpected-null-character");
    }

    fn starts_with_at_current(&self, text: &str, sensitive: bool) -> bool {
        let index = if self.cp == EOF {
            self.pos
        } else {
            self.pos.saturating_sub(1)
        };
        text.chars().enumerate().all(|(offset, wanted)| {
            self.input.get(index + offset).is_some_and(|actual| {
                if sensitive {
                    *actual == wanted
                } else {
                    actual.eq_ignore_ascii_case(&wanted)
                }
            })
        })
    }
    fn consume_sequence(&mut self, text: &str, sensitive: bool) -> bool {
        if self.starts_with_at_current(text, sensitive) {
            self.pos += text.chars().count() - 1;
            true
        } else {
            false
        }
    }
    fn special_end_tag(&mut self, fallback: State) {
        let name_len = self.last_start_tag_name.chars().count();
        let index = self.pos.saturating_sub(1);
        let delimiter = self
            .input
            .get(index + name_len)
            .map_or(EOF, |ch| *ch as u32);
        if !self.last_start_tag_name.is_empty()
            && self.starts_with_at_current(&self.last_start_tag_name, false)
            && (whitespace(delimiter) || delimiter == SLASH || delimiter == GT)
        {
            let name = self.last_start_tag_name.clone();
            self.create_tag(true);
            self.tag.name = name;
            self.pos += name_len;
            match delimiter {
                SLASH => self.state = State::SelfClosingStartTag,
                GT => {
                    self.state = State::Data;
                    self.emit_tag();
                }
                _ => self.state = State::BeforeAttributeName,
            }
        } else {
            self.emit_chars("</");
            self.again(fallback);
        }
    }
    fn character_reference(&mut self, in_attribute: bool) {
        // Named references are at most 32 ASCII characters. Keep a following
        // character for the attribute ambiguity rule, rather than copying the
        // whole unconsumed document each time an ampersand occurs.
        let end = if self.input.get(self.pos) == Some(&'#') {
            let mut end = self.pos + 1;
            let hexadecimal = matches!(self.input.get(end), Some('x' | 'X'));
            if hexadecimal {
                end += 1;
            }
            while self.input.get(end).is_some_and(|ch| {
                if hexadecimal {
                    ch.is_ascii_hexdigit()
                } else {
                    ch.is_ascii_digit()
                }
            }) {
                end += 1;
            }
            (end + 1).min(self.input.len())
        } else {
            (self.pos + 34).min(self.input.len())
        };
        let remainder: String = self.input[self.pos..end].iter().collect();
        if let Some((decoded, consumed)) = crate::entities::decode(&remainder, in_attribute) {
            let consumed_text = &remainder[..consumed];
            if !consumed_text.ends_with(';') {
                self.err("missing-semicolon-after-character-reference");
            }
            if consumed_text.starts_with('#') {
                self.numeric_reference_error(consumed_text);
            }
            self.pos += consumed_text.chars().count();
            if in_attribute {
                if let Some(attr) = &mut self.attr {
                    attr.value.push_str(&decoded);
                }
            } else {
                for ch in decoded.chars() {
                    self.emit_cp(ch as u32);
                }
            }
        } else {
            if remainder.starts_with('#') {
                self.err("absence-of-digits-in-numeric-character-reference");
            } else if !in_attribute {
                let length = self.input[self.pos..]
                    .iter()
                    .take_while(|ch| ch.is_ascii_alphanumeric())
                    .count();
                if length > 0 && self.input.get(self.pos + length) == Some(&';') {
                    self.err("unknown-named-character-reference");
                }
            }
            if in_attribute {
                if let Some(attr) = &mut self.attr {
                    attr.value.push('&');
                }
            } else {
                self.emit_chars("&");
            }
        }
    }
    fn numeric_reference_error(&mut self, text: &str) {
        let text = text.strip_prefix('#').unwrap_or(text).trim_end_matches(';');
        let (text, base) = match text.strip_prefix('x').or_else(|| text.strip_prefix('X')) {
            Some(text) => (text, 16),
            None => (text, 10),
        };
        let value = text.chars().fold(0u32, |value, ch| {
            value
                .saturating_mul(base)
                .saturating_add(ch.to_digit(base).unwrap_or(0))
        });
        let code = if value == 0 {
            Some("null-character-reference")
        } else if value > 0x10ffff {
            Some("character-reference-outside-unicode-range")
        } else if (0xd800..=0xdfff).contains(&value) {
            Some("surrogate-character-reference")
        } else if is_noncharacter(value) {
            Some("noncharacter-character-reference")
        } else if is_control(value) || value == 13 {
            Some("control-character-reference")
        } else {
            None
        };
        if let Some(code) = code {
            self.err(code);
        }
    }

    fn step(&mut self, cp: u32) {
        use State::*;
        match self.state {
            Data => match cp {
                LT => self.state = TagOpen,
                AMP => self.character_reference(false),
                NULL => {
                    self.null_error();
                    self.emit_cp(cp);
                }
                EOF => self.eof(),
                _ => self.emit_cp(cp),
            },
            Rcdata | Rawtext | ScriptData | Plaintext => {
                let state = self.state;
                match cp {
                    AMP if state == Rcdata => self.character_reference(false),
                    LT if state != Plaintext => {
                        self.state = match state {
                            Rcdata => RcdataLessThanSign,
                            Rawtext => RawtextLessThanSign,
                            _ => ScriptDataLessThanSign,
                        }
                    }
                    NULL => {
                        self.null_error();
                        self.emit_chars("\u{fffd}");
                    }
                    EOF => self.eof(),
                    _ => self.emit_cp(cp),
                }
            }
            TagOpen => match cp {
                cp if ascii_letter(cp) => {
                    self.create_tag(false);
                    self.again(TagName);
                }
                BANG => self.state = MarkupDeclarationOpen,
                SLASH => self.state = EndTagOpen,
                QUESTION => {
                    self.err("unexpected-question-mark-instead-of-tag-name");
                    self.comment.clear();
                    self.again(BogusComment);
                }
                EOF => {
                    self.err("eof-before-tag-name");
                    self.emit_chars("<");
                    self.eof();
                }
                _ => {
                    self.err("invalid-first-character-of-tag-name");
                    self.emit_chars("<");
                    self.again(Data);
                }
            },
            EndTagOpen => match cp {
                cp if ascii_letter(cp) => {
                    self.create_tag(true);
                    self.again(TagName);
                }
                GT => {
                    self.err("missing-end-tag-name");
                    self.state = Data;
                }
                EOF => {
                    self.err("eof-before-tag-name");
                    self.emit_chars("</");
                    self.eof();
                }
                _ => {
                    self.err("invalid-first-character-of-tag-name");
                    self.comment.clear();
                    self.again(BogusComment);
                }
            },
            TagName => match cp {
                SPACE | LF | TAB | FF => self.state = BeforeAttributeName,
                SLASH => self.state = SelfClosingStartTag,
                GT => {
                    self.state = Data;
                    self.emit_tag();
                }
                NULL => {
                    self.null_error();
                    self.tag.name.push('\u{fffd}');
                }
                EOF => self.tag_eof(),
                _ => self.tag.name.push(character(cp).to_ascii_lowercase()),
            },
            RcdataLessThanSign | RawtextLessThanSign => {
                let (end, fallback) = if self.state == RcdataLessThanSign {
                    (RcdataEndTagOpen, Rcdata)
                } else {
                    (RawtextEndTagOpen, Rawtext)
                };
                if cp == SLASH {
                    self.state = end;
                } else {
                    self.emit_chars("<");
                    self.again(fallback);
                }
            }
            RcdataEndTagOpen
            | RawtextEndTagOpen
            | ScriptDataEndTagOpen
            | ScriptDataEscapedEndTagOpen => {
                let (end, fallback) = match self.state {
                    RcdataEndTagOpen => (RcdataEndTagName, Rcdata),
                    RawtextEndTagOpen => (RawtextEndTagName, Rawtext),
                    ScriptDataEndTagOpen => (ScriptDataEndTagName, ScriptData),
                    _ => (ScriptDataEscapedEndTagName, ScriptDataEscaped),
                };
                if ascii_letter(cp) {
                    self.again(end);
                } else {
                    self.emit_chars("</");
                    self.again(fallback);
                }
            }
            RcdataEndTagName => self.special_end_tag(Rcdata),
            RawtextEndTagName => self.special_end_tag(Rawtext),
            ScriptDataEndTagName => self.special_end_tag(ScriptData),
            ScriptDataEscapedEndTagName => self.special_end_tag(ScriptDataEscaped),
            ScriptDataLessThanSign => match cp {
                SLASH => self.state = ScriptDataEndTagOpen,
                BANG => {
                    self.state = ScriptDataEscapeStart;
                    self.emit_chars("<!");
                }
                _ => {
                    self.emit_chars("<");
                    self.again(ScriptData);
                }
            },
            ScriptDataEscapeStart | ScriptDataEscapeStartDash => {
                if cp == DASH {
                    self.state = if self.state == ScriptDataEscapeStart {
                        ScriptDataEscapeStartDash
                    } else {
                        ScriptDataEscapedDashDash
                    };
                    self.emit_chars("-");
                } else {
                    self.again(ScriptData);
                }
            }
            ScriptDataEscaped
            | ScriptDataEscapedDash
            | ScriptDataEscapedDashDash
            | ScriptDataDoubleEscaped
            | ScriptDataDoubleEscapedDash
            | ScriptDataDoubleEscapedDashDash => {
                let double = matches!(
                    self.state,
                    ScriptDataDoubleEscaped
                        | ScriptDataDoubleEscapedDash
                        | ScriptDataDoubleEscapedDashDash
                );
                let dash = matches!(
                    self.state,
                    ScriptDataEscapedDash | ScriptDataDoubleEscapedDash
                );
                let dash_dash = matches!(
                    self.state,
                    ScriptDataEscapedDashDash | ScriptDataDoubleEscapedDashDash
                );
                let base = if double {
                    ScriptDataDoubleEscaped
                } else {
                    ScriptDataEscaped
                };
                match cp {
                    DASH => {
                        self.state = if double {
                            if dash || dash_dash {
                                ScriptDataDoubleEscapedDashDash
                            } else {
                                ScriptDataDoubleEscapedDash
                            }
                        } else if dash || dash_dash {
                            ScriptDataEscapedDashDash
                        } else {
                            ScriptDataEscapedDash
                        };
                        self.emit_chars("-");
                    }
                    LT => {
                        self.state = if double {
                            ScriptDataDoubleEscapedLessThanSign
                        } else {
                            ScriptDataEscapedLessThanSign
                        };
                        if double {
                            self.emit_chars("<");
                        }
                    }
                    GT if dash_dash => {
                        self.state = ScriptData;
                        self.emit_chars(">");
                    }
                    NULL => {
                        self.null_error();
                        self.state = base;
                        self.emit_chars("\u{fffd}");
                    }
                    EOF => {
                        self.err("eof-in-script-html-comment-like-text");
                        self.eof();
                    }
                    _ => {
                        self.state = base;
                        self.emit_cp(cp);
                    }
                }
            }
            ScriptDataEscapedLessThanSign => match cp {
                SLASH => self.state = ScriptDataEscapedEndTagOpen,
                cp if ascii_letter(cp) => {
                    self.emit_chars("<");
                    self.temporary_buffer.clear();
                    self.again(ScriptDataDoubleEscapeStart);
                }
                _ => {
                    self.emit_chars("<");
                    self.again(ScriptDataEscaped);
                }
            },
            ScriptDataDoubleEscapeStart | ScriptDataDoubleEscapeEnd => {
                let start = self.state == ScriptDataDoubleEscapeStart;
                if whitespace(cp) || cp == SLASH || cp == GT {
                    self.state = if self.temporary_buffer == "script" {
                        if start {
                            ScriptDataDoubleEscaped
                        } else {
                            ScriptDataEscaped
                        }
                    } else if start {
                        ScriptDataEscaped
                    } else {
                        ScriptDataDoubleEscaped
                    };
                    self.emit_cp(cp);
                } else if ascii_letter(cp) {
                    self.temporary_buffer
                        .push(character(cp).to_ascii_lowercase());
                    self.emit_cp(cp);
                } else {
                    self.again(if start {
                        ScriptDataEscaped
                    } else {
                        ScriptDataDoubleEscaped
                    });
                }
            }
            ScriptDataDoubleEscapedLessThanSign => {
                if cp == SLASH {
                    self.temporary_buffer.clear();
                    self.state = ScriptDataDoubleEscapeEnd;
                    self.emit_chars("/");
                } else {
                    self.again(ScriptDataDoubleEscaped);
                }
            }
            BeforeAttributeName => match cp {
                SPACE | LF | TAB | FF => (),
                SLASH | GT | EOF => self.again(AfterAttributeName),
                EQ => {
                    self.err("unexpected-equals-sign-before-attribute-name");
                    self.create_attr("=");
                    self.state = AttributeName;
                }
                _ => {
                    self.create_attr("");
                    self.again(AttributeName);
                }
            },
            AttributeName => match cp {
                SPACE | LF | TAB | FF | SLASH | GT | EOF => {
                    self.leave_attr_name();
                    self.again(AfterAttributeName);
                }
                EQ => {
                    self.leave_attr_name();
                    self.state = BeforeAttributeValue;
                }
                QUOTE | APOS | LT => {
                    self.err("unexpected-character-in-attribute-name");
                    self.attr.as_mut().unwrap().name.push(character(cp));
                }
                NULL => {
                    self.null_error();
                    self.attr.as_mut().unwrap().name.push('\u{fffd}');
                }
                _ => self
                    .attr
                    .as_mut()
                    .unwrap()
                    .name
                    .push(character(cp).to_ascii_lowercase()),
            },
            AfterAttributeName => match cp {
                SPACE | LF | TAB | FF => (),
                SLASH => self.state = SelfClosingStartTag,
                EQ => self.state = BeforeAttributeValue,
                GT => {
                    self.state = Data;
                    self.emit_tag();
                }
                EOF => self.tag_eof(),
                _ => {
                    self.create_attr("");
                    self.again(AttributeName);
                }
            },
            BeforeAttributeValue => match cp {
                SPACE | LF | TAB | FF => (),
                QUOTE => self.state = AttributeValueDoubleQuoted,
                APOS => self.state = AttributeValueSingleQuoted,
                GT => {
                    self.err("missing-attribute-value");
                    self.state = Data;
                    self.emit_tag();
                }
                _ => self.again(AttributeValueUnquoted),
            },
            AttributeValueDoubleQuoted | AttributeValueSingleQuoted => match cp {
                QUOTE if self.state == AttributeValueDoubleQuoted => {
                    self.state = AfterAttributeValueQuoted
                }
                APOS if self.state == AttributeValueSingleQuoted => {
                    self.state = AfterAttributeValueQuoted
                }
                AMP => self.character_reference(true),
                NULL => {
                    self.null_error();
                    self.attr.as_mut().unwrap().value.push('\u{fffd}');
                }
                EOF => self.tag_eof(),
                _ => self.attr.as_mut().unwrap().value.push(character(cp)),
            },
            AttributeValueUnquoted => match cp {
                SPACE | LF | TAB | FF => self.state = BeforeAttributeName,
                AMP => self.character_reference(true),
                GT => {
                    self.state = Data;
                    self.emit_tag();
                }
                NULL => {
                    self.null_error();
                    self.attr.as_mut().unwrap().value.push('\u{fffd}');
                }
                QUOTE | APOS | LT | EQ | GRAVE => {
                    self.err("unexpected-character-in-unquoted-attribute-value");
                    self.attr.as_mut().unwrap().value.push(character(cp));
                }
                EOF => self.tag_eof(),
                _ => self.attr.as_mut().unwrap().value.push(character(cp)),
            },
            AfterAttributeValueQuoted => match cp {
                SPACE | LF | TAB | FF => self.state = BeforeAttributeName,
                SLASH => self.state = SelfClosingStartTag,
                GT => {
                    self.state = Data;
                    self.emit_tag();
                }
                EOF => self.tag_eof(),
                _ => {
                    self.err("missing-whitespace-between-attributes");
                    self.again(BeforeAttributeName);
                }
            },
            SelfClosingStartTag => match cp {
                GT => {
                    self.tag.self_closing = true;
                    self.state = Data;
                    self.emit_tag();
                }
                EOF => self.tag_eof(),
                _ => {
                    self.err("unexpected-solidus-in-tag");
                    self.again(BeforeAttributeName);
                }
            },
            MarkupDeclarationOpen => {
                if self.consume_sequence("--", true) {
                    self.comment.clear();
                    self.state = CommentStart;
                } else if self.consume_sequence("doctype", false) {
                    self.doctype = DoctypeToken::default();
                    self.state = Doctype;
                } else if self.consume_sequence("[CDATA[", true) {
                    if self.in_foreign_node {
                        self.state = CdataSection;
                    } else {
                        self.err("cdata-in-html-content");
                        self.comment = "[CDATA[".into();
                        self.state = BogusComment;
                    }
                } else {
                    self.err("incorrectly-opened-comment");
                    self.comment.clear();
                    self.again(BogusComment);
                }
            }
            BogusComment => match cp {
                GT => {
                    self.state = Data;
                    self.emit_comment();
                }
                EOF => {
                    self.emit_comment();
                    self.eof();
                }
                NULL => {
                    self.null_error();
                    self.comment.push('\u{fffd}');
                }
                _ => self.comment.push(character(cp)),
            },
            CommentStart => match cp {
                DASH => self.state = CommentStartDash,
                GT => {
                    self.err("abrupt-closing-of-empty-comment");
                    self.state = Data;
                    self.emit_comment();
                }
                _ => self.again(Comment),
            },
            CommentStartDash => match cp {
                DASH => self.state = CommentEnd,
                GT => {
                    self.err("abrupt-closing-of-empty-comment");
                    self.state = Data;
                    self.emit_comment();
                }
                EOF => self.comment_eof(),
                _ => {
                    self.comment.push('-');
                    self.again(Comment);
                }
            },
            Comment => match cp {
                DASH => self.state = CommentEndDash,
                LT => {
                    self.comment.push('<');
                    self.state = CommentLessThanSign;
                }
                NULL => {
                    self.null_error();
                    self.comment.push('\u{fffd}');
                }
                EOF => self.comment_eof(),
                _ => self.comment.push(character(cp)),
            },
            CommentLessThanSign => match cp {
                BANG => {
                    self.comment.push('!');
                    self.state = CommentLessThanSignBang;
                }
                LT => self.comment.push('<'),
                _ => self.again(Comment),
            },
            CommentLessThanSignBang => {
                if cp == DASH {
                    self.state = CommentLessThanSignBangDash;
                } else {
                    self.again(Comment);
                }
            }
            CommentLessThanSignBangDash => {
                if cp == DASH {
                    self.state = CommentLessThanSignBangDashDash;
                } else {
                    self.again(CommentEndDash);
                }
            }
            CommentLessThanSignBangDashDash => {
                if cp != GT && cp != EOF {
                    self.err("nested-comment");
                }
                self.again(CommentEnd);
            }
            CommentEndDash => match cp {
                DASH => self.state = CommentEnd,
                EOF => self.comment_eof(),
                _ => {
                    self.comment.push('-');
                    self.again(Comment);
                }
            },
            CommentEnd => match cp {
                GT => {
                    self.state = Data;
                    self.emit_comment();
                }
                BANG => self.state = CommentEndBang,
                DASH => self.comment.push('-'),
                EOF => self.comment_eof(),
                _ => {
                    self.comment.push_str("--");
                    self.again(Comment);
                }
            },
            CommentEndBang => match cp {
                DASH => {
                    self.comment.push_str("--!");
                    self.state = CommentEndDash;
                }
                GT => {
                    self.err("incorrectly-closed-comment");
                    self.state = Data;
                    self.emit_comment();
                }
                EOF => self.comment_eof(),
                _ => {
                    self.comment.push_str("--!");
                    self.again(Comment);
                }
            },
            Doctype => match cp {
                SPACE | LF | TAB | FF => self.state = BeforeDoctypeName,
                GT => self.again(BeforeDoctypeName),
                EOF => self.doctype_eof(),
                _ => {
                    self.err("missing-whitespace-before-doctype-name");
                    self.again(BeforeDoctypeName);
                }
            },
            BeforeDoctypeName => match cp {
                SPACE | LF | TAB | FF => (),
                NULL => {
                    self.null_error();
                    self.doctype.name = Some("\u{fffd}".into());
                    self.state = DoctypeName;
                }
                GT => {
                    self.err("missing-doctype-name");
                    self.doctype.force_quirks = true;
                    self.emit_doctype();
                    self.state = Data;
                }
                EOF => self.doctype_eof(),
                _ => {
                    self.doctype.name = Some(character(cp).to_ascii_lowercase().into());
                    self.state = DoctypeName;
                }
            },
            DoctypeName => match cp {
                SPACE | LF | TAB | FF => self.state = AfterDoctypeName,
                GT => {
                    self.state = Data;
                    self.emit_doctype();
                }
                NULL => {
                    self.null_error();
                    self.doctype.name.as_mut().unwrap().push('\u{fffd}');
                }
                EOF => self.doctype_eof(),
                _ => self
                    .doctype
                    .name
                    .as_mut()
                    .unwrap()
                    .push(character(cp).to_ascii_lowercase()),
            },
            AfterDoctypeName => match cp {
                SPACE | LF | TAB | FF => (),
                GT => {
                    self.state = Data;
                    self.emit_doctype();
                }
                EOF => self.doctype_eof(),
                _ => {
                    if self.consume_sequence("public", false) {
                        self.state = AfterDoctypePublicKeyword;
                    } else if self.consume_sequence("system", false) {
                        self.state = AfterDoctypeSystemKeyword;
                    } else {
                        self.err("invalid-character-sequence-after-doctype-name");
                        self.doctype.force_quirks = true;
                        self.again(BogusDoctype);
                    }
                }
            },
            AfterDoctypePublicKeyword
            | BeforeDoctypePublicIdentifier
            | AfterDoctypeSystemKeyword
            | BeforeDoctypeSystemIdentifier => {
                let public = matches!(
                    self.state,
                    AfterDoctypePublicKeyword | BeforeDoctypePublicIdentifier
                );
                let after_keyword = matches!(
                    self.state,
                    AfterDoctypePublicKeyword | AfterDoctypeSystemKeyword
                );
                match cp {
                    SPACE | LF | TAB | FF => {
                        self.state = if public {
                            BeforeDoctypePublicIdentifier
                        } else {
                            BeforeDoctypeSystemIdentifier
                        }
                    }
                    QUOTE | APOS => {
                        if after_keyword {
                            self.err(if public {
                                "missing-whitespace-after-doctype-public-keyword"
                            } else {
                                "missing-whitespace-after-doctype-system-keyword"
                            });
                        }
                        if public {
                            self.doctype.public_id = Some(String::new());
                            self.state = if cp == QUOTE {
                                DoctypePublicIdentifierDoubleQuoted
                            } else {
                                DoctypePublicIdentifierSingleQuoted
                            };
                        } else {
                            self.doctype.system_id = Some(String::new());
                            self.state = if cp == QUOTE {
                                DoctypeSystemIdentifierDoubleQuoted
                            } else {
                                DoctypeSystemIdentifierSingleQuoted
                            };
                        }
                    }
                    GT => {
                        self.err(if public {
                            "missing-doctype-public-identifier"
                        } else {
                            "missing-doctype-system-identifier"
                        });
                        self.doctype.force_quirks = true;
                        self.state = Data;
                        self.emit_doctype();
                    }
                    EOF => self.doctype_eof(),
                    _ => {
                        self.err(if public {
                            "missing-quote-before-doctype-public-identifier"
                        } else {
                            "missing-quote-before-doctype-system-identifier"
                        });
                        self.doctype.force_quirks = true;
                        self.again(BogusDoctype);
                    }
                }
            }
            DoctypePublicIdentifierDoubleQuoted
            | DoctypePublicIdentifierSingleQuoted
            | DoctypeSystemIdentifierDoubleQuoted
            | DoctypeSystemIdentifierSingleQuoted => {
                let public = matches!(
                    self.state,
                    DoctypePublicIdentifierDoubleQuoted | DoctypePublicIdentifierSingleQuoted
                );
                let double = matches!(
                    self.state,
                    DoctypePublicIdentifierDoubleQuoted | DoctypeSystemIdentifierDoubleQuoted
                );
                match cp {
                    QUOTE if double => {
                        self.state = if public {
                            AfterDoctypePublicIdentifier
                        } else {
                            AfterDoctypeSystemIdentifier
                        }
                    }
                    APOS if !double => {
                        self.state = if public {
                            AfterDoctypePublicIdentifier
                        } else {
                            AfterDoctypeSystemIdentifier
                        }
                    }
                    NULL => {
                        self.null_error();
                        if public {
                            self.doctype.public_id.as_mut().unwrap().push('\u{fffd}');
                        } else {
                            self.doctype.system_id.as_mut().unwrap().push('\u{fffd}');
                        }
                    }
                    GT => {
                        self.err(if public {
                            "abrupt-doctype-public-identifier"
                        } else {
                            "abrupt-doctype-system-identifier"
                        });
                        self.doctype.force_quirks = true;
                        self.emit_doctype();
                        self.state = Data;
                    }
                    EOF => self.doctype_eof(),
                    _ => {
                        if public {
                            self.doctype.public_id.as_mut().unwrap().push(character(cp));
                        } else {
                            self.doctype.system_id.as_mut().unwrap().push(character(cp));
                        }
                    }
                }
            }
            AfterDoctypePublicIdentifier | BetweenDoctypePublicAndSystemIdentifiers => match cp {
                SPACE | LF | TAB | FF => self.state = BetweenDoctypePublicAndSystemIdentifiers,
                GT => {
                    self.state = Data;
                    self.emit_doctype();
                }
                QUOTE | APOS => {
                    if self.state == AfterDoctypePublicIdentifier {
                        self.err(
                            "missing-whitespace-between-doctype-public-and-system-identifiers",
                        );
                    }
                    self.doctype.system_id = Some(String::new());
                    self.state = if cp == QUOTE {
                        DoctypeSystemIdentifierDoubleQuoted
                    } else {
                        DoctypeSystemIdentifierSingleQuoted
                    };
                }
                EOF => self.doctype_eof(),
                _ => {
                    self.err("missing-quote-before-doctype-system-identifier");
                    self.doctype.force_quirks = true;
                    self.again(BogusDoctype);
                }
            },
            AfterDoctypeSystemIdentifier => match cp {
                SPACE | LF | TAB | FF => (),
                GT => {
                    self.state = Data;
                    self.emit_doctype();
                }
                EOF => self.doctype_eof(),
                _ => {
                    self.err("unexpected-character-after-doctype-system-identifier");
                    self.again(BogusDoctype);
                }
            },
            BogusDoctype => match cp {
                GT => {
                    self.state = Data;
                    self.emit_doctype();
                }
                NULL => self.null_error(),
                EOF => {
                    self.emit_doctype();
                    self.eof();
                }
                _ => (),
            },
            CdataSection => match cp {
                BRACKET => self.state = CdataSectionBracket,
                EOF => {
                    self.err("eof-in-cdata");
                    self.eof();
                }
                _ => self.emit_cp(cp),
            },
            CdataSectionBracket => {
                if cp == BRACKET {
                    self.state = CdataSectionEnd;
                } else {
                    self.emit_chars("]");
                    self.again(CdataSection);
                }
            }
            CdataSectionEnd => match cp {
                GT => self.state = Data,
                BRACKET => self.emit_chars("]"),
                _ => {
                    self.emit_chars("]]");
                    self.again(CdataSection);
                }
            },
        }
    }
}

fn character(cp: u32) -> char {
    char::from_u32(cp).unwrap_or('\u{fffd}')
}
fn ascii_letter(cp: u32) -> bool {
    (65..=90).contains(&cp) || (97..=122).contains(&cp)
}
fn whitespace(cp: u32) -> bool {
    matches!(cp, SPACE | TAB | LF | FF | 13)
}
fn is_control(cp: u32) -> bool {
    (1..=8).contains(&cp) || cp == 11 || (14..=31).contains(&cp) || (127..=159).contains(&cp)
}
fn is_noncharacter(cp: u32) -> bool {
    (0xfdd0..=0xfdef).contains(&cp) || cp & 0xffff == 0xfffe || cp & 0xffff == 0xffff
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(html: &str, mode: TextMode, tag: &str) -> Vec<Token> {
        let mut tokenizer = Tokenizer::new(html);
        tokenizer.set_mode(mode, tag);
        let mut result = Vec::new();
        loop {
            let token = tokenizer.next_token();
            if token == Token::Eof {
                break;
            }
            result.push(token);
        }
        result
    }
    fn text(tokens: &[Token]) -> String {
        tokens
            .iter()
            .filter_map(|token| match token {
                Token::Character(value)
                | Token::Whitespace(value)
                | Token::NullCharacter(value) => Some(value.as_str()),
                _ => None,
            })
            .collect()
    }
    #[test]
    fn attributes_and_character_references_follow_parse5_rules() {
        let result = tokens(
            "<A X='&notit;&not;&NotEqualTilde;' x=second y=&#x80; z=\"&amp=ok\"/>",
            TextMode::Data,
            "",
        );
        let Token::StartTag(tag) = &result[0] else {
            panic!("missing tag")
        };
        assert_eq!(tag.name, "a");
        assert_eq!(tag.attr("x"), Some("&notit;¬≂\u{338}"));
        assert_eq!(tag.attr("y"), Some("€"));
        assert_eq!(tag.attr("z"), Some("&amp=ok"));
        assert_eq!(tag.attrs.len(), 3);
        assert!(tag.self_closing);
    }
    #[test]
    fn text_modes_require_an_appropriate_end_tag() {
        let result = tokens(
            "one</textareaX>&amp;</TEXTAREA x=y>tail",
            TextMode::Rcdata,
            "textarea",
        );
        assert_eq!(text(&result), "one</textareaX>&tail");
        assert!(
            result
                .iter()
                .any(|token| matches!(token, Token::EndTag(tag) if tag.name == "textarea"))
        );
        assert_eq!(
            text(&tokens("<b>&amp;\0</STYLE>", TextMode::Rawtext, "style")),
            "<b>&amp;�"
        );
        assert_eq!(
            text(&tokens("<b>&amp;\0</p>", TextMode::Plaintext, "")),
            "<b>&amp;�</p>"
        );
    }
    #[test]
    fn script_double_escape_does_not_end_at_the_inner_script_end_tag() {
        let result = tokens(
            "<!--<script>x</script>y--></SCRIPT>z",
            TextMode::ScriptData,
            "script",
        );
        assert_eq!(text(&result), "<!--<script>x</script>y-->z");
        let ends = result
            .iter()
            .filter(|token| matches!(token, Token::EndTag(_)))
            .count();
        assert_eq!(ends, 1);
    }
    #[test]
    fn comments_doctypes_and_cdata() {
        let result = tokens(
            "<!--a<!--b--!><!DOCTYPE html PUBLIC 'pub' 'sys'>",
            TextMode::Data,
            "",
        );
        assert_eq!(result[0], Token::Comment("a<!--b".into()));
        assert_eq!(
            result[1],
            Token::Doctype(DoctypeToken {
                name: Some("html".into()),
                public_id: Some("pub".into()),
                system_id: Some("sys".into()),
                force_quirks: false,
            })
        );
        assert_eq!(
            tokens("<![CDATA[a]]>", TextMode::Data, "")[0],
            Token::Comment("[CDATA[a]]".into())
        );
        let mut tokenizer = Tokenizer::new("<![CDATA[a\0]]]>b");
        tokenizer.set_in_foreign_node(true);
        let mut result = Vec::new();
        loop {
            let token = tokenizer.next_token();
            if token == Token::Eof {
                break;
            }
            result.push(token);
        }
        assert_eq!(text(&result), "a\0]b");
    }
    #[test]
    fn preprocessing_and_eof_recovery() {
        assert_eq!(
            text(&tokens("a\r\nb\rc\n\0", TextMode::Data, "")),
            "a\nb\nc\n\0"
        );
        assert_eq!(
            tokens("<!--x--", TextMode::Data, "")[0],
            Token::Comment("x".into())
        );
        assert!(tokens("<a x='unfinished", TextMode::Data, "").is_empty());
        assert_eq!(
            text(&tokens("&notin;&bogus;&#0;&#x110000;", TextMode::Data, "")),
            "∉&bogus;��"
        );
    }

    // The fixture repository remains external; tools/test_tokenizer.py runs
    // this ignored test with RP5_HTML5LIB_TOKENIZER pointing at its directory.
    #[test]
    #[ignore = "requires the pinned external html5lib tokenizer fixtures"]
    fn html5lib_tokenizer_fixture_corpus() {
        use serde_json::{Value, json};

        fn decode_double_escaped(text: &str) -> Option<String> {
            let bytes = text.as_bytes();
            let mut pos = 0;
            let mut decoded = String::new();
            while pos < bytes.len() {
                if bytes.get(pos..pos + 2) == Some(b"\\u") && pos + 6 <= bytes.len() {
                    let mut cp = u32::from_str_radix(&text[pos + 2..pos + 6], 16).ok()?;
                    pos += 6;
                    if (0xd800..=0xdbff).contains(&cp) {
                        if bytes.get(pos..pos + 2) != Some(b"\\u") || pos + 6 > bytes.len() {
                            return None;
                        }
                        let low = u32::from_str_radix(&text[pos + 2..pos + 6], 16).ok()?;
                        if !(0xdc00..=0xdfff).contains(&low) {
                            return None;
                        }
                        cp = 0x10000 + ((cp - 0xd800) << 10) + (low - 0xdc00);
                        pos += 6;
                    }
                    decoded.push(char::from_u32(cp)?);
                } else {
                    let ch = text[pos..].chars().next()?;
                    decoded.push(ch);
                    pos += ch.len_utf8();
                }
            }
            Some(decoded)
        }
        fn decode_values(value: &mut Value) -> bool {
            match value {
                Value::String(text) => match decode_double_escaped(text) {
                    Some(decoded) => {
                        *text = decoded;
                        true
                    }
                    None => false,
                },
                Value::Array(values) => values.iter_mut().all(decode_values),
                Value::Object(map) => map.values_mut().all(decode_values),
                _ => true,
            }
        }
        fn fixture_tokens(tokenizer: &mut Tokenizer) -> Vec<Value> {
            let mut result = Vec::new();
            let mut chars = String::new();
            loop {
                let token = tokenizer.next_token();
                match token {
                    Token::Character(value)
                    | Token::Whitespace(value)
                    | Token::NullCharacter(value) => {
                        chars.push_str(&value);
                        continue;
                    }
                    _ => (),
                }
                if !chars.is_empty() {
                    result.push(json!(["Character", std::mem::take(&mut chars)]));
                }
                match token {
                    Token::StartTag(tag) => {
                        let attrs: serde_json::Map<_, _> = tag
                            .attrs
                            .into_iter()
                            .map(|attr| (attr.name, json!(attr.value)))
                            .collect();
                        result.push(if tag.self_closing {
                            json!(["StartTag", tag.name, attrs, true])
                        } else {
                            json!(["StartTag", tag.name, attrs])
                        });
                    }
                    Token::EndTag(tag) => result.push(json!(["EndTag", tag.name])),
                    Token::Comment(data) => result.push(json!(["Comment", data])),
                    Token::Doctype(tag) => result.push(json!([
                        "DOCTYPE",
                        tag.name,
                        tag.public_id,
                        tag.system_id,
                        !tag.force_quirks,
                    ])),
                    Token::Eof => break,
                    _ => unreachable!(),
                }
            }
            result
        }

        let directory = std::env::var("RP5_HTML5LIB_TOKENIZER")
            .expect("set RP5_HTML5LIB_TOKENIZER to the html5lib tokenizer fixture directory");
        let mut paths: Vec<_> = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "test")
            })
            .collect();
        paths.sort();
        assert!(
            !paths.is_empty(),
            "the fixture directory contains no .test files"
        );
        let mut total = 0;
        let mut skipped_surrogates = 0;
        let mut failures = Vec::new();
        for path in paths {
            let fixtures: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            // xmlViolationTests are optional XML coercion tests, not parse5 behavior.
            let Some(cases) = fixtures.get("tests").and_then(Value::as_array) else {
                continue;
            };
            for original_case in cases {
                let mut case = original_case.clone();
                // Rust strings cannot hold isolated UTF-16 surrogates.
                if case["doubleEscaped"] == true && !decode_values(&mut case) {
                    skipped_surrogates += 1;
                    continue;
                }
                let input = case["input"].as_str().unwrap();
                let states = case
                    .get("initialStates")
                    .and_then(Value::as_array)
                    .cloned()
                    .unwrap_or(vec![json!("Data state")]);
                for state in states {
                    let initial = match state.as_str().unwrap() {
                        "Data state" => State::Data,
                        "RCDATA state" => State::Rcdata,
                        "RAWTEXT state" => State::Rawtext,
                        "Script data state" => State::ScriptData,
                        "PLAINTEXT state" => State::Plaintext,
                        "CDATA section state" => State::CdataSection,
                        other => panic!("unknown initial state {other}"),
                    };
                    let mut tokenizer = Tokenizer::new(input);
                    tokenizer.state = initial;
                    tokenizer.last_start_tag_name = case
                        .get("lastStartTag")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .into();
                    let actual = fixture_tokens(&mut tokenizer);
                    total += 1;
                    let mut expected_errors: Vec<_> = case
                        .get("errors")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .map(|error| error["code"].as_str().unwrap().to_owned())
                        .collect();
                    let mut actual_errors: Vec<_> = tokenizer
                        .errors
                        .iter()
                        .map(|error| error.code.clone())
                        .collect();
                    expected_errors.sort();
                    actual_errors.sort();
                    if json!(actual) != case["output"] || expected_errors != actual_errors {
                        failures.push(json!({
                            "file": path, "description": case["description"], "state": state, "input": input,
                            "expected": case["output"], "actual": actual,
                            "expected_errors": expected_errors, "actual_errors": actual_errors,
                        }));
                    }
                }
            }
        }
        println!(
            "html5lib tokenizer: {total} cases, {} mismatches, {skipped_surrogates} isolated-surrogate inputs skipped",
            failures.len()
        );
        assert!(
            failures.is_empty(),
            "{}",
            serde_json::to_string_pretty(&failures).unwrap()
        );
    }
}

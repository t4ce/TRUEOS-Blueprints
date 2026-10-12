//! Native translation of parse5's tree construction, open element stack and
//! active formatting element algorithms. See the vendored MIT-licensed p5/src.
use alloc::{string::{String, ToString}, vec, vec::Vec};
use crate::dom::{Document, DocumentMode, Namespace, NodeId, NodeKind};
use crate::token::{Attribute, DoctypeToken, TagToken, Token};
use crate::tokenizer::{TextMode, Tokenizer};
use crate::{FragmentContext, ParseError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Initial, BeforeHtml, BeforeHead, InHead, AfterHead, InBody, Text,
    InTable, InTableText, InCaption, InColumnGroup, InTableBody, InRow, InCell,
    InSelect, InSelectInTable, InTemplate, AfterBody, InFrameset, AfterFrameset,
    AfterAfterBody, AfterAfterFrameset,
}
#[derive(Clone)]
struct Formatting { node: NodeId, token: TagToken }
#[derive(Clone, Copy)]
enum Scope { Normal, Button, List, Table, Select }

pub fn parse(html: &str) -> Document { Parser::new(html, None, false).run() }
pub fn parse_fragment(html: &str, context: Option<FragmentContext>) -> Document {
    Parser::new(html, Some(context.unwrap_or_else(|| FragmentContext {
        name: "template".into(), namespace: Namespace::Html, attrs: Vec::new(),
    })), true).run()
}

struct Parser<'a> {
    doc: Document, tokenizer: Tokenizer<'a>, stack: Vec<NodeId>,
    formatting: Vec<Option<Formatting>>, templates: Vec<Mode>,
    mode: Mode, original: Mode, context: Option<NodeId>, fragment: bool,
    head: Option<NodeId>, form: Option<NodeId>, frameset_ok: bool,
    skip_newline: bool, foster: bool, pending: Vec<(String, bool)>, stopped: bool,
}
impl<'a> Parser<'a> {
    fn new(html: &'a str, context: Option<FragmentContext>, fragment: bool) -> Self {
        let mut p = Self { doc: Document::new(fragment), tokenizer: Tokenizer::new(html),
            stack: Vec::new(), formatting: Vec::new(), templates: Vec::new(),
            mode: Mode::Initial, original: Mode::Initial, context: None, fragment,
            head: None, form: None, frameset_ok: true, skip_newline: false,
            foster: false, pending: Vec::new(), stopped: false };
        if let Some(ctx) = context {
            let name = ctx.name.clone();
            let ns = ctx.namespace;
            let id = p.doc.add(NodeKind::Element { name: ctx.name, namespace: ns,
                attrs: ctx.attrs, template_content: None });
            p.context = Some(id);
            if ns == Namespace::Html {
                let text_mode = match name.as_str() {
                    "title" | "textarea" => TextMode::Rcdata,
                    "style" | "xmp" | "iframe" | "noembed" | "noframes" | "noscript" => TextMode::Rawtext,
                    "script" => TextMode::ScriptData, "plaintext" => TextMode::Plaintext,
                    _ => TextMode::Data,
                };
                p.tokenizer.set_mode(text_mode, &name);
                if name == "template" { p.templates.push(Mode::InTemplate); }
                if name == "form" { p.form = Some(id); }
            }
            p.fake("html");
            p.reset_mode();
        }
        p
    }
    fn run(mut self) -> Document {
        while !self.stopped {
            let current = self.adjusted_current();
            let foreign = self.doc.namespace(current) != Namespace::Html && !self.integration(current, false);
            self.tokenizer.set_in_foreign_node(foreign);
            let token = self.tokenizer.next_token();
            self.process(token);
        }
        self.doc.errors.append(&mut self.tokenizer.errors);
        if self.fragment {
            if let Some(html) = self.stack.first().copied().or_else(|| self.doc.nodes[self.doc.root].children.first().copied()) {
                let children = self.doc.nodes[html].children.clone();
                for child in children { self.doc.append(self.doc.root, child); }
                self.doc.detach(html);
            }
        }
        self.doc
    }
    fn error(&mut self, code: &str) { self.doc.errors.push(ParseError { code: code.into(), offset: 0 }); }
    fn current(&self) -> NodeId { self.stack.last().copied().unwrap_or(self.doc.root) }
    fn adjusted_current(&self) -> NodeId {
        if self.stack.len() == 1 { self.context.unwrap_or(self.current()) } else { self.current() }
    }
    fn name(&self) -> &str { self.doc.name(self.current()) }
    fn is_html(&self, id: NodeId, name: &str) -> bool {
        self.doc.namespace(id) == Namespace::Html && self.doc.name(id) == name
    }
    fn in_template(&self) -> bool { self.stack.iter().any(|&id| self.is_html(id, "template")) }
    fn target(&self, node: NodeId) -> NodeId {
        if let NodeKind::Element { template_content: Some(content), .. } = &self.doc.nodes[node].kind { *content } else { node }
    }
    fn foster_location(&self) -> (NodeId, Option<NodeId>) {
        for (i, &id) in self.stack.iter().enumerate().rev() {
            if self.is_html(id, "template") { return (self.target(id), None); }
            if self.is_html(id, "table") {
                if let Some(parent) = self.doc.nodes[id].parent { return (parent, Some(id)); }
                return (self.stack.get(i.wrapping_sub(1)).copied().unwrap_or(self.doc.root), None);
            }
        }
        (self.stack.first().copied().unwrap_or(self.doc.root), None)
    }
    fn location(&self) -> (NodeId, Option<NodeId>) {
        if self.foster && table_structure(self.name()) { self.foster_location() }
        else { (self.target(self.current()), None) }
    }
    fn attach(&mut self, node: NodeId) {
        let (parent, before) = self.location();
        if let Some(before) = before { self.doc.insert_before(parent, node, before); }
        else { self.doc.append(parent, node); }
    }
    fn create(&mut self, token: &TagToken, namespace: Namespace) -> NodeId {
        let content = if namespace == Namespace::Html && token.name == "template" { Some(self.doc.add(NodeKind::Fragment)) } else { None };
        self.doc.add(NodeKind::Element { name: token.name.clone(), namespace,
            attrs: token.attrs.clone(), template_content: content })
    }
    fn insert(&mut self, token: &TagToken, ns: Namespace, push: bool) -> NodeId {
        let node = self.create(token, ns); self.attach(node); if push { self.stack.push(node); } node
    }
    fn fake(&mut self, name: &str) -> NodeId { self.insert(&tag(name), Namespace::Html, true) }
    fn text(&mut self, chars: &str) {
        let (parent, before) = self.location(); self.doc.append_text(parent, chars, before);
    }
    fn comment(&mut self, chars: String, parent: NodeId) {
        let node = self.doc.add(NodeKind::Comment(chars)); self.doc.append(parent, node);
    }
    fn pop(&mut self) { self.stack.pop(); }
    fn pop_name(&mut self, name: &str) {
        if let Some(pos) = self.stack.iter().rposition(|&id| self.is_html(id, name)) { self.stack.truncate(pos); }
    }
    fn remove_stack(&mut self, node: NodeId) { if let Some(pos) = self.stack.iter().position(|&id| id == node) { self.stack.remove(pos); } }
    fn scope(&self, name: &str, scope: Scope) -> bool {
        for &id in self.stack.iter().rev() {
            let ns = self.doc.namespace(id); let n = self.doc.name(id);
            if ns == Namespace::Html {
                if n == name { return true; }
                let stop = match scope {
                    Scope::Table => matches!(n, "table" | "html"),
                    Scope::Select => !matches!(n, "option" | "optgroup"),
                    _ => scope_html(n) || matches!(scope, Scope::Button) && n == "button"
                        || matches!(scope, Scope::List) && matches!(n, "ol" | "ul"),
                };
                if stop { return false; }
            } else if !matches!(scope, Scope::Table | Scope::Select) && special_foreign(ns, n) { return false; }
        }
        true
    }
    fn implied(&mut self, exclusion: Option<&str>, thorough: bool) {
        loop { let name = self.name();
            if exclusion == Some(name) || !implied_tag(name, thorough || exclusion.is_some()) { break; }
            self.pop();
        }
    }
    fn close_p(&mut self) { self.implied(Some("p"), false); self.pop_name("p"); }
    fn clear_back(&mut self, names: &[&str]) {
        if let Some(pos) = self.stack.iter().rposition(|&id| self.doc.namespace(id) == Namespace::Html && names.contains(&self.doc.name(id))) { self.stack.truncate(pos + 1); }
    }
    fn clear_marker(&mut self) {
        while let Some(entry) = self.formatting.pop() { if entry.is_none() { break; } }
    }
    fn active(&self, name: &str) -> Option<usize> {
        for (i, entry) in self.formatting.iter().enumerate().rev() {
            match entry { Some(e) if self.doc.name(e.node) == name => return Some(i), None => break, _ => () }
        }
        None
    }
    fn push_formatting(&mut self, node: NodeId, token: &TagToken) {
        let mut equal = Vec::new();
        for (i, e) in self.formatting.iter().enumerate().rev() {
            match e {
                None => break,
                Some(e) if e.token.name == token.name && self.doc.namespace(e.node) == self.doc.namespace(node)
                    && attrs_equal(&e.token.attrs, &token.attrs) => equal.push(i),
                _ => (),
            }
        }
        if equal.len() >= 3 { self.formatting.remove(*equal.last().unwrap()); }
        self.formatting.push(Some(Formatting { node, token: token.clone() }));
    }
    fn reconstruct(&mut self) {
        let mut start = self.formatting.len();
        while start > 0 {
            match &self.formatting[start - 1] {
                Some(e) if !self.stack.contains(&e.node) => start -= 1,
                _ => break,
            }
        }
        for i in start..self.formatting.len() {
            if let Some(entry) = self.formatting[i].clone() {
                let ns = self.doc.namespace(entry.node);
                let node = self.insert(&entry.token, ns, true);
                self.formatting[i] = Some(Formatting { node, ..entry });
            }
        }
    }
    fn adopt_attrs(&mut self, node: NodeId, attrs: &[Attribute]) {
        if let NodeKind::Element { attrs: target, .. } = &mut self.doc.nodes[node].kind {
            for attr in attrs { if !target.iter().any(|a| a.name == attr.name) { target.push(attr.clone()); } }
        }
    }
    fn special(&self, node: NodeId) -> bool {
        let n = self.doc.name(node); let ns = self.doc.namespace(node);
        if ns != Namespace::Html { special_foreign(ns, n) } else { special_html(n) }
    }
    fn generic_end(&mut self, token: &TagToken) {
        for i in (1..self.stack.len()).rev() {
            let id = self.stack[i];
            if self.is_html(id, &token.name) {
                self.implied(Some(&token.name), false);
                if self.stack.len() > i { self.stack.truncate(i); }
                break;
            }
            if self.special(id) { break; }
        }
    }
    fn adoption(&mut self, token: &TagToken) {
        for _ in 0..8 {
            let Some(fi) = self.active(&token.name) else { self.generic_end(token); break };
            let entry = self.formatting[fi].clone().unwrap();
            let Some(si) = self.stack.iter().position(|&n| n == entry.node) else { self.formatting.remove(fi); break };
            if !self.scope(&token.name, Scope::Normal) { break; }
            let Some(bi) = (si + 1..self.stack.len()).find(|&i| self.special(self.stack[i])) else {
                self.stack.truncate(si); self.formatting.remove(fi); break;
            };
            let furthest = self.stack[bi];
            let common = if si > 0 { self.stack[si - 1] } else { self.doc.root };
            let mut last = furthest;
            let mut bookmark = fi;
            let mut index = bi;
            let mut inner = 0;
            while index > si + 1 {
                index -= 1; inner += 1;
                let node = self.stack[index];
                let mut ai = self.formatting.iter().position(|e| e.as_ref().is_some_and(|e| e.node == node));
                if inner > 3 {
                    if let Some(ai_value) = ai { self.formatting.remove(ai_value); if ai_value < bookmark { bookmark -= 1; } ai = None; }
                }
                let Some(ai) = ai else { self.stack.remove(index); continue };
                let active = self.formatting[ai].clone().unwrap();
                let fresh = self.create(&active.token, self.doc.namespace(active.node));
                self.stack[index] = fresh;
                self.formatting[ai] = Some(Formatting { node: fresh, ..active });
                if last == furthest { bookmark = ai + 1; }
                self.doc.append(fresh, last); last = fresh;
            }
            self.doc.detach(last);
            if table_structure(self.doc.name(common)) {
                let (parent, before) = self.foster_location();
                if let Some(before) = before { self.doc.insert_before(parent, last, before); } else { self.doc.append(parent, last); }
            } else { self.doc.append(self.target(common), last); }
            let fresh = self.create(&entry.token, self.doc.namespace(entry.node));
            let children = self.doc.nodes[furthest].children.clone();
            for child in children { self.doc.append(fresh, child); }
            self.doc.append(furthest, fresh);
            if let Some(pos) = self.formatting.iter().position(|e| e.as_ref().is_some_and(|e| e.node == entry.node)) {
                self.formatting.remove(pos); if pos < bookmark { bookmark -= 1; }
            }
            self.formatting.insert(bookmark.min(self.formatting.len()), Some(Formatting { node: fresh, token: entry.token }));
            self.remove_stack(entry.node);
            if let Some(pos) = self.stack.iter().position(|&n| n == furthest) { self.stack.insert(pos + 1, fresh); }
        }
    }
    fn integration(&self, node: NodeId, html_only: bool) -> bool {
        let name = self.doc.name(node); let ns = self.doc.namespace(node);
        if ns == Namespace::Svg && matches!(name, "foreignObject" | "desc" | "title") { return true; }
        if ns == Namespace::MathMl {
            if !html_only && matches!(name, "mi" | "mo" | "mn" | "ms" | "mtext") { return true; }
            if name == "annotation-xml" {
                if let Some((_, _, attrs)) = self.doc.element(node) {
                    return attrs.iter().any(|a| a.name == "encoding" && (a.value.eq_ignore_ascii_case("text/html") || a.value.eq_ignore_ascii_case("application/xhtml+xml")));
                }
            }
        }
        false
    }
    fn foreign(&mut self, token: Token) -> Option<Token> {
        let current = self.adjusted_current(); let ns = self.doc.namespace(current);
        if ns == Namespace::Html { return Some(token); }
        match token {
            Token::StartTag(mut t) => {
                if t.name == "svg" && ns == Namespace::MathMl && self.doc.name(current) == "annotation-xml" { return Some(Token::StartTag(t)); }
                if self.integration(current, false) && (!matches!(t.name.as_str(), "mglyph" | "malignmark") || self.integration(current, true)) { return Some(Token::StartTag(t)); }
                if foreign_exit(&t) {
                    while self.doc.namespace(self.current()) != Namespace::Html && !self.integration(self.current(), false) { self.pop(); if self.stack.is_empty() { break; } }
                    return Some(Token::StartTag(t));
                }
                adjust_foreign(&mut t, ns, true);
                self.insert(&t, ns, !t.self_closing); None
            }
            Token::EndTag(t) => {
                if matches!(t.name.as_str(), "p" | "br") {
                    while self.doc.namespace(self.current()) != Namespace::Html && !self.integration(self.current(), false) { self.pop(); if self.stack.is_empty() { break; } }
                    return Some(Token::EndTag(t));
                }
                for i in (1..self.stack.len()).rev() {
                    let id = self.stack[i];
                    if self.doc.namespace(id) == Namespace::Html { return Some(Token::EndTag(t)); }
                    if self.doc.name(id).eq_ignore_ascii_case(&t.name) { self.stack.truncate(i); break; }
                }
                None
            }
            Token::Character(s) if !self.integration(current, false) => { self.text(&s); self.frameset_ok = false; None }
            Token::Whitespace(s) if !self.integration(current, false) => { self.text(&s); None }
            Token::NullCharacter(s) if !self.integration(current, false) => { self.text(&s.replace('\0', "\u{fffd}")); None }
            Token::Comment(s) => { self.comment(s, self.target(self.current())); None }
            Token::Doctype(_) => None,
            token => Some(token),
        }
    }
    fn reset_mode(&mut self) {
        for i in (0..self.stack.len()).rev() {
            let node = if i == 0 { self.context.unwrap_or(self.stack[i]) } else { self.stack[i] };
            self.mode = match self.doc.name(node) {
                "tr" => Mode::InRow, "tbody" | "thead" | "tfoot" => Mode::InTableBody,
                "caption" => Mode::InCaption, "colgroup" => Mode::InColumnGroup,
                "table" => Mode::InTable, "body" => Mode::InBody, "frameset" => Mode::InFrameset,
                "select" => {
                    let mut mode = Mode::InSelect;
                    for &node in self.stack[..i].iter().rev() {
                        if self.doc.name(node) == "template" { break; }
                        if self.doc.name(node) == "table" { mode = Mode::InSelectInTable; break; }
                    } mode
                },
                "template" => self.templates.last().copied().unwrap_or(Mode::InTemplate),
                "html" => if self.head.is_some() { Mode::AfterHead } else { Mode::BeforeHead },
                "td" | "th" if i > 0 => Mode::InCell,
                "head" if i > 0 => Mode::InHead,
                _ => continue,
            }; return;
        }
        self.mode = Mode::InBody;
    }
    fn switch_text(&mut self, tag: &TagToken, mode: TextMode) {
        self.insert(tag, Namespace::Html, true); self.tokenizer.set_mode(mode, &tag.name);
        self.original = self.mode; self.mode = Mode::Text;
    }
    fn close_template(&mut self) {
        if !self.in_template() { self.error("end-tag-without-matching-open-element"); return; }
        self.implied(None, true); self.pop_name("template"); self.clear_marker(); self.templates.pop(); self.reset_mode();
    }
    fn close_cell(&mut self) {
        self.implied(None, false);
        if let Some(pos) = self.stack.iter().rposition(|&id| self.doc.namespace(id) == Namespace::Html && matches!(self.doc.name(id), "td" | "th")) { self.stack.truncate(pos); }
        self.clear_marker(); self.mode = Mode::InRow;
    }
    fn process(&mut self, mut token: Token) {
        if self.skip_newline {
            self.skip_newline = false;
            if let Token::Whitespace(s) = &mut token { if s.starts_with('\n') { s.remove(0); if s.is_empty() { return; } } }
        }
        let Some(token) = self.foreign(token) else { return };
        self.process_html(token);
    }
    fn process_html(&mut self, token: Token) {
        if self.mode == Mode::InTableText {
            match token {
                Token::Character(s) => { self.pending.push((s, false)); return; }
                Token::Whitespace(s) => { self.pending.push((s, true)); return; }
                Token::NullCharacter(_) => return,
                _ => {
                    let pending = core::mem::take(&mut self.pending);
                    let nonwhite = pending.iter().any(|(_, white)| !white);
                    for (s, _) in pending { if nonwhite { self.table_fallback(Token::Character(s)); } else { self.text(&s); } }
                    self.mode = self.original;
                }
            }
        }
        match token {
            Token::StartTag(t) => self.start(t), Token::EndTag(t) => self.end(t),
            Token::Character(s) => self.characters(s, false, false),
            Token::Whitespace(s) => self.characters(s, true, false),
            Token::NullCharacter(s) => self.characters(s, false, true),
            Token::Comment(s) => {
                let parent = match self.mode {
                    Mode::Initial | Mode::BeforeHtml | Mode::AfterAfterBody | Mode::AfterAfterFrameset => self.doc.root,
                    Mode::AfterBody => self.stack.first().copied().unwrap_or(self.doc.root),
                    _ => self.target(self.current()),
                }; self.comment(s, parent);
            }
            Token::Doctype(t) => {
                if self.mode == Mode::Initial {
                    self.doc.mode = doctype_mode(&t);
                    let node = self.doc.add(NodeKind::Doctype { name: t.name.unwrap_or_default(), public_id: t.public_id.unwrap_or_default(), system_id: t.system_id.unwrap_or_default() });
                    self.doc.append(self.doc.root, node); self.mode = Mode::BeforeHtml;
                } else { self.error("misplaced-doctype"); }
            }
            Token::Eof => self.eof(),
        }
    }
    fn ensure_initial(&mut self) {
        self.error("missing-doctype"); self.doc.mode = DocumentMode::Quirks; self.mode = Mode::BeforeHtml;
    }
    fn characters(&mut self, s: String, white: bool, null: bool) {
        if null && self.mode != Mode::Text { return; }
        match self.mode {
            Mode::Initial => { if !white { self.ensure_initial(); self.characters(s, white, null); } }
            Mode::BeforeHtml => { if !white { self.fake("html"); self.mode = Mode::BeforeHead; self.characters(s, white, null); } }
            Mode::BeforeHead => { if !white { self.head = Some(self.fake("head")); self.mode = Mode::InHead; self.characters(s, white, null); } }
            Mode::InHead => { if white { self.text(&s); } else { self.pop(); self.mode = Mode::AfterHead; self.characters(s, white, null); } }
            Mode::AfterHead => { if white { self.text(&s); } else { self.fake("body"); self.mode = Mode::InBody; self.characters(s, white, null); } }
            Mode::InBody | Mode::InCaption | Mode::InCell | Mode::InTemplate => { self.reconstruct(); self.text(&s); if !white { self.frameset_ok = false; } }
            Mode::Text | Mode::InSelect | Mode::InSelectInTable => self.text(&s),
            Mode::InTable | Mode::InTableBody | Mode::InRow => {
                if table_structure(self.name()) {
                    self.original = self.mode; self.mode = Mode::InTableText; self.pending.push((s, white));
                } else { self.table_fallback(if white { Token::Whitespace(s) } else { Token::Character(s) }); }
            }
            Mode::InColumnGroup => { if white { self.text(&s); } else if self.name() == "colgroup" { self.pop(); self.mode = Mode::InTable; self.characters(s, white, null); } }
            Mode::AfterBody | Mode::AfterAfterBody => { if white { self.reconstruct(); self.text(&s); } else { self.mode = Mode::InBody; self.characters(s, white, null); } }
            Mode::InFrameset | Mode::AfterFrameset => { if white { self.text(&s); } }
            Mode::AfterAfterFrameset => { if white { self.reconstruct(); self.text(&s); } }
            Mode::InTableText => unreachable!(),
        }
    }

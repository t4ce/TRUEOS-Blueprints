//! Native translation of parse5's tree construction, open element stack and
//! active formatting element algorithms. See the vendored MIT-licensed p5/src.
use crate::dom::{Document, DocumentMode, Namespace, NodeId, NodeKind};
use crate::token::{Attribute, DoctypeToken, TagToken, Token};
use crate::tokenizer::{TextMode, Tokenizer};
use crate::{FragmentContext, ParseError};
use alloc::{
    string::{String, ToString},
    vec::Vec,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Initial,
    BeforeHtml,
    BeforeHead,
    InHead,
    AfterHead,
    InBody,
    Text,
    InTable,
    InTableText,
    InCaption,
    InColumnGroup,
    InTableBody,
    InRow,
    InCell,
    InSelect,
    InSelectInTable,
    InTemplate,
    AfterBody,
    InFrameset,
    AfterFrameset,
    AfterAfterBody,
    AfterAfterFrameset,
}
#[derive(Clone)]
struct Formatting {
    node: NodeId,
    token: TagToken,
}
#[derive(Clone, Copy)]
enum Scope {
    Normal,
    Button,
    List,
    Table,
    Select,
}

pub fn parse(html: &str) -> Document {
    Parser::new(html, None, false).run()
}
pub fn parse_fragment(html: &str, context: Option<FragmentContext>) -> Document {
    Parser::new(
        html,
        Some(context.unwrap_or_else(|| FragmentContext {
            name: "template".into(),
            namespace: Namespace::Html,
            attrs: Vec::new(),
        })),
        true,
    )
    .run()
}

struct Parser {
    doc: Document,
    tokenizer: Tokenizer,
    stack: Vec<NodeId>,
    formatting: Vec<Option<Formatting>>,
    templates: Vec<Mode>,
    mode: Mode,
    original: Mode,
    context: Option<NodeId>,
    fragment: bool,
    head: Option<NodeId>,
    form: Option<NodeId>,
    frameset_ok: bool,
    skip_newline: bool,
    foster: bool,
    pending: Vec<(String, bool)>,
    stopped: bool,
}
impl Parser {
    fn new(html: &str, context: Option<FragmentContext>, fragment: bool) -> Self {
        let mut p = Self {
            doc: Document::new(fragment),
            tokenizer: Tokenizer::new(html),
            stack: Vec::new(),
            formatting: Vec::new(),
            templates: Vec::new(),
            mode: Mode::Initial,
            original: Mode::Initial,
            context: None,
            fragment,
            head: None,
            form: None,
            frameset_ok: true,
            skip_newline: false,
            foster: false,
            pending: Vec::new(),
            stopped: false,
        };
        if let Some(ctx) = context {
            let name = ctx.name.clone();
            let ns = ctx.namespace;
            let id = p.doc.add(NodeKind::Element {
                name: ctx.name,
                namespace: ns,
                attrs: ctx.attrs,
                template_content: None,
            });
            p.context = Some(id);
            if ns == Namespace::Html {
                let text_mode = match name.as_str() {
                    "title" | "textarea" => TextMode::Rcdata,
                    "style" | "xmp" | "iframe" | "noembed" | "noframes" | "noscript" => {
                        TextMode::Rawtext
                    }
                    "script" => TextMode::ScriptData,
                    "plaintext" => TextMode::Plaintext,
                    _ => TextMode::Data,
                };
                // Fragment context selects the state but is not a seen start tag.
                p.tokenizer.set_mode(text_mode, "");
                if name == "template" {
                    p.templates.push(Mode::InTemplate);
                }
                if name == "form" {
                    p.form = Some(id);
                }
            }
            p.fake("html");
            p.reset_mode();
        }
        p
    }
    fn run(mut self) -> Document {
        while !self.stopped {
            let current = self.adjusted_current();
            let foreign =
                self.doc.namespace(current) != Namespace::Html && !self.integration(current, false);
            self.tokenizer.set_in_foreign_node(foreign);
            let token = self.tokenizer.next_token();
            self.process(token);
        }
        self.doc.errors.append(&mut self.tokenizer.errors);
        if self.fragment {
            if let Some(html) = self
                .stack
                .first()
                .copied()
                .or_else(|| self.doc.nodes[self.doc.root].children.first().copied())
            {
                let children = self.doc.nodes[html].children.clone();
                for child in children {
                    self.doc.append(self.doc.root, child);
                }
                self.doc.detach(html);
            }
        }
        self.doc
    }
    fn error(&mut self, code: &str) {
        self.doc.errors.push(ParseError {
            code: code.into(),
            offset: 0,
        });
    }
    fn current(&self) -> NodeId {
        self.stack.last().copied().unwrap_or(self.doc.root)
    }
    fn adjusted_current(&self) -> NodeId {
        if self.stack.len() == 1 {
            self.context.unwrap_or(self.current())
        } else {
            self.current()
        }
    }
    fn name(&self) -> &str {
        self.doc.name(self.current())
    }
    fn is_html(&self, id: NodeId, name: &str) -> bool {
        self.doc.namespace(id) == Namespace::Html && self.doc.name(id) == name
    }
    fn in_template(&self) -> bool {
        self.stack.iter().any(|&id| self.is_html(id, "template"))
    }
    fn target(&self, node: NodeId) -> NodeId {
        if let NodeKind::Element {
            template_content: Some(content),
            ..
        } = &self.doc.nodes[node].kind
        {
            *content
        } else {
            node
        }
    }
    fn foster_location(&self) -> (NodeId, Option<NodeId>) {
        for (i, &id) in self.stack.iter().enumerate().rev() {
            if self.is_html(id, "template") {
                return (self.target(id), None);
            }
            if self.is_html(id, "table") {
                if let Some(parent) = self.doc.nodes[id].parent {
                    return (parent, Some(id));
                }
                return (
                    self.stack
                        .get(i.wrapping_sub(1))
                        .copied()
                        .unwrap_or(self.doc.root),
                    None,
                );
            }
        }
        (self.stack.first().copied().unwrap_or(self.doc.root), None)
    }
    fn location(&self) -> (NodeId, Option<NodeId>) {
        if self.foster && table_structure(self.name()) {
            self.foster_location()
        } else {
            (self.target(self.current()), None)
        }
    }
    fn attach(&mut self, node: NodeId) {
        let (parent, before) = self.location();
        if let Some(before) = before {
            self.doc.insert_before(parent, node, before);
        } else {
            self.doc.append(parent, node);
        }
    }
    fn create(&mut self, token: &TagToken, namespace: Namespace) -> NodeId {
        let content = if namespace == Namespace::Html && token.name == "template" {
            Some(self.doc.add(NodeKind::Fragment))
        } else {
            None
        };
        self.doc.add(NodeKind::Element {
            name: token.name.clone(),
            namespace,
            attrs: token.attrs.clone(),
            template_content: content,
        })
    }
    fn insert(&mut self, token: &TagToken, ns: Namespace, push: bool) -> NodeId {
        let node = self.create(token, ns);
        self.attach(node);
        if push {
            self.stack.push(node);
        }
        node
    }
    fn fake(&mut self, name: &str) -> NodeId {
        self.insert(&tag(name), Namespace::Html, true)
    }
    fn text(&mut self, chars: &str) {
        let (parent, before) = self.location();
        self.doc.append_text(parent, chars, before);
    }
    fn comment(&mut self, chars: String, parent: NodeId) {
        let node = self.doc.add(NodeKind::Comment(chars));
        self.doc.append(parent, node);
    }
    fn pop(&mut self) {
        self.stack.pop();
    }
    fn pop_name(&mut self, name: &str) {
        if let Some(pos) = self.stack.iter().rposition(|&id| self.is_html(id, name)) {
            self.stack.truncate(pos);
        }
    }
    fn remove_stack(&mut self, node: NodeId) {
        if let Some(pos) = self.stack.iter().position(|&id| id == node) {
            self.stack.remove(pos);
        }
    }
    fn scope(&self, name: &str, scope: Scope) -> bool {
        for &id in self.stack.iter().rev() {
            let ns = self.doc.namespace(id);
            let n = self.doc.name(id);
            if ns == Namespace::Html {
                if n == name {
                    return true;
                }
                let stop = match scope {
                    Scope::Table => matches!(n, "table" | "html"),
                    Scope::Select => !matches!(n, "option" | "optgroup"),
                    _ => {
                        scope_html(n)
                            || matches!(scope, Scope::Button) && n == "button"
                            || matches!(scope, Scope::List) && matches!(n, "ol" | "ul")
                    }
                };
                if stop {
                    return false;
                }
            } else if !matches!(scope, Scope::Table | Scope::Select) && special_foreign(ns, n) {
                return false;
            }
        }
        true
    }
    fn implied(&mut self, exclusion: Option<&str>, thorough: bool) {
        loop {
            let name = self.name();
            if exclusion == Some(name) || !implied_tag(name, thorough || exclusion.is_some()) {
                break;
            }
            self.pop();
        }
    }
    fn close_p(&mut self) {
        self.implied(Some("p"), false);
        self.pop_name("p");
    }
    fn clear_back(&mut self, names: &[&str]) {
        if let Some(pos) = self.stack.iter().rposition(|&id| {
            self.doc.namespace(id) == Namespace::Html && names.contains(&self.doc.name(id))
        }) {
            self.stack.truncate(pos + 1);
        }
    }
    fn clear_marker(&mut self) {
        while let Some(entry) = self.formatting.pop() {
            if entry.is_none() {
                break;
            }
        }
    }
    fn active(&self, name: &str) -> Option<usize> {
        for (i, entry) in self.formatting.iter().enumerate().rev() {
            match entry {
                Some(e) if self.doc.name(e.node) == name => return Some(i),
                None => break,
                _ => (),
            }
        }
        None
    }
    fn push_formatting(&mut self, node: NodeId, token: &TagToken) {
        let mut equal = Vec::new();
        for (i, e) in self.formatting.iter().enumerate().rev() {
            match e {
                None => break,
                Some(e)
                    if e.token.name == token.name
                        && self.doc.namespace(e.node) == self.doc.namespace(node)
                        && attrs_equal(&e.token.attrs, &token.attrs) =>
                {
                    equal.push(i)
                }
                _ => (),
            }
        }
        if equal.len() >= 3 {
            self.formatting.remove(*equal.last().unwrap());
        }
        self.formatting.push(Some(Formatting {
            node,
            token: token.clone(),
        }));
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
            for attr in attrs {
                if !target.iter().any(|a| a.name == attr.name) {
                    target.push(attr.clone());
                }
            }
        }
    }
    fn special(&self, node: NodeId) -> bool {
        let n = self.doc.name(node);
        let ns = self.doc.namespace(node);
        if ns != Namespace::Html {
            special_foreign(ns, n)
        } else {
            special_html(n)
        }
    }
    fn generic_end(&mut self, token: &TagToken) {
        for i in (1..self.stack.len()).rev() {
            let id = self.stack[i];
            if self.is_html(id, &token.name) {
                self.implied(Some(&token.name), false);
                if self.stack.len() > i {
                    self.stack.truncate(i);
                }
                break;
            }
            if self.special(id) {
                break;
            }
        }
    }
    fn adoption(&mut self, token: &TagToken) {
        for _ in 0..8 {
            let Some(fi) = self.active(&token.name) else {
                self.generic_end(token);
                break;
            };
            let entry = self.formatting[fi].clone().unwrap();
            let Some(si) = self.stack.iter().position(|&n| n == entry.node) else {
                self.formatting.remove(fi);
                break;
            };
            if !self.scope(&token.name, Scope::Normal) {
                break;
            }
            let Some(bi) = (si + 1..self.stack.len()).find(|&i| self.special(self.stack[i])) else {
                self.stack.truncate(si);
                self.formatting.remove(fi);
                break;
            };
            let furthest = self.stack[bi];
            let common = if si > 0 {
                self.stack[si - 1]
            } else {
                self.doc.root
            };
            let mut last = furthest;
            let mut bookmark = fi;
            let mut index = bi;
            let mut inner = 0;
            while index > si + 1 {
                index -= 1;
                inner += 1;
                let node = self.stack[index];
                let mut ai = self
                    .formatting
                    .iter()
                    .position(|e| e.as_ref().is_some_and(|e| e.node == node));
                if inner > 3 {
                    if let Some(ai_value) = ai {
                        self.formatting.remove(ai_value);
                        if ai_value < bookmark {
                            bookmark -= 1;
                        }
                        ai = None;
                    }
                }
                let Some(ai) = ai else {
                    self.stack.remove(index);
                    continue;
                };
                let active = self.formatting[ai].clone().unwrap();
                let fresh = self.create(&active.token, self.doc.namespace(active.node));
                self.stack[index] = fresh;
                self.formatting[ai] = Some(Formatting {
                    node: fresh,
                    ..active
                });
                if last == furthest {
                    bookmark = ai + 1;
                }
                self.doc.append(fresh, last);
                last = fresh;
            }
            self.doc.detach(last);
            if table_structure(self.doc.name(common)) {
                let (parent, before) = self.foster_location();
                if let Some(before) = before {
                    self.doc.insert_before(parent, last, before);
                } else {
                    self.doc.append(parent, last);
                }
            } else {
                self.doc.append(self.target(common), last);
            }
            let fresh = self.create(&entry.token, self.doc.namespace(entry.node));
            let children = self.doc.nodes[furthest].children.clone();
            for child in children {
                self.doc.append(fresh, child);
            }
            self.doc.append(furthest, fresh);
            if let Some(pos) = self
                .formatting
                .iter()
                .position(|e| e.as_ref().is_some_and(|e| e.node == entry.node))
            {
                self.formatting.remove(pos);
                if pos < bookmark {
                    bookmark -= 1;
                }
            }
            self.formatting.insert(
                bookmark.min(self.formatting.len()),
                Some(Formatting {
                    node: fresh,
                    token: entry.token,
                }),
            );
            self.remove_stack(entry.node);
            if let Some(pos) = self.stack.iter().position(|&n| n == furthest) {
                self.stack.insert(pos + 1, fresh);
            }
        }
    }
    fn integration(&self, node: NodeId, html_only: bool) -> bool {
        let name = self.doc.name(node);
        let ns = self.doc.namespace(node);
        if ns == Namespace::Svg && matches!(name, "foreignObject" | "desc" | "title") {
            return true;
        }
        if ns == Namespace::MathMl {
            if !html_only && matches!(name, "mi" | "mo" | "mn" | "ms" | "mtext") {
                return true;
            }
            if name == "annotation-xml" {
                if let Some((_, _, attrs)) = self.doc.element(node) {
                    return attrs.iter().any(|a| {
                        a.name == "encoding"
                            && (a.value.eq_ignore_ascii_case("text/html")
                                || a.value.eq_ignore_ascii_case("application/xhtml+xml"))
                    });
                }
            }
        }
        false
    }
    fn foreign(&mut self, token: Token) -> Option<Token> {
        let current = self.adjusted_current();
        let ns = self.doc.namespace(current);
        if ns == Namespace::Html {
            return Some(token);
        }
        match token {
            Token::StartTag(mut t) => {
                if t.name == "svg"
                    && ns == Namespace::MathMl
                    && self.doc.name(current) == "annotation-xml"
                {
                    return Some(Token::StartTag(t));
                }
                if self.integration(current, false)
                    && (!matches!(t.name.as_str(), "mglyph" | "malignmark")
                        || self.integration(current, true))
                {
                    return Some(Token::StartTag(t));
                }
                if foreign_exit(&t) {
                    while self.doc.namespace(self.current()) != Namespace::Html
                        && !self.integration(self.current(), false)
                    {
                        self.pop();
                        if self.stack.is_empty() {
                            break;
                        }
                    }
                    return Some(Token::StartTag(t));
                }
                adjust_foreign(&mut t, ns, true);
                self.insert(&t, ns, !t.self_closing);
                None
            }
            Token::EndTag(t) => {
                if matches!(t.name.as_str(), "p" | "br") {
                    while self.doc.namespace(self.current()) != Namespace::Html
                        && !self.integration(self.current(), false)
                    {
                        self.pop();
                        if self.stack.is_empty() {
                            break;
                        }
                    }
                    return Some(Token::EndTag(t));
                }
                for i in (1..self.stack.len()).rev() {
                    let id = self.stack[i];
                    if self.doc.namespace(id) == Namespace::Html {
                        return Some(Token::EndTag(t));
                    }
                    if self.doc.name(id).eq_ignore_ascii_case(&t.name) {
                        self.stack.truncate(i);
                        break;
                    }
                }
                None
            }
            Token::Character(s) if !self.integration(current, false) => {
                self.text(&s);
                self.frameset_ok = false;
                None
            }
            Token::Whitespace(s) if !self.integration(current, false) => {
                self.text(&s);
                None
            }
            Token::NullCharacter(_) if !self.integration(current, false) => {
                self.text("\u{fffd}");
                None
            }
            Token::Comment(s) => {
                self.comment(s, self.target(self.current()));
                None
            }
            Token::Doctype(_) => None,
            token => Some(token),
        }
    }
    fn reset_mode(&mut self) {
        for i in (0..self.stack.len()).rev() {
            let node = if i == 0 {
                self.context.unwrap_or(self.stack[i])
            } else {
                self.stack[i]
            };
            self.mode = match self.doc.name(node) {
                "tr" => Mode::InRow,
                "tbody" | "thead" | "tfoot" => Mode::InTableBody,
                "caption" => Mode::InCaption,
                "colgroup" => Mode::InColumnGroup,
                "table" => Mode::InTable,
                "body" => Mode::InBody,
                "frameset" => Mode::InFrameset,
                "select" => {
                    let mut mode = Mode::InSelect;
                    for &node in self.stack[..i].iter().rev() {
                        if self.doc.name(node) == "template" {
                            break;
                        }
                        if self.doc.name(node) == "table" {
                            mode = Mode::InSelectInTable;
                            break;
                        }
                    }
                    mode
                }
                "template" => self.templates.last().copied().unwrap_or(Mode::InTemplate),
                "html" => {
                    if self.head.is_some() {
                        Mode::AfterHead
                    } else {
                        Mode::BeforeHead
                    }
                }
                "td" | "th" if i > 0 => Mode::InCell,
                "head" if i > 0 => Mode::InHead,
                _ => continue,
            };
            return;
        }
        self.mode = Mode::InBody;
    }
    fn switch_text(&mut self, tag: &TagToken, mode: TextMode) {
        self.insert(tag, Namespace::Html, true);
        self.tokenizer.set_mode(mode, &tag.name);
        self.original = self.mode;
        self.mode = Mode::Text;
    }
    fn close_template(&mut self) {
        if !self.in_template() {
            self.error("end-tag-without-matching-open-element");
            return;
        }
        self.implied(None, true);
        self.pop_name("template");
        self.clear_marker();
        self.templates.pop();
        self.reset_mode();
    }
    fn close_cell(&mut self) {
        self.implied(None, false);
        if let Some(pos) = self.stack.iter().rposition(|&id| {
            self.doc.namespace(id) == Namespace::Html && matches!(self.doc.name(id), "td" | "th")
        }) {
            self.stack.truncate(pos);
        }
        self.clear_marker();
        self.mode = Mode::InRow;
    }
    fn process(&mut self, mut token: Token) {
        if self.skip_newline {
            self.skip_newline = false;
            if let Token::Whitespace(s) = &mut token {
                if s.starts_with('\n') {
                    s.remove(0);
                    if s.is_empty() {
                        return;
                    }
                }
            }
        }
        let Some(token) = self.foreign(token) else {
            return;
        };
        self.process_html(token);
    }
    fn process_html(&mut self, token: Token) {
        if self.mode == Mode::InTableText {
            match token {
                Token::Character(s) => {
                    self.pending.push((s, false));
                    return;
                }
                Token::Whitespace(s) => {
                    self.pending.push((s, true));
                    return;
                }
                Token::NullCharacter(_) => return,
                _ => {
                    let pending = core::mem::take(&mut self.pending);
                    let nonwhite = pending.iter().any(|(_, white)| !white);
                    for (s, _) in pending {
                        if nonwhite {
                            self.table_fallback(Token::Character(s));
                        } else {
                            self.text(&s);
                        }
                    }
                    self.mode = self.original;
                }
            }
        }
        match token {
            Token::StartTag(t) => self.start(t),
            Token::EndTag(t) => self.end(t),
            Token::Character(s) => self.characters(s, false, false),
            Token::Whitespace(s) => self.characters(s, true, false),
            Token::NullCharacter(s) => self.characters(s, false, true),
            Token::Comment(s) => {
                let parent = match self.mode {
                    Mode::Initial
                    | Mode::BeforeHtml
                    | Mode::AfterAfterBody
                    | Mode::AfterAfterFrameset => self.doc.root,
                    Mode::AfterBody => self.stack.first().copied().unwrap_or(self.doc.root),
                    _ => self.target(self.current()),
                };
                self.comment(s, parent);
            }
            Token::Doctype(t) => {
                if self.mode == Mode::Initial {
                    self.doc.mode = doctype_mode(&t);
                    let node = self.doc.add(NodeKind::Doctype {
                        name: t.name.unwrap_or_default(),
                        public_id: t.public_id.unwrap_or_default(),
                        system_id: t.system_id.unwrap_or_default(),
                    });
                    self.doc.append(self.doc.root, node);
                    self.mode = Mode::BeforeHtml;
                } else {
                    self.error("misplaced-doctype");
                }
            }
            Token::Eof => self.eof(),
        }
    }
    fn ensure_initial(&mut self) {
        self.error("missing-doctype");
        self.doc.mode = DocumentMode::Quirks;
        self.mode = Mode::BeforeHtml;
    }
    fn characters(&mut self, s: String, white: bool, null: bool) {
        if null
            && matches!(
                self.mode,
                Mode::InBody
                    | Mode::InCaption
                    | Mode::InCell
                    | Mode::InTemplate
                    | Mode::InSelect
                    | Mode::InSelectInTable
                    | Mode::InFrameset
                    | Mode::AfterFrameset
                    | Mode::AfterAfterFrameset
            )
        {
            return;
        }
        match self.mode {
            Mode::Initial => {
                if !white {
                    self.ensure_initial();
                    self.characters(s, white, null);
                }
            }
            Mode::BeforeHtml => {
                if !white {
                    self.fake("html");
                    self.mode = Mode::BeforeHead;
                    self.characters(s, white, null);
                }
            }
            Mode::BeforeHead => {
                if !white {
                    self.head = Some(self.fake("head"));
                    self.mode = Mode::InHead;
                    self.characters(s, white, null);
                }
            }
            Mode::InHead => {
                if white {
                    self.text(&s);
                } else {
                    self.pop();
                    self.mode = Mode::AfterHead;
                    self.characters(s, white, null);
                }
            }
            Mode::AfterHead => {
                if white {
                    self.text(&s);
                } else {
                    self.fake("body");
                    self.mode = Mode::InBody;
                    self.characters(s, white, null);
                }
            }
            Mode::InBody | Mode::InCaption | Mode::InCell | Mode::InTemplate => {
                self.reconstruct();
                self.text(&s);
                if !white {
                    self.frameset_ok = false;
                }
            }
            Mode::Text | Mode::InSelect | Mode::InSelectInTable => self.text(&s),
            Mode::InTable | Mode::InTableBody | Mode::InRow => {
                if table_structure(self.name()) {
                    self.original = self.mode;
                    self.mode = Mode::InTableText;
                    if !null {
                        self.pending.push((s, white));
                    }
                } else {
                    self.table_fallback(if white {
                        Token::Whitespace(s)
                    } else {
                        Token::Character(s)
                    });
                }
            }
            Mode::InColumnGroup => {
                if white {
                    self.text(&s);
                } else if self.name() == "colgroup" {
                    self.pop();
                    self.mode = Mode::InTable;
                    self.characters(s, white, null);
                }
            }
            Mode::AfterBody | Mode::AfterAfterBody => {
                if white {
                    self.reconstruct();
                    self.text(&s);
                } else {
                    self.mode = Mode::InBody;
                    self.characters(s, white, null);
                }
            }
            Mode::InFrameset | Mode::AfterFrameset => {
                if white {
                    self.text(&s);
                }
            }
            Mode::AfterAfterFrameset => {
                if white {
                    self.reconstruct();
                    self.text(&s);
                }
            }
            Mode::InTableText => unreachable!(),
        }
    }
    fn start(&mut self, t: TagToken) {
        match self.mode {
            Mode::Initial => {
                self.ensure_initial();
                self.start(t);
            }
            Mode::BeforeHtml => {
                if t.name == "html" {
                    self.insert(&t, Namespace::Html, true);
                    self.mode = Mode::BeforeHead;
                } else {
                    self.fake("html");
                    self.mode = Mode::BeforeHead;
                    self.start(t);
                }
            }
            Mode::BeforeHead => match t.name.as_str() {
                "html" => self.start_body(t),
                "head" => {
                    self.head = Some(self.insert(&t, Namespace::Html, true));
                    self.mode = Mode::InHead;
                }
                _ => {
                    self.head = Some(self.fake("head"));
                    self.mode = Mode::InHead;
                    self.start(t);
                }
            },
            Mode::InHead => self.start_head(t),
            Mode::AfterHead => match t.name.as_str() {
                "html" => self.start_body(t),
                "body" => {
                    self.insert(&t, Namespace::Html, true);
                    self.frameset_ok = false;
                    self.mode = Mode::InBody;
                }
                "frameset" => {
                    self.insert(&t, Namespace::Html, true);
                    self.mode = Mode::InFrameset;
                }
                "base" | "basefont" | "bgsound" | "link" | "meta" | "noframes" | "script"
                | "style" | "template" | "title" => {
                    if let Some(head) = self.head {
                        self.stack.push(head);
                        self.start_head(t);
                        self.remove_stack(head);
                    }
                }
                "head" => (),
                _ => {
                    self.fake("body");
                    self.mode = Mode::InBody;
                    self.start_body(t);
                }
            },
            Mode::InBody => self.start_body(t),
            Mode::Text => (),
            Mode::InTable => self.start_table(t),
            Mode::InCaption => {
                if table_void(&t.name) {
                    if self.scope("caption", Scope::Table) {
                        self.implied(None, false);
                        self.pop_name("caption");
                        self.clear_marker();
                        self.mode = Mode::InTable;
                        self.start_table(t);
                    }
                } else {
                    self.start_body(t);
                }
            }
            Mode::InColumnGroup => match t.name.as_str() {
                "html" => self.start_body(t),
                "col" => {
                    self.insert(&t, Namespace::Html, false);
                }
                "template" => self.start_head(t),
                _ => {
                    if self.name() == "colgroup" {
                        self.pop();
                        self.mode = Mode::InTable;
                        self.start(t);
                    }
                }
            },
            Mode::InTableBody => match t.name.as_str() {
                "tr" => {
                    self.clear_back(&["tbody", "tfoot", "thead", "template", "html"]);
                    self.insert(&t, Namespace::Html, true);
                    self.mode = Mode::InRow;
                }
                "td" | "th" => {
                    self.clear_back(&["tbody", "tfoot", "thead", "template", "html"]);
                    self.fake("tr");
                    self.mode = Mode::InRow;
                    self.start(t);
                }
                "caption" | "col" | "colgroup" | "tbody" | "tfoot" | "thead" => {
                    if self.body_table_scope() {
                        self.clear_back(&["tbody", "tfoot", "thead", "template", "html"]);
                        self.pop();
                        self.mode = Mode::InTable;
                        self.start(t);
                    }
                }
                _ => self.start_table(t),
            },
            Mode::InRow => match t.name.as_str() {
                "td" | "th" => {
                    self.clear_back(&["tr", "template", "html"]);
                    self.insert(&t, Namespace::Html, true);
                    self.mode = Mode::InCell;
                    self.formatting.push(None);
                }
                "caption" | "col" | "colgroup" | "tbody" | "tfoot" | "thead" | "tr" => {
                    if self.scope("tr", Scope::Table) {
                        self.clear_back(&["tr", "template", "html"]);
                        self.pop();
                        self.mode = Mode::InTableBody;
                        self.start(t);
                    }
                }
                _ => self.start_table(t),
            },
            Mode::InCell => {
                if table_void(&t.name) {
                    if self.scope("td", Scope::Table) || self.scope("th", Scope::Table) {
                        self.close_cell();
                        self.start(t);
                    }
                } else {
                    self.start_body(t);
                }
            }
            Mode::InSelectInTable if select_table_tag(&t.name) => {
                self.pop_name("select");
                self.reset_mode();
                self.start(t);
            }
            Mode::InSelect | Mode::InSelectInTable => self.start_select(t),
            Mode::InTemplate => {
                self.mode = match t.name.as_str() {
                    "base" | "basefont" | "bgsound" | "link" | "meta" | "noframes" | "script"
                    | "style" | "template" | "title" => {
                        self.start_head(t);
                        return;
                    }
                    "caption" | "colgroup" | "tbody" | "tfoot" | "thead" => Mode::InTable,
                    "col" => Mode::InColumnGroup,
                    "tr" => Mode::InTableBody,
                    "td" | "th" => Mode::InRow,
                    _ => Mode::InBody,
                };
                if let Some(mode) = self.templates.last_mut() {
                    *mode = self.mode;
                }
                self.start(t);
            }
            Mode::AfterBody | Mode::AfterAfterBody => {
                if t.name == "html" {
                    self.start_body(t);
                } else {
                    self.mode = Mode::InBody;
                    self.start_body(t);
                }
            }
            Mode::InFrameset => match t.name.as_str() {
                "html" => self.start_body(t),
                "frameset" => {
                    self.insert(&t, Namespace::Html, true);
                }
                "frame" => {
                    self.insert(&t, Namespace::Html, false);
                }
                "noframes" => self.start_head(t),
                _ => (),
            },
            Mode::AfterFrameset | Mode::AfterAfterFrameset => match t.name.as_str() {
                "html" => self.start_body(t),
                "noframes" => self.start_head(t),
                _ => (),
            },
            Mode::InTableText => unreachable!(),
        }
    }
    fn start_head(&mut self, t: TagToken) {
        match t.name.as_str() {
            "html" => self.start_body(t),
            "base" | "basefont" | "bgsound" | "link" | "meta" => {
                self.insert(&t, Namespace::Html, false);
            }
            "title" => self.switch_text(&t, TextMode::Rcdata),
            "noscript" | "noframes" | "style" => self.switch_text(&t, TextMode::Rawtext),
            "script" => self.switch_text(&t, TextMode::ScriptData),
            "template" => {
                self.insert(&t, Namespace::Html, true);
                self.formatting.push(None);
                self.frameset_ok = false;
                self.mode = Mode::InTemplate;
                self.templates.push(Mode::InTemplate);
            }
            "head" => self.error("misplaced-start-tag-for-head-element"),
            _ => {
                self.pop();
                self.mode = Mode::AfterHead;
                self.start(t);
            }
        }
    }
    fn start_body(&mut self, mut t: TagToken) {
        let name = t.name.clone();
        match name.as_str() {
            "html" => {
                if !self.in_template() {
                    if let Some(root) = self.stack.first().copied() {
                        self.adopt_attrs(root, &t.attrs);
                    }
                }
            }
            "body" => {
                if !self.in_template() {
                    if let Some(body) = self
                        .stack
                        .get(1)
                        .copied()
                        .filter(|&id| self.is_html(id, "body"))
                    {
                        self.frameset_ok = false;
                        self.adopt_attrs(body, &t.attrs);
                    }
                }
            }
            "frameset" => {
                if self.frameset_ok {
                    if let Some(body) = self
                        .stack
                        .get(1)
                        .copied()
                        .filter(|&id| self.is_html(id, "body"))
                    {
                        self.doc.detach(body);
                        self.stack.truncate(1);
                        self.insert(&t, Namespace::Html, true);
                        self.mode = Mode::InFrameset;
                    }
                }
            }
            "base" | "basefont" | "bgsound" | "link" | "meta" | "style" | "title" | "script"
            | "template" => self.start_head(t),
            "p" | "dl" | "ol" | "ul" | "div" | "dir" | "nav" | "main" | "menu" | "aside"
            | "center" | "figure" | "footer" | "header" | "hgroup" | "dialog" | "details"
            | "address" | "article" | "search" | "section" | "summary" | "fieldset"
            | "blockquote" | "figcaption" => {
                if self.scope("p", Scope::Button) {
                    self.close_p();
                }
                self.insert(&t, Namespace::Html, true);
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                if self.scope("p", Scope::Button) {
                    self.close_p();
                }
                if header(self.name()) {
                    self.pop();
                }
                self.insert(&t, Namespace::Html, true);
            }
            "pre" | "listing" => {
                if self.scope("p", Scope::Button) {
                    self.close_p();
                }
                self.insert(&t, Namespace::Html, true);
                self.skip_newline = true;
                self.frameset_ok = false;
            }
            "form" => {
                if self.form.is_none() || self.in_template() {
                    if self.scope("p", Scope::Button) {
                        self.close_p();
                    }
                    let node = self.insert(&t, Namespace::Html, true);
                    if !self.in_template() {
                        self.form = Some(node);
                    }
                }
            }
            "li" | "dd" | "dt" => {
                self.frameset_ok = false;
                for i in (0..self.stack.len()).rev() {
                    let node = self.stack[i];
                    let n = self.doc.name(node).to_string();
                    if (name == "li" && n == "li")
                        || (matches!(name.as_str(), "dd" | "dt")
                            && matches!(n.as_str(), "dd" | "dt"))
                    {
                        self.implied(Some(&n), false);
                        self.pop_name(&n);
                        break;
                    }
                    if !matches!(n.as_str(), "address" | "div" | "p") && self.special(node) {
                        break;
                    }
                }
                if self.scope("p", Scope::Button) {
                    self.close_p();
                }
                self.insert(&t, Namespace::Html, true);
            }
            "plaintext" => {
                if self.scope("p", Scope::Button) {
                    self.close_p();
                }
                self.insert(&t, Namespace::Html, true);
                self.tokenizer.set_mode(TextMode::Plaintext, "plaintext");
            }
            "button" => {
                if self.scope("button", Scope::Normal) {
                    self.implied(None, false);
                    self.pop_name("button");
                }
                self.reconstruct();
                self.insert(&t, Namespace::Html, true);
                self.frameset_ok = false;
            }
            "a" => {
                if let Some(pos) = self.active("a") {
                    let old = self.formatting[pos].as_ref().unwrap().node;
                    self.adoption(&t);
                    self.remove_stack(old);
                    if let Some(pos) = self
                        .formatting
                        .iter()
                        .position(|e| e.as_ref().is_some_and(|e| e.node == old))
                    {
                        self.formatting.remove(pos);
                    }
                }
                self.reconstruct();
                let node = self.insert(&t, Namespace::Html, true);
                self.push_formatting(node, &t);
            }
            "b" | "i" | "s" | "u" | "em" | "tt" | "big" | "code" | "font" | "small" | "strike"
            | "strong" | "nobr" => {
                self.reconstruct();
                if name == "nobr" && self.scope("nobr", Scope::Normal) {
                    self.adoption(&t);
                    self.reconstruct();
                }
                let node = self.insert(&t, Namespace::Html, true);
                self.push_formatting(node, &t);
            }
            "applet" | "object" | "marquee" => {
                self.reconstruct();
                self.insert(&t, Namespace::Html, true);
                self.formatting.push(None);
                self.frameset_ok = false;
            }
            "table" => {
                if self.doc.mode != DocumentMode::Quirks && self.scope("p", Scope::Button) {
                    self.close_p();
                }
                self.insert(&t, Namespace::Html, true);
                self.frameset_ok = false;
                self.mode = Mode::InTable;
            }
            "image" => {
                t.name = "img".into();
                self.start_body(t);
            }
            "area" | "br" | "embed" | "img" | "keygen" | "wbr" => {
                self.reconstruct();
                self.insert(&t, Namespace::Html, false);
                self.frameset_ok = false;
            }
            "input" => {
                self.reconstruct();
                self.insert(&t, Namespace::Html, false);
                if !hidden(&t) {
                    self.frameset_ok = false;
                }
            }
            "param" | "source" | "track" => {
                self.insert(&t, Namespace::Html, false);
            }
            "hr" => {
                if self.scope("p", Scope::Button) {
                    self.close_p();
                }
                self.insert(&t, Namespace::Html, false);
                self.frameset_ok = false;
            }
            "textarea" => {
                self.switch_text(&t, TextMode::Rcdata);
                self.skip_newline = true;
                self.frameset_ok = false;
            }
            "xmp" => {
                if self.scope("p", Scope::Button) {
                    self.close_p();
                }
                self.reconstruct();
                self.frameset_ok = false;
                self.switch_text(&t, TextMode::Rawtext);
            }
            "iframe" => {
                self.frameset_ok = false;
                self.switch_text(&t, TextMode::Rawtext);
            }
            "noembed" | "noframes" | "noscript" => self.switch_text(&t, TextMode::Rawtext),
            "select" => {
                self.reconstruct();
                self.insert(&t, Namespace::Html, true);
                self.frameset_ok = false;
                self.mode = if matches!(
                    self.mode,
                    Mode::InTable
                        | Mode::InCaption
                        | Mode::InTableBody
                        | Mode::InRow
                        | Mode::InCell
                ) {
                    Mode::InSelectInTable
                } else {
                    Mode::InSelect
                };
            }
            "option" | "optgroup" => {
                if self.name() == "option" {
                    self.pop();
                }
                self.reconstruct();
                self.insert(&t, Namespace::Html, true);
            }
            "rb" | "rtc" => {
                if self.scope("ruby", Scope::Normal) {
                    self.implied(None, false);
                }
                self.insert(&t, Namespace::Html, true);
            }
            "rt" | "rp" => {
                if self.scope("ruby", Scope::Normal) {
                    self.implied(Some("rtc"), false);
                }
                self.insert(&t, Namespace::Html, true);
            }
            "math" | "svg" => {
                self.reconstruct();
                let ns = if name == "svg" {
                    Namespace::Svg
                } else {
                    Namespace::MathMl
                };
                adjust_foreign(&mut t, ns, false);
                self.insert(&t, ns, !t.self_closing);
            }
            "col" | "th" | "td" | "tr" | "head" | "frame" | "tbody" | "tfoot" | "thead"
            | "caption" | "colgroup" => (),
            _ => {
                self.reconstruct();
                self.insert(&t, Namespace::Html, true);
            }
        }
    }
    fn table_fallback(&mut self, token: Token) {
        let foster = self.foster;
        self.foster = true;
        match token {
            Token::StartTag(t) => self.start_body(t),
            Token::EndTag(t) => self.end_body(t),
            Token::Character(s) => {
                self.reconstruct();
                self.text(&s);
                self.frameset_ok = false;
            }
            Token::Whitespace(s) => {
                self.reconstruct();
                self.text(&s);
            }
            Token::Eof => self.eof(),
            _ => (),
        }
        self.foster = foster;
    }
    fn start_table(&mut self, t: TagToken) {
        match t.name.as_str() {
            "caption" => {
                self.clear_back(&["table", "template", "html"]);
                self.formatting.push(None);
                self.insert(&t, Namespace::Html, true);
                self.mode = Mode::InCaption;
            }
            "colgroup" => {
                self.clear_back(&["table", "template", "html"]);
                self.insert(&t, Namespace::Html, true);
                self.mode = Mode::InColumnGroup;
            }
            "col" => {
                self.clear_back(&["table", "template", "html"]);
                self.fake("colgroup");
                self.mode = Mode::InColumnGroup;
                self.start(t);
            }
            "tbody" | "tfoot" | "thead" => {
                self.clear_back(&["table", "template", "html"]);
                self.insert(&t, Namespace::Html, true);
                self.mode = Mode::InTableBody;
            }
            "td" | "th" | "tr" => {
                self.clear_back(&["table", "template", "html"]);
                self.fake("tbody");
                self.mode = Mode::InTableBody;
                self.start(t);
            }
            "table" => {
                if self.scope("table", Scope::Table) {
                    self.pop_name("table");
                    self.reset_mode();
                    self.start(t);
                }
            }
            "style" | "script" | "template" => self.start_head(t),
            "input" if hidden(&t) => {
                self.insert(&t, Namespace::Html, false);
            }
            "form" => {
                if self.form.is_none() && !self.in_template() {
                    self.form = Some(self.insert(&t, Namespace::Html, true));
                    self.pop();
                }
            }
            _ => self.table_fallback(Token::StartTag(t)),
        }
    }
    fn body_table_scope(&self) -> bool {
        self.scope("tbody", Scope::Table)
            || self.scope("thead", Scope::Table)
            || self.scope("tfoot", Scope::Table)
    }
    fn start_select(&mut self, t: TagToken) {
        match t.name.as_str() {
            "html" => self.start_body(t),
            "option" => {
                if self.name() == "option" {
                    self.pop();
                }
                self.insert(&t, Namespace::Html, true);
            }
            "optgroup" => {
                if self.name() == "option" {
                    self.pop();
                }
                if self.name() == "optgroup" {
                    self.pop();
                }
                self.insert(&t, Namespace::Html, true);
            }
            "hr" => {
                if self.name() == "option" {
                    self.pop();
                }
                if self.name() == "optgroup" {
                    self.pop();
                }
                self.insert(&t, Namespace::Html, false);
            }
            "input" | "keygen" | "textarea" | "select" => {
                if self.scope("select", Scope::Select) {
                    self.pop_name("select");
                    self.reset_mode();
                    if t.name != "select" {
                        self.start(t);
                    }
                }
            }
            "script" | "template" => self.start_head(t),
            _ => (),
        }
    }
    fn end(&mut self, t: TagToken) {
        match self.mode {
            Mode::Initial => {
                self.ensure_initial();
                self.end(t);
            }
            Mode::BeforeHtml => {
                if matches!(t.name.as_str(), "head" | "body" | "html" | "br") {
                    self.fake("html");
                    self.mode = Mode::BeforeHead;
                    self.end(t);
                }
            }
            Mode::BeforeHead => {
                if matches!(t.name.as_str(), "head" | "body" | "html" | "br") {
                    self.head = Some(self.fake("head"));
                    self.mode = Mode::InHead;
                    self.end(t);
                }
            }
            Mode::InHead => match t.name.as_str() {
                "head" => {
                    self.pop();
                    self.mode = Mode::AfterHead;
                }
                "body" | "br" | "html" => {
                    self.pop();
                    self.mode = Mode::AfterHead;
                    self.end(t);
                }
                "template" => self.close_template(),
                _ => (),
            },
            Mode::AfterHead => match t.name.as_str() {
                "body" | "html" | "br" => {
                    self.fake("body");
                    self.mode = Mode::InBody;
                    self.end_body(t);
                }
                "template" => self.close_template(),
                _ => (),
            },
            Mode::InBody => self.end_body(t),
            Mode::Text => {
                self.pop();
                self.mode = self.original;
                self.tokenizer.set_mode(TextMode::Data, "");
            }
            Mode::InTable => self.end_table(t),
            Mode::InCaption => match t.name.as_str() {
                "caption" | "table" => {
                    if self.scope("caption", Scope::Table) {
                        self.implied(None, false);
                        self.pop_name("caption");
                        self.clear_marker();
                        self.mode = Mode::InTable;
                        if t.name == "table" {
                            self.end_table(t);
                        }
                    }
                }
                "body" | "col" | "colgroup" | "html" | "tbody" | "td" | "tfoot" | "th"
                | "thead" | "tr" => (),
                _ => self.end_body(t),
            },
            Mode::InColumnGroup => match t.name.as_str() {
                "colgroup" => {
                    if self.name() == "colgroup" {
                        self.pop();
                        self.mode = Mode::InTable;
                    }
                }
                "col" => (),
                "template" => self.close_template(),
                _ => {
                    if self.name() == "colgroup" {
                        self.pop();
                        self.mode = Mode::InTable;
                        self.end(t);
                    }
                }
            },
            Mode::InTableBody => match t.name.as_str() {
                "tbody" | "tfoot" | "thead" => {
                    if self.scope(&t.name, Scope::Table) {
                        self.clear_back(&["tbody", "tfoot", "thead", "template", "html"]);
                        self.pop();
                        self.mode = Mode::InTable;
                    }
                }
                "table" => {
                    if self.body_table_scope() {
                        self.clear_back(&["tbody", "tfoot", "thead", "template", "html"]);
                        self.pop();
                        self.mode = Mode::InTable;
                        self.end_table(t);
                    }
                }
                "body" | "caption" | "col" | "colgroup" | "html" | "td" | "th" | "tr" => (),
                _ => self.end_table(t),
            },
            Mode::InRow => match t.name.as_str() {
                "tr" => {
                    if self.scope("tr", Scope::Table) {
                        self.clear_back(&["tr", "template", "html"]);
                        self.pop();
                        self.mode = Mode::InTableBody;
                    }
                }
                "table" => {
                    if self.scope("tr", Scope::Table) {
                        self.clear_back(&["tr", "template", "html"]);
                        self.pop();
                        self.mode = Mode::InTableBody;
                        self.end(t);
                    }
                }
                "tbody" | "tfoot" | "thead" => {
                    if self.scope(&t.name, Scope::Table) || self.scope("tr", Scope::Table) {
                        self.clear_back(&["tr", "template", "html"]);
                        self.pop();
                        self.mode = Mode::InTableBody;
                        self.end(t);
                    }
                }
                "body" | "caption" | "col" | "colgroup" | "html" | "td" | "th" => (),
                _ => self.end_table(t),
            },
            Mode::InCell => match t.name.as_str() {
                "td" | "th" => {
                    if self.scope(&t.name, Scope::Table) {
                        self.implied(None, false);
                        self.pop_name(&t.name);
                        self.clear_marker();
                        self.mode = Mode::InRow;
                    }
                }
                "table" | "tbody" | "tfoot" | "thead" | "tr" => {
                    if self.scope(&t.name, Scope::Table) {
                        self.close_cell();
                        self.end(t);
                    }
                }
                "body" | "caption" | "col" | "colgroup" | "html" => (),
                _ => self.end_body(t),
            },
            Mode::InSelectInTable if select_table_tag(&t.name) => {
                if self.scope(&t.name, Scope::Table) {
                    self.pop_name("select");
                    self.reset_mode();
                    self.end(t);
                }
            }
            Mode::InSelect | Mode::InSelectInTable => match t.name.as_str() {
                "optgroup" => {
                    if self.name() == "option"
                        && self.stack.len() > 1
                        && self.doc.name(self.stack[self.stack.len() - 2]) == "optgroup"
                    {
                        self.pop();
                    }
                    if self.name() == "optgroup" {
                        self.pop();
                    }
                }
                "option" => {
                    if self.name() == "option" {
                        self.pop();
                    }
                }
                "select" => {
                    if self.scope("select", Scope::Select) {
                        self.pop_name("select");
                        self.reset_mode();
                    }
                }
                "template" => self.close_template(),
                _ => (),
            },
            Mode::InTemplate => {
                if t.name == "template" {
                    self.close_template();
                }
            }
            Mode::AfterBody => {
                if t.name == "html" {
                    if !self.fragment {
                        self.mode = Mode::AfterAfterBody;
                    }
                } else {
                    self.mode = Mode::InBody;
                    self.end_body(t);
                }
            }
            Mode::AfterAfterBody => {
                self.mode = Mode::InBody;
                self.end_body(t);
            }
            Mode::InFrameset => {
                if t.name == "frameset" && self.stack.len() > 1 {
                    self.pop();
                    if !self.fragment && self.name() != "frameset" {
                        self.mode = Mode::AfterFrameset;
                    }
                }
            }
            Mode::AfterFrameset => {
                if t.name == "html" {
                    self.mode = Mode::AfterAfterFrameset;
                }
            }
            Mode::AfterAfterFrameset => (),
            Mode::InTableText => unreachable!(),
        }
    }
    fn end_body(&mut self, t: TagToken) {
        match t.name.as_str() {
            "a" | "b" | "i" | "s" | "u" | "em" | "tt" | "big" | "code" | "font" | "nobr"
            | "small" | "strike" | "strong" => self.adoption(&t),
            "body" => {
                if self.scope("body", Scope::Normal) {
                    self.mode = Mode::AfterBody;
                }
            }
            "html" => {
                if self.scope("body", Scope::Normal) {
                    self.mode = Mode::AfterBody;
                    self.end(t);
                }
            }
            "dl" | "ul" | "ol" | "dir" | "div" | "nav" | "pre" | "main" | "menu" | "aside"
            | "button" | "center" | "figure" | "footer" | "header" | "hgroup" | "dialog"
            | "address" | "article" | "details" | "search" | "section" | "summary" | "listing"
            | "fieldset" | "blockquote" | "figcaption" => {
                if self.scope(&t.name, Scope::Normal) {
                    self.implied(None, false);
                    self.pop_name(&t.name);
                }
            }
            "form" => {
                let template = self.in_template();
                let form = self.form;
                if !template {
                    self.form = None;
                }
                if (form.is_some() || template) && self.scope("form", Scope::Normal) {
                    self.implied(None, false);
                    if template {
                        self.pop_name("form");
                    } else if let Some(form) = form {
                        self.remove_stack(form);
                    }
                }
            }
            "p" => {
                if !self.scope("p", Scope::Button) {
                    self.fake("p");
                }
                self.close_p();
            }
            "li" => {
                if self.scope("li", Scope::List) {
                    self.implied(Some("li"), false);
                    self.pop_name("li");
                }
            }
            "dd" | "dt" => {
                if self.scope(&t.name, Scope::Normal) {
                    self.implied(Some(&t.name), false);
                    self.pop_name(&t.name);
                }
            }
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                if self.header_scope() {
                    self.implied(None, false);
                    if let Some(pos) = self.stack.iter().rposition(|&id| {
                        self.doc.namespace(id) == Namespace::Html && header(self.doc.name(id))
                    }) {
                        self.stack.truncate(pos);
                    }
                }
            }
            "applet" | "object" | "marquee" => {
                if self.scope(&t.name, Scope::Normal) {
                    self.implied(None, false);
                    self.pop_name(&t.name);
                    self.clear_marker();
                }
            }
            "br" => {
                self.reconstruct();
                self.insert(&tag("br"), Namespace::Html, false);
                self.frameset_ok = false;
            }
            "template" => self.close_template(),
            _ => self.generic_end(&t),
        }
    }
    fn header_scope(&self) -> bool {
        for &node in self.stack.iter().rev() {
            let ns = self.doc.namespace(node);
            let name = self.doc.name(node);
            if ns == Namespace::Html {
                if header(name) {
                    return true;
                }
                if scope_html(name) {
                    return false;
                }
            } else if special_foreign(ns, name) {
                return false;
            }
        }
        true
    }
    fn end_table(&mut self, t: TagToken) {
        match t.name.as_str() {
            "table" => {
                if self.scope("table", Scope::Table) {
                    self.pop_name("table");
                    self.reset_mode();
                }
            }
            "template" => self.close_template(),
            "body" | "caption" | "col" | "colgroup" | "html" | "tbody" | "td" | "tfoot" | "th"
            | "thead" | "tr" => (),
            _ => self.table_fallback(Token::EndTag(t)),
        }
    }
    fn eof(&mut self) {
        // EOF can close arbitrarily many templates; unwind iteratively.
        loop {
            match self.mode {
                Mode::Initial => {
                    self.ensure_initial();
                }
                Mode::BeforeHtml => {
                    self.fake("html");
                    self.mode = Mode::BeforeHead;
                }
                Mode::BeforeHead => {
                    self.head = Some(self.fake("head"));
                    self.mode = Mode::InHead;
                }
                Mode::InHead => {
                    self.pop();
                    self.mode = Mode::AfterHead;
                }
                Mode::AfterHead => {
                    self.fake("body");
                    self.mode = Mode::InBody;
                }
                Mode::Text => {
                    self.error("eof-in-element-that-can-contain-only-text");
                    self.pop();
                    self.mode = self.original;
                }
                _ if !self.templates.is_empty() && self.in_template() => {
                    self.pop_name("template");
                    self.clear_marker();
                    self.templates.pop();
                    self.reset_mode();
                }
                _ => {
                    self.stopped = true;
                    break;
                }
            }
        }
    }
}

fn tag(name: &str) -> TagToken {
    TagToken {
        name: name.into(),
        attrs: Vec::new(),
        self_closing: false,
    }
}
fn hidden(t: &TagToken) -> bool {
    t.attrs
        .iter()
        .any(|a| a.name == "type" && a.value.eq_ignore_ascii_case("hidden"))
}
fn attrs_equal(a: &[Attribute], b: &[Attribute]) -> bool {
    a.len() == b.len()
        && a.iter().all(|x| {
            b.iter()
                .any(|y| x.name == y.name && x.value == y.value && x.namespace == y.namespace)
        })
}
fn header(n: &str) -> bool {
    matches!(n, "h1" | "h2" | "h3" | "h4" | "h5" | "h6")
}
fn table_structure(n: &str) -> bool {
    matches!(n, "table" | "tbody" | "tfoot" | "thead" | "tr")
}
fn table_void(n: &str) -> bool {
    matches!(
        n,
        "caption" | "col" | "colgroup" | "tbody" | "td" | "tfoot" | "th" | "thead" | "tr"
    )
}
fn select_table_tag(n: &str) -> bool {
    matches!(
        n,
        "caption" | "table" | "tbody" | "tfoot" | "thead" | "tr" | "td" | "th"
    )
}
fn implied_tag(n: &str, thorough: bool) -> bool {
    matches!(
        n,
        "dd" | "dt" | "li" | "optgroup" | "option" | "p" | "rb" | "rp" | "rt" | "rtc"
    ) || thorough && table_void(n) && n != "col"
}
fn scope_html(n: &str) -> bool {
    matches!(
        n,
        "applet" | "caption" | "html" | "marquee" | "object" | "table" | "td" | "template" | "th"
    )
}
fn special_foreign(ns: Namespace, n: &str) -> bool {
    match ns {
        Namespace::MathMl => matches!(n, "mi" | "mo" | "mn" | "ms" | "mtext" | "annotation-xml"),
        Namespace::Svg => matches!(n, "title" | "foreignObject" | "desc"),
        Namespace::Html => false,
    }
}
fn special_html(n: &str) -> bool {
    matches!(
        n,
        "address"
            | "applet"
            | "area"
            | "article"
            | "aside"
            | "base"
            | "basefont"
            | "bgsound"
            | "blockquote"
            | "body"
            | "br"
            | "button"
            | "caption"
            | "center"
            | "col"
            | "colgroup"
            | "dd"
            | "details"
            | "dir"
            | "div"
            | "dl"
            | "dt"
            | "embed"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "frame"
            | "frameset"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "head"
            | "header"
            | "hgroup"
            | "hr"
            | "html"
            | "iframe"
            | "img"
            | "input"
            | "li"
            | "link"
            | "listing"
            | "main"
            | "marquee"
            | "menu"
            | "meta"
            | "nav"
            | "noembed"
            | "noframes"
            | "noscript"
            | "object"
            | "ol"
            | "p"
            | "param"
            | "plaintext"
            | "pre"
            | "script"
            | "section"
            | "select"
            | "source"
            | "style"
            | "summary"
            | "table"
            | "tbody"
            | "td"
            | "template"
            | "textarea"
            | "tfoot"
            | "th"
            | "thead"
            | "title"
            | "tr"
            | "track"
            | "ul"
            | "wbr"
            | "xmp"
    )
}
fn foreign_exit(t: &TagToken) -> bool {
    matches!(
        t.name.as_str(),
        "b" | "big"
            | "blockquote"
            | "body"
            | "br"
            | "center"
            | "code"
            | "dd"
            | "div"
            | "dl"
            | "dt"
            | "em"
            | "embed"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "head"
            | "hr"
            | "i"
            | "img"
            | "li"
            | "listing"
            | "menu"
            | "meta"
            | "nobr"
            | "ol"
            | "p"
            | "pre"
            | "ruby"
            | "s"
            | "small"
            | "span"
            | "strong"
            | "strike"
            | "sub"
            | "sup"
            | "table"
            | "tt"
            | "u"
            | "ul"
            | "var"
    ) || t.name == "font"
        && t.attrs
            .iter()
            .any(|a| matches!(a.name.as_str(), "color" | "size" | "face"))
}
fn adjust_foreign(t: &mut TagToken, ns: Namespace, name: bool) {
    if ns == Namespace::Svg {
        if name {
            if let Some((_, adjusted)) = crate::parser_data::SVG_TAG_ADJUSTMENTS
                .iter()
                .find(|(lower, _)| *lower == t.name)
            {
                t.name = (*adjusted).into();
            }
        }
        for attr in &mut t.attrs {
            if let Some((_, adjusted)) = crate::parser_data::SVG_ATTR_ADJUSTMENTS
                .iter()
                .find(|(lower, _)| *lower == attr.name)
            {
                attr.name = (*adjusted).into();
            }
        }
    } else if ns == Namespace::MathMl {
        for attr in &mut t.attrs {
            if attr.name == "definitionurl" {
                attr.name = "definitionURL".into();
                break;
            }
        }
    }
    for attr in &mut t.attrs {
        let adjustment = match attr.name.as_str() {
            "xlink:actuate" => Some(("xlink", "actuate", "http://www.w3.org/1999/xlink")),
            "xlink:arcrole" => Some(("xlink", "arcrole", "http://www.w3.org/1999/xlink")),
            "xlink:href" => Some(("xlink", "href", "http://www.w3.org/1999/xlink")),
            "xlink:role" => Some(("xlink", "role", "http://www.w3.org/1999/xlink")),
            "xlink:show" => Some(("xlink", "show", "http://www.w3.org/1999/xlink")),
            "xlink:title" => Some(("xlink", "title", "http://www.w3.org/1999/xlink")),
            "xlink:type" => Some(("xlink", "type", "http://www.w3.org/1999/xlink")),
            "xml:lang" => Some(("xml", "lang", "http://www.w3.org/XML/1998/namespace")),
            "xml:space" => Some(("xml", "space", "http://www.w3.org/XML/1998/namespace")),
            "xmlns" => Some(("", "xmlns", "http://www.w3.org/2000/xmlns/")),
            "xmlns:xlink" => Some(("xmlns", "xlink", "http://www.w3.org/2000/xmlns/")),
            _ => None,
        };
        if let Some((prefix, name, ns)) = adjustment {
            attr.prefix = Some(prefix.into());
            attr.name = name.into();
            attr.namespace = Some(ns.into());
        }
    }
}
fn doctype_mode(t: &DoctypeToken) -> DocumentMode {
    if t.force_quirks || t.name.as_deref() != Some("html") {
        return DocumentMode::Quirks;
    }
    if t.system_id.as_ref().is_some_and(|s| {
        s.eq_ignore_ascii_case("http://www.ibm.com/data/dtd/v11/ibmxhtml1-transitional.dtd")
    }) {
        return DocumentMode::Quirks;
    }
    if let Some(public) = &t.public_id {
        let public = public.to_ascii_lowercase();
        if matches!(
            public.as_str(),
            "-//w3o//dtd w3 html strict 3.0//en//" | "-/w3c/dtd html 4.0 transitional/en" | "html"
        ) || crate::parser_data::QUIRKS_PUBLIC_PREFIXES
            .iter()
            .any(|prefix| public.starts_with(prefix))
        {
            return DocumentMode::Quirks;
        }
        let html4 = public.starts_with("-//w3c//dtd html 4.01 frameset//")
            || public.starts_with("-//w3c//dtd html 4.01 transitional//");
        if html4 && t.system_id.is_none() {
            return DocumentMode::Quirks;
        }
        if html4
            || public.starts_with("-//w3c//dtd xhtml 1.0 frameset//")
            || public.starts_with("-//w3c//dtd xhtml 1.0 transitional//")
        {
            return DocumentMode::LimitedQuirks;
        }
    }
    DocumentMode::NoQuirks
}

#[cfg(test)]
mod tests {
    use super::*;
    fn descendants<'a>(doc: &'a Document, name: &str) -> Vec<NodeId> {
        doc.nodes
            .iter()
            .enumerate()
            .filter_map(|(id, node)| {
                matches!(&node.kind, NodeKind::Element { name: n, .. } if n == name).then_some(id)
            })
            .collect()
    }
    fn parent_name<'a>(doc: &'a Document, id: NodeId) -> &'a str {
        doc.name(doc.nodes[id].parent.unwrap())
    }
    #[test]
    fn implied_document_structure() {
        let d = parse("<!doctype html><title>Hi</title><p>A<p>B");
        assert_eq!(d.mode, DocumentMode::NoQuirks);
        assert_eq!(descendants(&d, "html").len(), 1);
        assert_eq!(descendants(&d, "head").len(), 1);
        assert_eq!(descendants(&d, "body").len(), 1);
        assert_eq!(descendants(&d, "p").len(), 2);
        for id in descendants(&d, "p") {
            assert_eq!(parent_name(&d, id), "body");
        }
    }
    #[test]
    fn table_foster_and_implied_sections() {
        let d = parse("<!doctype html><table>before<tr><td>A<td>B</table>");
        let body = descendants(&d, "body")[0];
        assert!(
            matches!(&d.nodes[d.nodes[body].children[0]].kind, NodeKind::Text(s) if s == "before")
        );
        let tr = descendants(&d, "tr")[0];
        assert_eq!(parent_name(&d, tr), "tbody");
        assert_eq!(descendants(&d, "td").len(), 2);
    }
    #[test]
    fn adoption_agency_reconstructs_formatting() {
        let d = parse("<!doctype html><p><b>one<i>two</b>three</i>four");
        assert_eq!(descendants(&d, "i").len(), 2);
        assert_eq!(parent_name(&d, descendants(&d, "i")[1]), "p");
    }
    #[test]
    fn template_has_separate_fragment() {
        let d = parse("<!doctype html><template><tr><td>inside</template><p>outside");
        let template = descendants(&d, "template")[0];
        let NodeKind::Element {
            template_content: Some(content),
            ..
        } = &d.nodes[template].kind
        else {
            panic!()
        };
        assert!(d.nodes[template].children.is_empty());
        assert_eq!(d.name(d.nodes[*content].children[0]), "tr");
        assert_eq!(parent_name(&d, descendants(&d, "p")[0]), "body");
    }
    #[test]
    fn svg_adjustment_and_html_integration() {
        let d = parse("<!doctype html><svg viewbox='0 0 1 1'><lineargradient/><foreignobject><p>html</p></foreignobject></svg>");
        let svg = descendants(&d, "svg")[0];
        assert!(d
            .element(svg)
            .unwrap()
            .2
            .iter()
            .any(|a| a.name == "viewBox"));
        let gradient = descendants(&d, "linearGradient")[0];
        assert_eq!(d.namespace(gradient), Namespace::Svg);
        let p = descendants(&d, "p")[0];
        assert_eq!(d.namespace(p), Namespace::Html);
        assert_eq!(parent_name(&d, p), "foreignObject");
    }
    #[test]
    fn fragment_table_context() {
        let d = parse_fragment(
            "<tr><td>x",
            Some(FragmentContext {
                name: "table".into(),
                namespace: Namespace::Html,
                attrs: Vec::new(),
            }),
        );
        assert_eq!(d.name(d.nodes[d.root].children[0]), "tbody");
        assert_eq!(descendants(&d, "td").len(), 1);
    }
    #[test]
    fn fragment_context_is_not_a_tokenized_start_tag() {
        for (name, html) in [
            ("script", "<!-- inside </script> -->"),
            ("xmp", "foo</xmp>"),
        ] {
            let d = parse_fragment(
                html,
                Some(FragmentContext {
                    name: name.into(),
                    namespace: Namespace::Html,
                    attrs: Vec::new(),
                }),
            );
            assert!(
                matches!(&d.nodes[d.nodes[d.root].children[0]].kind, NodeKind::Text(text) if text == html)
            );
        }
    }
    #[test]
    fn eof_unwinds_nested_templates_without_recursive_calls() {
        let mut html = "<!doctype html>".to_string();
        for _ in 0..1024 {
            html.push_str("<template>");
        }
        let d = parse(&html);
        assert_eq!(descendants(&d, "template").len(), 1024);
        assert_eq!(descendants(&d, "body").len(), 1);
    }
    #[test]
    fn select_closes_options() {
        let d = parse("<!doctype html><select><option>a<option>b<optgroup><option>c</select>");
        let options = descendants(&d, "option");
        assert_eq!(options.len(), 3);
        assert_eq!(parent_name(&d, options[0]), "select");
        assert_eq!(parent_name(&d, options[1]), "select");
        assert_eq!(parent_name(&d, options[2]), "optgroup");
    }
    #[test]
    fn frameset_and_legacy_modes() {
        let d = parse("<!doctype html><frameset><frame src=x></frameset>");
        assert!(descendants(&d, "body").is_empty());
        assert_eq!(descendants(&d, "frame").len(), 1);
        assert_eq!(parse("hello").mode, DocumentMode::Quirks);
        assert_eq!(
            parse("<!doctype html PUBLIC '-//W3C//DTD XHTML 1.0 Transitional//EN' 'x'>").mode,
            DocumentMode::LimitedQuirks
        );
    }
}

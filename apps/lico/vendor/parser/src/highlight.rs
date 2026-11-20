use crate::Rule;
use pest::iterators::Pair;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightKind {
    Binding,
    Node,
    Reference,
    Number,
    Sample,
    Script,
    Comment,
    Operator,
}

/// Byte offsets into the source passed to the parser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HighlightSpan {
    pub start: usize,
    pub end: usize,
    pub kind: HighlightKind,
}

pub(crate) fn collect(pair: Pair<'_, Rule>, spans: &mut Vec<HighlightSpan>) {
    use HighlightKind::*;

    let range = pair.as_span();
    let kind = match pair.as_rule() {
        Rule::line => {
            let mut parts = pair.into_inner();
            if let Some(binding) = parts.next() {
                let binding = binding.as_span();
                spans.push(HighlightSpan {
                    start: binding.start(),
                    end: binding.end(),
                    kind: Binding,
                });
            }
            for part in parts {
                collect(part, spans);
            }
            return;
        }
        Rule::node => {
            if let Some(node) = pair.into_inner().next() {
                let keyword = node
                    .as_str()
                    .split([' ', '\t', '\n', ',', '|'])
                    .next()
                    .unwrap_or_default();
                if node.as_rule() != Rule::points
                    && (node.as_rule() != Rule::get || keyword == "get")
                {
                    spans.push(HighlightSpan {
                        start: range.start(),
                        end: range.start() + keyword.len(),
                        kind: Node,
                    });
                }
                collect(node, spans);
            }
            return;
        }
        Rule::reference | Rule::note_ref => Reference,
        Rule::number | Rule::integer => Number,
        Rule::symbol => Sample,
        Rule::code => Script,
        Rule::eval_content => {
            spans.push(HighlightSpan {
                start: range.start() - 1,
                end: range.end() + 1,
                kind: Script,
            });
            return;
        }
        Rule::comment => Comment,
        Rule::chain_operator | Rule::rest | Rule::is_looping => Operator,
        _ => {
            for child in pair.into_inner() {
                collect(child, spans);
            }
            return;
        }
    };
    spans.push(HighlightSpan {
        start: range.start(),
        end: range.end(),
        kind,
    });
}

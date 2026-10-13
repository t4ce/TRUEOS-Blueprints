//! Prepare selector components and import URLs once, immediately after CSS parsing.
use lightningcss::{rules::CssRule, selector::Component};
use url::Url;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) enum SelectorKey {
    Id(String),
    Class(String),
    Tag(String),
    Attribute(String),
}

pub(crate) struct PreparedSelector {
    pub components: Vec<Component<'static>>,
    pub specificity: u32,
    pub key: Option<SelectorKey>,
    pub never_matches: bool,
}

pub(crate) enum PreparedRule {
    Style {
        index: usize,
        selectors: Vec<PreparedSelector>,
    },
    Media {
        index: usize,
        children: PreparedRules,
    },
    Import {
        index: usize,
        url: Option<String>,
    },
}

#[derive(Default)]
pub(crate) struct PreparedRules {
    pub rules: Vec<PreparedRule>,
}

impl PreparedRules {
    pub fn new(rules: &[CssRule<'static>], origin: &str) -> Self {
        let base = Url::parse(origin).ok();
        let mut prepared = Self::default();
        for (index, rule) in rules.iter().enumerate() {
            match rule {
                CssRule::Style(rule) => {
                    let selectors = rule
                        .selectors
                        .0
                        .iter()
                        .map(|selector| {
                            let components: Vec<_> =
                                selector.iter_raw_match_order().cloned().collect();
                            let subject: Vec<_> = components
                                .iter()
                                .take_while(|component| {
                                    !matches!(component, Component::Combinator(_))
                                })
                                .collect();
                            // A required component of the rightmost compound is a
                            // safe lookup key. Ancestor and :is/:not arguments aren't.
                            let key = subject
                                .iter()
                                .find_map(|component| match component {
                                    Component::ID(value) => {
                                        Some(SelectorKey::Id(value.as_ref().to_owned()))
                                    }
                                    _ => None,
                                })
                                .or_else(|| {
                                    subject.iter().find_map(|component| match component {
                                        Component::Class(value) => {
                                            Some(SelectorKey::Class(value.as_ref().to_owned()))
                                        }
                                        _ => None,
                                    })
                                })
                                .or_else(|| {
                                    subject.iter().find_map(|component| match component {
                                        Component::LocalName(value) => {
                                            Some(SelectorKey::Tag(value.name.as_ref().to_owned()))
                                        }
                                        _ => None,
                                    })
                                })
                                .or_else(|| {
                                    subject.iter().find_map(|component| match component {
                                        Component::AttributeInNoNamespaceExists {
                                            local_name,
                                            ..
                                        }
                                        | Component::AttributeInNoNamespace {
                                            local_name, ..
                                        } => Some(SelectorKey::Attribute(
                                            local_name.as_ref().to_owned(),
                                        )),
                                        _ => None,
                                    })
                                });
                            // These top-level conditions always fail Hull's
                            // current matcher; functional arguments stay intact.
                            let never_matches =
                                components.iter().any(|component| match component {
                                    Component::PseudoElement(_) => true,
                                    Component::NonTSPseudoClass(pseudo) => !matches!(
                                        pseudo,
                                        lightningcss::selector::PseudoClass::Link
                                            | lightningcss::selector::PseudoClass::AnyLink(_)
                                            | lightningcss::selector::PseudoClass::Disabled
                                            | lightningcss::selector::PseudoClass::Enabled
                                            | lightningcss::selector::PseudoClass::Checked
                                    ),
                                    _ => false,
                                });
                            PreparedSelector {
                                never_matches,
                                components,
                                specificity: selector.specificity(),
                                key,
                            }
                        })
                        .collect();
                    prepared
                        .rules
                        .push(PreparedRule::Style { index, selectors });
                }
                CssRule::Media(rule) => prepared.rules.push(PreparedRule::Media {
                    index,
                    children: Self::new(&rule.rules.0, origin),
                }),
                CssRule::Import(rule) => {
                    let url = base
                        .as_ref()
                        .and_then(|base| base.join(&rule.url).ok())
                        .map(|mut url| {
                            url.set_fragment(None);
                            url.to_string()
                        });
                    prepared.rules.push(PreparedRule::Import { index, url });
                }
                _ => {}
            }
        }
        prepared
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Attribute {
    pub name: String,
    pub value: String,
    pub namespace: Option<String>,
    pub prefix: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TagToken {
    pub name: String,
    pub attrs: Vec<Attribute>,
    pub self_closing: bool,
}
impl TagToken {
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|attr| attr.name == name)
            .map(|attr| attr.value.as_str())
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DoctypeToken {
    pub name: Option<String>,
    pub public_id: Option<String>,
    pub system_id: Option<String>,
    pub force_quirks: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    Character(String),
    NullCharacter(String),
    Whitespace(String),
    StartTag(TagToken),
    EndTag(TagToken),
    Comment(String),
    Doctype(DoctypeToken),
    Eof,
}

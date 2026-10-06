use syn;

// An iterator yielding all conrod attributes in the given attributes.
pub struct ConrodAttrs<I> {
    attrs: I,
}

pub fn conrod_attrs<'a, I>(attrs: I) -> ConrodAttrs<I::IntoIter>
    where I: IntoIterator<Item=&'a syn::Attribute>,
{
    ConrodAttrs { attrs: attrs.into_iter() }
}

impl<'a, I> Iterator for ConrodAttrs<I>
    where I: Iterator<Item=&'a syn::Attribute>,
{
    type Item = Vec<syn::Meta>;
    fn next(&mut self) -> Option<Self::Item> {
        while let Some(attr) = self.attrs.next() {
            if attr.path().is_ident("conrod") && matches!(attr.meta, syn::Meta::List(_)) {
                let nested = attr
                    .parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
                    .expect("invalid conrod attribute");
                return Some(nested.into_iter().collect());
            }
        }
        None
    }
}

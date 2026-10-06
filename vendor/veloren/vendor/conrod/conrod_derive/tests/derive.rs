// Exercise public and internal derives against the vendored Conrod API.
extern crate conrod_core;
#[macro_use] extern crate conrod_derive;

use conrod_core::theme::WidgetDefault;
pub use conrod_core::{Theme, widget};
use std::any::TypeId;

#[derive(WidgetCommon)]
struct PublicWidget<T> {
    #[conrod(common_builder)]
    common: widget::CommonBuilder,
    payload: T,
}

#[derive(WidgetCommon_)]
struct InternalWidget<T> {
    #[conrod(common_builder)]
    common: widget::CommonBuilder,
    payload: T,
}

#[derive(Clone, Copy, Debug, PartialEq, WidgetStyle)]
struct PublicStyle<T: Copy + std::fmt::Debug + PartialEq + 'static> {
    #[conrod(default = "42")]
    value: Option<u32>,
    // Fields without a Conrod attribute must not generate getters.
    payload: T,
}

#[derive(Clone, Copy, Debug, PartialEq, WidgetStyle_)]
struct InternalStyle {
    #[conrod(default = "21 * 2")]
    value: Option<u32>,
}

#[test]
fn common_derives_access_the_marked_field() {
    use widget::Common;

    let mut public = PublicWidget {
        common: widget::CommonBuilder::default(),
        payload: "public",
    };
    let mut internal = InternalWidget {
        common: widget::CommonBuilder {
            is_floating: true,
            ..widget::CommonBuilder::default()
        },
        payload: "internal",
    };
    assert!(!public.common().is_floating);
    assert!(internal.common().is_floating);
    public.common_mut().is_floating = true;
    internal.common_mut().is_floating = false;
    assert!(public.common().is_floating);
    assert!(!internal.common().is_floating);
    assert_eq!(public.payload, "public");
    assert_eq!(internal.payload, "internal");
}

#[test]
fn style_derives_preserve_override_theme_and_expression_defaults() {
    let public = PublicStyle {
        value: None,
        payload: (),
    };
    let internal = InternalStyle { value: None };
    let empty_theme = Theme::default();
    assert_eq!(public.value(&empty_theme), 42);
    assert_eq!(internal.value(&empty_theme), 42);

    let mut public_theme = Theme::default();
    public_theme.widget_styling.insert(
        TypeId::of::<PublicStyle<()>>(),
        WidgetDefault::new(Box::new(PublicStyle {
            value: Some(7),
            payload: (),
        })),
    );
    let mut internal_theme = Theme::default();
    internal_theme.widget_styling.insert(
        TypeId::of::<InternalStyle>(),
        WidgetDefault::new(Box::new(InternalStyle { value: Some(8) })),
    );
    assert_eq!(public.value(&public_theme), 7);
    assert_eq!(internal.value(&internal_theme), 8);
    assert_eq!(
        PublicStyle {
            value: Some(9),
            ..public
        }
        .value(&public_theme),
        9
    );
    assert_eq!(InternalStyle { value: Some(10) }.value(&internal_theme), 10);
    assert_eq!(public.payload, ());
}

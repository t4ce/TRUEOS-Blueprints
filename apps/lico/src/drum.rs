use glicol_parser::{HighlightKind, get_ast_with_spans};

/// Normalize only parsed bindings and references, leaving scripts and comments alone.
pub fn display_alias(code: &str) -> Option<String> {
    if !code.contains("~drum") {
        return None;
    }
    let (_, spans) = get_ast_with_spans(code).ok()?;
    let mut updated = String::new();
    let mut offset = 0;
    for span in spans {
        if !matches!(span.kind, HighlightKind::Binding | HighlightKind::Reference) {
            continue;
        }
        let replacement = match &code[span.start..span.end] {
            "~drum" => "~🥁",
            "~drum.." => "~🥁..",
            _ => continue,
        };
        updated.push_str(&code[offset..span.start]);
        updated.push_str(replacement);
        offset = span.end;
    }
    if offset == 0 {
        None
    } else {
        updated.push_str(&code[offset..]);
        Some(updated)
    }
}

pub fn track_to_append(draft: &str) -> String {
    // Tokenize the draft even if it is still incomplete. Skipping occupied
    // names also prevents replacing a binding on a semicolon-separated line.
    let names: std::collections::HashSet<_> = draft
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '~' | '🥁')))
        .collect();
    let (voice, output) = (1..)
        .map(|number| {
            let suffix = if number == 1 {
                String::new()
            } else {
                number.to_string()
            };
            (format!("~🥁{suffix}"), format!("drum{suffix}"))
        })
        .find(|(voice, output)| {
            !names.contains(voice.as_str())
                && !names.contains(output.as_str())
                && (*voice != "~🥁" || !names.contains("~drum"))
        })
        .expect("draft cannot contain infinitely many names");
    let separator = if draft.is_empty() {
        ""
    } else if draft.ends_with('\n') {
        "\n"
    } else {
        "\n\n"
    };
    format!("{separator}{voice}: seq 60 _ 60 _ >> sp \\808bd;\n{output}: {voice} >> mul 0.25;")
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
mod menu {
    use v::bp_abi::{
        TrueosUi4ContextMenuEntry, TrueosUi4ContextMenuEvent, UI4_CONTEXT_MENU_TERMINAL_WINDOW_ID,
        trueos_cabi_ui4_context_menu_event_take, trueos_cabi_ui4_context_menu_register,
    };

    const ADD_DRUM: u32 = 1;

    pub fn register() -> bool {
        let label = "Add Drum";
        let entry = TrueosUi4ContextMenuEntry {
            label_ptr: label.as_ptr(),
            label_len: label.len(),
            action_id: ADD_DRUM,
            enabled: 1,
        };
        // Registration copies the label and records it against our terminal lease.
        unsafe {
            trueos_cabi_ui4_context_menu_register(UI4_CONTEXT_MENU_TERMINAL_WINDOW_ID, &entry, 1)
                == 0
        }
    }

    pub fn take_add_drum() -> bool {
        let mut event = TrueosUi4ContextMenuEvent::default();
        let status = unsafe {
            trueos_cabi_ui4_context_menu_event_take(UI4_CONTEXT_MENU_TERMINAL_WINDOW_ID, &mut event)
        };
        status == 0 && event.selected != 0 && event.action_id == ADD_DRUM
    }
}

#[cfg(any(target_os = "trueos", target_os = "zkvm"))]
pub use menu::{register, take_add_drum};

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
pub fn register() -> bool {
    false
}

#[cfg(not(any(target_os = "trueos", target_os = "zkvm")))]
pub fn take_add_drum() -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use glicol_parser::get_ast;

    #[test]
    fn alias_preserves_parser_identity_and_non_reference_text() {
        let legacy = "~drum: sin 55; out: mix ~drum >> mul 0.1; // ~drum";
        let updated = display_alias(legacy).unwrap();
        assert_eq!(updated, "~🥁: sin 55; out: mix ~🥁 >> mul 0.1; // ~drum");
        assert_eq!(get_ast(legacy).unwrap(), get_ast(&updated).unwrap());
        assert!(display_alias("~drummer: sin 55; out: ~drummer;").is_none());
        assert!(display_alias("out: sin 55 >> meta `print(\"~drum\"); input`;").is_none());
        assert_eq!(
            get_ast("~drum: sin 55; out: add ~🥁;").unwrap(),
            get_ast("~🥁: sin 55; out: add ~drum;").unwrap(),
        );
    }

    #[test]
    fn repeated_adds_create_distinct_playable_chains() {
        let mut code = String::from("~drum: sin 55;");
        for _ in 0..2 {
            code.push_str(&track_to_append(&code));
        }
        let ast = get_ast(&code).unwrap();
        for name in ["~drum", "~🥁2", "drum2", "~🥁3", "drum3"] {
            assert!(ast.nodes.contains_key(name), "{name}");
        }
    }
}

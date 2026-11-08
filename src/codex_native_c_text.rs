//! Native text selection for exact pinned parser and regex sources.
use std::path::Path;

pub(super) fn compiler_options(name: &str, binding: &str) -> String {
    if !matches!(name, "tree-sitter" | "onig_sys") {
        return String::new();
    }
    let mut options = format!(
        "\n        {binding}.define(\"TRUEOS_BLUEPRINT_NATIVE_TEXT\", None).include(\".\").flag(\"-include\").flag(\"trueos_native_text.h\");"
    );
    if name == "onig_sys" {
        options.push_str(&format!(
            "\n        if std::env::var_os(\"CARGO_FEATURE_PRINT_DEBUG\").is_some() {{ panic!(\"Onig FILE-based print-debug is unavailable on TRUEOS; regex errors and warning callbacks remain supported\"); }}\n        {binding}.define(\"ONIG_NO_PRINT\", None);"
        ));
    }
    options
}

fn replace_once(text: &mut String, before: &str, after: &str, label: &str) -> Result<(), String> {
    if text.matches(before).count() != 1 {
        return Err(format!("pinned native text boundary drifted: {label}"));
    }
    *text = text.replace(before, after);
    Ok(())
}

fn adapt_file(staged: &Path, relative: &str, adapt: impl FnOnce(&mut String) -> Result<(), String>) -> Result<(), String> {
    let path = staged.join(relative);
    let mut text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    adapt(&mut text)?;
    std::fs::write(path, text).map_err(|e| e.to_string())
}

pub(super) fn adapt(staged: &Path, name: &str) -> Result<(), String> {
    if !matches!(name, "tree-sitter" | "onig_sys") {
        return Ok(());
    }
    std::fs::write(staged.join("trueos_native_text.h"), include_str!("native_c_text.h"))
        .map_err(|e| e.to_string())?;
    if name != "tree-sitter" {
        return Ok(());
    }
    // Native files expose object capabilities, not Unix descriptors. Keep the
    // parser's callback logger and text graph writer; omit only FD entry points.
    adapt_file(staged, "binding_rust/lib.rs", |text| {
        let before = "#[cfg(not(target_os = \"wasi\"))]";
        if text.matches(before).count() != 4 {
            return Err("Tree-sitter descriptor DOT method selectors drifted".into());
        }
        *text = text.replace(before, "#[cfg(not(any(target_os = \"wasi\", target_os = \"trueos\")))]");
        Ok(())
    })?;
    adapt_file(staged, "src/parser.c", |text| {
        replace_once(text, "void ts_parser_print_dot_graphs(TSParser *self, int fd) {", "#if !defined(TRUEOS_BLUEPRINT_NATIVE_TEXT)\nvoid ts_parser_print_dot_graphs(TSParser *self, int fd) {", "parser fd entry")?;
        let before = "\nconst size_t *ts_parser_cancellation_flag(";
        let after = "\n#else\nvoid ts_parser_print_dot_graphs(TSParser *self, int fd) {\n  if (fd >= 0) {\n    static const unsigned char message[] = \"Tree-sitter raw-descriptor DOT output is unavailable on TRUEOS; use the text callback\\n\";\n    trueos_cabi_write(2, message, sizeof(message) - 1);\n    abort();\n  }\n  self->dot_graph_file = NULL;\n}\n/* Caller owns the callback sink for as long as it is installed, like TSLogger. */\nvoid ts_parser_set_text_sink(TSParser *self, TrueosCTextSink *sink) {\n  self->dot_graph_file = sink;\n}\n#endif\n\nconst size_t *ts_parser_cancellation_flag(";
        replace_once(text, before, after, "parser next entry")
    })?;
    adapt_file(staged, "src/tree.c", |text| {
        replace_once(text, "#ifdef _WIN32", "#if defined(TRUEOS_BLUEPRINT_NATIVE_TEXT)\n\nvoid ts_tree_write_dot_graph(const TSTree *self, TrueosCTextSink *sink) {\n  ts_subtree_print_dot_graph(self->root, self->language, sink);\n}\n\n#elif defined(_WIN32)", "tree fd selector")
    })?;
    Ok(())
}

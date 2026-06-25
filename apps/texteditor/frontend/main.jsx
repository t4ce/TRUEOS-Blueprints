import "@blocknote/core/fonts/inter.css";
import "@blocknote/core/style.css";
import "@blocknote/react/style.css";
import "@blocknote/mantine/style.css";
import "./style.css";

import React, { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { useCreateBlockNote, useEditorChange } from "@blocknote/react";
import { BlockNoteView } from "@blocknote/mantine";

const API = "/api/texteditor";
const defaultBlocks = [
  {
    type: "heading",
    props: { level: 1 },
    content: "TRUEOS Text Editor",
  },
  {
    type: "paragraph",
    content: "Start writing here. Use the slash menu for headings, lists, tables, images, and code blocks.",
  },
];

function fileNameFor(format) {
  if (format === "html") return "trueos-texteditor.html";
  if (format === "md") return "trueos-texteditor.md";
  if (format === "txt") return "trueos-texteditor.txt";
  return "trueos-texteditor.blocknote.json";
}

function downloadBlob(format, text, type) {
  const blob = new Blob([text], { type });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = fileNameFor(format);
  document.body.append(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
}

function formatTime(timestampSeconds) {
  if (!timestampSeconds) return "not saved";
  return new Date(timestampSeconds * 1000).toLocaleString();
}

function EditorSurface({ initialDocument }) {
  const editor = useCreateBlockNote({
    initialContent: initialDocument?.blocks?.length ? initialDocument.blocks : defaultBlocks,
  });
  const [status, setStatus] = useState("ready");
  const [savedAt, setSavedAt] = useState(initialDocument?.updatedAtS ?? 0);
  const [dirty, setDirty] = useState(false);
  const [wordCount, setWordCount] = useState(0);
  const saveTimer = useRef(null);
  const saving = useRef(false);

  const updateWordCount = useCallback(async () => {
    const markdown = await editor.blocksToMarkdownLossy(editor.document);
    const count = markdown.trim().split(/\s+/).filter(Boolean).length;
    setWordCount(count);
  }, [editor]);

  const save = useCallback(async () => {
    if (saving.current) return;
    saving.current = true;
    setStatus("saving");
    try {
      const [markdown, html] = await Promise.all([
        editor.blocksToMarkdownLossy(editor.document),
        editor.blocksToHTMLLossy(editor.document),
      ]);
      const res = await fetch(`${API}/document`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          blocks: editor.document,
          markdown,
          html,
        }),
      });
      const json = await res.json();
      if (!res.ok || !json.ok) throw new Error(json.error || `HTTP ${res.status}`);
      setSavedAt(json.document.updatedAtS);
      setDirty(false);
      setStatus("saved");
    } catch (err) {
      setStatus(err.message);
    } finally {
      saving.current = false;
    }
  }, [editor]);

  useEditorChange(() => {
    setDirty(true);
    setStatus("editing");
    updateWordCount();
    if (saveTimer.current) window.clearTimeout(saveTimer.current);
    saveTimer.current = window.setTimeout(save, 900);
  }, editor);

  useEffect(() => {
    updateWordCount();
    return () => {
      if (saveTimer.current) window.clearTimeout(saveTimer.current);
    };
  }, [updateWordCount]);

  const exportCurrent = useCallback(async (format) => {
    if (format === "json") {
      downloadBlob("json", JSON.stringify(editor.document, null, 2), "application/json;charset=utf-8");
      return;
    }
    if (format === "html") {
      const html = await editor.blocksToHTMLLossy(editor.document);
      downloadBlob("html", html, "text/html;charset=utf-8");
      return;
    }
    const markdown = await editor.blocksToMarkdownLossy(editor.document);
    downloadBlob(format, markdown, format === "txt" ? "text/plain;charset=utf-8" : "text/markdown;charset=utf-8");
  }, [editor]);

  const badge = useMemo(() => {
    if (dirty) return "Unsaved";
    if (status === "saved") return "Saved";
    return status;
  }, [dirty, status]);

  return React.createElement("div", { className: "shell" },
    React.createElement("header", { className: "topbar" },
      React.createElement("div", { className: "brand" },
        React.createElement("div", { className: "brand-mark" }, "T"),
        React.createElement("div", null,
          React.createElement("h1", null, "TRUEOS Text Editor"),
          React.createElement("p", null, `${wordCount} words / ${formatTime(savedAt)}`)
        )
      ),
      React.createElement("div", { className: "actions" },
        React.createElement("span", { className: dirty ? "pill warn" : "pill ok" }, badge),
        React.createElement("button", { type: "button", className: "btn primary", onClick: save }, "Save"),
        React.createElement("button", { type: "button", className: "btn", onClick: () => exportCurrent("json") }, "JSON"),
        React.createElement("button", { type: "button", className: "btn", onClick: () => exportCurrent("md") }, "MD"),
        React.createElement("button", { type: "button", className: "btn", onClick: () => exportCurrent("html") }, "HTML")
      )
    ),
    React.createElement("main", { className: "editor-frame" },
      React.createElement(BlockNoteView, {
        editor,
        theme: "light",
        className: "trueos-editor",
      })
    )
  );
}

function Boot() {
  const [state, setState] = useState({ loading: true, document: null, error: "" });

  useEffect(() => {
    let cancelled = false;
    fetch(`${API}/document`, { cache: "no-store" })
      .then(async (res) => {
        const json = await res.json();
        if (!res.ok || !json.ok) throw new Error(json.error || `HTTP ${res.status}`);
        if (!cancelled) setState({ loading: false, document: json.document, error: "" });
      })
      .catch((err) => {
        if (!cancelled) setState({ loading: false, document: null, error: err.message });
      });
    return () => {
      cancelled = true;
    };
  }, []);

  if (state.loading) {
    return React.createElement("div", { className: "boot" }, "Loading editor");
  }
  if (state.error) {
    return React.createElement("div", { className: "boot error" }, state.error);
  }
  return React.createElement(EditorSurface, { initialDocument: state.document });
}

createRoot(document.getElementById("app")).render(React.createElement(Boot));

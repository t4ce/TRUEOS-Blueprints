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
  if (!timestampSeconds) return "not stored yet";
  return new Date(timestampSeconds * 1000).toLocaleString();
}

function suggestedFilename(format) {
  if (format === "html") return "document.html";
  if (format === "json") return "document.blocknote.json";
  if (format === "txt") return "document.txt";
  return "document.md";
}

function joinPath(dir, name) {
  const cleanDir = String(dir || ".").replace(/\/+$/g, "");
  const cleanName = String(name || "").replace(/^\/+/g, "");
  if (!cleanDir || cleanDir === ".") return cleanName;
  return `${cleanDir}/${cleanName}`;
}

function parentPath(path) {
  const clean = String(path || ".").replace(/\/+$/g, "");
  if (!clean || clean === "." || !clean.includes("/")) return ".";
  return clean.split("/").slice(0, -1).join("/") || ".";
}

function StoreDialog({ editor, onClose, onStored }) {
  const [dir, setDir] = useState("texteditor");
  const [entries, setEntries] = useState([]);
  const [filename, setFilename] = useState("document.md");
  const [format, setFormat] = useState("md");
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);

  const loadDir = useCallback(async (nextDir = dir) => {
    setBusy(true);
    setMessage("");
    try {
      const res = await fetch(`${API}/fs/list?path=${encodeURIComponent(nextDir || ".")}`, { cache: "no-store" });
      const json = await res.json();
      if (!res.ok || !json.ok) throw new Error(json.error || `HTTP ${res.status}`);
      setDir(json.path || ".");
      setEntries(json.entries || []);
    } catch (err) {
      setMessage(err.message);
    } finally {
      setBusy(false);
    }
  }, [dir]);

  useEffect(() => {
    loadDir("texteditor");
  }, []);

  const selectFormat = useCallback((nextFormat) => {
    setFormat(nextFormat);
    setFilename((current) => {
      const base = current.replace(/(\.blocknote)?\.(json|md|markdown|html|txt)$/i, "");
      const suggested = suggestedFilename(nextFormat);
      const ext = suggested.slice("document".length);
      return `${base || "document"}${ext}`;
    });
  }, []);

  const createFolder = useCallback(async () => {
    const name = window.prompt("Folder name");
    if (!name) return;
    const folderPath = joinPath(dir, name.trim());
    setBusy(true);
    setMessage("");
    try {
      const res = await fetch(`${API}/fs/mkdir`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ path: folderPath }),
      });
      const json = await res.json();
      if (!res.ok || !json.ok) throw new Error(json.error || `HTTP ${res.status}`);
      await loadDir(dir);
    } catch (err) {
      setMessage(err.message);
    } finally {
      setBusy(false);
    }
  }, [dir, loadDir]);

  const storeCopy = useCallback(async () => {
    const cleanName = filename.trim();
    if (!cleanName) {
      setMessage("Choose a file name");
      return;
    }
    setBusy(true);
    setMessage("Storing");
    try {
      const [markdown, html] = await Promise.all([
        editor.blocksToMarkdownLossy(editor.document),
        editor.blocksToHTMLLossy(editor.document),
      ]);
      const res = await fetch(`${API}/fs/store`, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          path: joinPath(dir, cleanName),
          format,
          blocks: editor.document,
          markdown,
          html,
        }),
      });
      const json = await res.json();
      if (!res.ok || !json.ok) throw new Error(json.error || `HTTP ${res.status}`);
      setMessage(`Stored ${json.path}`);
      onStored?.(json.path);
      await loadDir(dir);
    } catch (err) {
      setMessage(err.message);
    } finally {
      setBusy(false);
    }
  }, [dir, editor, filename, format, loadDir, onStored]);

  return React.createElement("div", { className: "modal-backdrop", role: "dialog", "aria-modal": "true" },
    React.createElement("div", { className: "file-dialog" },
      React.createElement("div", { className: "dialog-head" },
        React.createElement("div", null,
          React.createElement("h2", null, "Store Copy"),
          React.createElement("p", null, dir)
        ),
        React.createElement("button", { type: "button", className: "icon-btn", onClick: onClose, "aria-label": "Close" }, "x")
      ),
      React.createElement("div", { className: "dialog-toolbar" },
        React.createElement("button", { type: "button", className: "btn", onClick: () => loadDir(parentPath(dir)), disabled: busy }, "Up"),
        React.createElement("button", { type: "button", className: "btn", onClick: createFolder, disabled: busy }, "New Folder"),
        React.createElement("button", { type: "button", className: "btn", onClick: () => loadDir(dir), disabled: busy }, "Refresh")
      ),
      React.createElement("div", { className: "file-list" },
        entries.length ? entries.map((entry) =>
          React.createElement("button", {
            key: entry.path,
            type: "button",
            className: `file-row ${entry.kind}`,
            onClick: () => entry.kind === "folder" ? loadDir(entry.path) : setFilename(entry.name),
          },
            React.createElement("span", { className: "file-kind" }, entry.kind === "folder" ? "/" : "."),
            React.createElement("span", { className: "file-name" }, entry.name),
            React.createElement("span", { className: "file-size" }, entry.kind === "folder" ? "" : `${entry.size} B`)
          )
        ) : React.createElement("div", { className: "empty-list" }, busy ? "Loading" : "Empty")
      ),
      React.createElement("div", { className: "store-controls" },
        React.createElement("select", { value: format, onChange: (event) => selectFormat(event.target.value), disabled: busy },
          React.createElement("option", { value: "md" }, "Markdown"),
          React.createElement("option", { value: "txt" }, "Plain Text"),
          React.createElement("option", { value: "html" }, "HTML"),
          React.createElement("option", { value: "json" }, "BlockNote JSON")
        ),
        React.createElement("input", {
          value: filename,
          onChange: (event) => setFilename(event.target.value),
          disabled: busy,
          spellCheck: "false",
        }),
        React.createElement("button", { type: "button", className: "btn primary", onClick: storeCopy, disabled: busy }, "Store Here")
      ),
      React.createElement("p", { className: message.includes("failed") || message.includes("bad") ? "dialog-message error" : "dialog-message" }, message || " ")
    )
  );
}

function EditorSurface({ initialDocument }) {
  const editor = useCreateBlockNote({
    initialContent: initialDocument?.blocks?.length ? initialDocument.blocks : defaultBlocks,
  });
  const [status, setStatus] = useState("ready");
  const [savedAt, setSavedAt] = useState(initialDocument?.updatedAtS ?? 0);
  const [dirty, setDirty] = useState(false);
  const [wordCount, setWordCount] = useState(0);
  const [storeDialogOpen, setStoreDialogOpen] = useState(false);
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
      setStatus("stored");
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
    if (status === "stored") return "Stored on TRUEOS FS";
    if (status === "ready") return savedAt ? "Loaded from TRUEOS FS" : "Ready";
    if (status === "saving") return "Storing";
    return status;
  }, [dirty, savedAt, status]);

  return React.createElement("div", { className: "shell" },
    React.createElement("header", { className: "topbar" },
      React.createElement("div", { className: "brand" },
        React.createElement("div", { className: "brand-mark" }, "T"),
        React.createElement("div", null,
          React.createElement("h1", null, "TRUEOS Text Editor"),
          React.createElement("p", null, `${wordCount} words / ${formatTime(savedAt)} / texteditor/document.json`)
        )
      ),
      React.createElement("div", { className: "actions" },
        React.createElement("span", { className: dirty ? "pill warn" : "pill ok" }, badge),
        React.createElement("button", { type: "button", className: "btn primary", onClick: save }, "Save"),
        React.createElement("button", { type: "button", className: "btn", onClick: () => setStoreDialogOpen(true) }, "Store Copy"),
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
    ),
    storeDialogOpen && React.createElement(StoreDialog, {
      editor,
      onClose: () => setStoreDialogOpen(false),
      onStored: () => setStatus("stored"),
    })
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

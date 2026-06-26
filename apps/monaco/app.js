const API = "/api/monaco";

const els = {
  editor: document.getElementById("editor"),
  language: document.getElementById("language"),
  load: document.getElementById("load"),
  meta: document.getElementById("meta"),
  path: document.getElementById("path"),
  position: document.getElementById("position"),
  save: document.getElementById("save"),
  status: document.getElementById("status"),
  theme: document.getElementById("theme"),
};

const starter = `fn main() {
    println!("hello from TRUEOS Monaco");
}
`;

let monacoEditor = null;
let plainEditor = null;
let dark = false;
let savedValue = starter;

function setStatus(text) {
  els.status.textContent = text;
}

function setMeta(timestampSeconds) {
  els.meta.textContent = timestampSeconds
    ? `stored ${new Date(timestampSeconds * 1000).toLocaleString()}`
    : "not stored yet";
}

function languageForPath(path) {
  const clean = String(path || "").toLowerCase();
  if (clean.endsWith(".rs")) return "rust";
  if (clean.endsWith(".ts")) return "typescript";
  if (clean.endsWith(".js") || clean.endsWith(".mjs")) return "javascript";
  if (clean.endsWith(".json")) return "json";
  if (clean.endsWith(".md")) return "markdown";
  if (clean.endsWith(".html") || clean.endsWith(".htm")) return "html";
  if (clean.endsWith(".css")) return "css";
  return els.language.value || "plaintext";
}

function currentValue() {
  return monacoEditor ? monacoEditor.getValue() : plainEditor.value;
}

function setCurrentValue(value) {
  const next = typeof value === "string" ? value : starter;
  plainEditor.value = next;
  if (monacoEditor) {
    const model = monacoEditor.getModel();
    if (model.getValue() !== next) model.setValue(next);
  }
  updatePosition();
}

function setCurrentLanguage(language) {
  els.language.value = language || languageForPath(els.path.value);
  if (monacoEditor && window.monaco) {
    monaco.editor.setModelLanguage(monacoEditor.getModel(), els.language.value);
  }
}

function markSaved(statusText) {
  savedValue = currentValue();
  setStatus(statusText);
  updatePosition();
}

function updatePosition() {
  const value = currentValue();
  let line = 1;
  let column = 1;

  if (monacoEditor) {
    const position = monacoEditor.getPosition() || { lineNumber: 1, column: 1 };
    line = position.lineNumber;
    column = position.column;
  } else {
    const head = value.slice(0, plainEditor.selectionStart || 0);
    const lines = head.split("\n");
    line = lines.length;
    column = lines[lines.length - 1].length + 1;
  }

  els.position.textContent = `Ln ${line}, Col ${column} · ${value.length} chars`;
}

function noteDirty() {
  setStatus(currentValue() === savedValue ? "saved" : "modified");
  updatePosition();
}

function createPlainEditor() {
  plainEditor = document.createElement("textarea");
  plainEditor.className = "plain-editor";
  plainEditor.spellcheck = false;
  plainEditor.value = starter;
  plainEditor.setAttribute("aria-label", "Editor");
  els.editor.appendChild(plainEditor);
  plainEditor.addEventListener("input", noteDirty);
  plainEditor.addEventListener("keyup", updatePosition);
  plainEditor.addEventListener("click", updatePosition);
  updatePosition();
}

async function loadDocument() {
  const path = els.path.value.trim() || "monaco/main.rs";
  setStatus("loading");
  const res = await fetch(`${API}/document?path=${encodeURIComponent(path)}`, { cache: "no-store" });
  const json = await res.json();
  if (!res.ok || !json.ok) throw new Error(json.error || `HTTP ${res.status}`);
  const doc = json.document;
  els.path.value = doc.path;
  setCurrentLanguage(doc.language || languageForPath(doc.path));
  setCurrentValue(doc.value);
  setMeta(doc.updatedAtS);
  markSaved("loaded");
}

async function saveDocument() {
  const path = els.path.value.trim() || "monaco/main.rs";
  const language = els.language.value || languageForPath(path);
  setStatus("saving");
  const res = await fetch(`${API}/document`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ path, language, value: currentValue() }),
  });
  const json = await res.json();
  if (!res.ok || !json.ok) throw new Error(json.error || `HTTP ${res.status}`);
  els.path.value = json.document.path;
  setCurrentLanguage(json.document.language);
  setMeta(json.document.updatedAtS);
  markSaved(`saved ${json.bytes} B`);
}

function wireControls() {
  els.language.addEventListener("change", () => setCurrentLanguage(els.language.value));
  els.path.addEventListener("change", () => setCurrentLanguage(languageForPath(els.path.value)));
  els.load.addEventListener("click", () => loadDocument().catch((err) => setStatus(err.message)));
  els.save.addEventListener("click", () => saveDocument().catch((err) => setStatus(err.message)));
  els.theme.addEventListener("click", () => {
    dark = !dark;
    document.body.classList.toggle("dark", dark);
    if (monacoEditor && window.monaco) monaco.editor.setTheme(dark ? "vs-dark" : "vs");
    els.theme.textContent = dark ? "Light" : "Dark";
  });
  window.addEventListener("keydown", (event) => {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "s") {
      event.preventDefault();
      saveDocument().catch((err) => setStatus(err.message));
    }
  });
}

function upgradeToMonaco() {
  monacoEditor = monaco.editor.create(els.editor, {
    value: plainEditor.value,
    language: els.language.value || "rust",
    automaticLayout: true,
    fontFamily: "JetBrains Mono, Menlo, Consolas, monospace",
    fontSize: 14,
    lineHeight: 22,
    minimap: { enabled: true },
    renderWhitespace: "selection",
    scrollBeyondLastLine: false,
    theme: dark ? "vs-dark" : "vs",
  });
  plainEditor.hidden = true;
  monacoEditor.onDidChangeCursorPosition(updatePosition);
  monacoEditor.onDidChangeModelContent(noteDirty);
  updatePosition();
  setStatus("Monaco ready");
}

function bootMonaco() {
  if (!window.require || typeof require.config !== "function") {
    setStatus("Monaco loader missing; plain editor active");
    return;
  }

  setStatus("loading Monaco");
  require.config({ paths: { vs: "/monaco/vs" } });
  require(
    ["vs/editor/editor.main"],
    () => {
      try {
        upgradeToMonaco();
      } catch (err) {
        monacoEditor = null;
        plainEditor.hidden = false;
        setStatus(`Monaco failed: ${err.message}; plain editor active`);
      }
    },
    (err) => {
      monacoEditor = null;
      plainEditor.hidden = false;
      setStatus(`Monaco failed: ${err.message || err}; plain editor active`);
    }
  );
}

createPlainEditor();
wireControls();
setMeta(0);
loadDocument()
  .then(bootMonaco)
  .catch((err) => {
    setStatus(err.message);
    bootMonaco();
  });

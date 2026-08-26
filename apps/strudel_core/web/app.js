const API = "/api/strudel";

const els = {
  detail: document.getElementById("detail"),
  instrumentEditor: document.getElementById("instrument-editor"),
  patternEditor: document.getElementById("pattern-editor"),
  position: document.getElementById("position"),
  revision: document.getElementById("revision"),
  runtime: document.getElementById("runtime"),
  status: document.getElementById("status"),
  statusDot: document.getElementById("status-dot"),
  submit: document.getElementById("submit"),
};

const starter = `stack(
  sequence(
    instrument("🎹", { note: "c4", velocity: 104, pan: -0.18 }),
    [instrument("🎸", { note: "g4" }), instrument("🎷", { note: "bb4", pan: 0.28 })],
  ),
  sequence(
    instrument("🥁", { note: 36, velocity: 112 }),
    null,
    instrument("🎚️", { note: "ab1", velocity: 106 }),
    instrument("🪘", { note: 48, velocity: 106 }),
  ),
)`;

let monacoEditor = null;
let monacoInstrumentEditor = null;
let plainEditor = null;
let plainInstrumentEditor = null;
let lastCommittedSource = "";
let submitting = false;

function setStatus(kind, title, detail) {
  els.statusDot.className = `status-dot ${kind}`;
  els.status.textContent = title;
  els.detail.textContent = detail || "";
}

function currentValue() {
  return monacoEditor ? monacoEditor.getValue() : plainEditor.value;
}

function setCurrentValue(value) {
  const next = typeof value === "string" && value.trim() ? value : starter;
  plainEditor.value = next;
  if (monacoEditor) {
    const model = monacoEditor.getModel();
    if (model.getValue() !== next) model.setValue(next);
  }
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

function clearMarkers() {
  if (monacoEditor && window.monaco) {
    monaco.editor.setModelMarkers(monacoEditor.getModel(), "strudel-core", []);
  }
}

function markError(message) {
  if (!monacoEditor || !window.monaco) return;
  const model = monacoEditor.getModel();
  const text = String(message || "Pattern submission failed");
  const match = text.match(/:(\d+)(?::(\d+))?/);
  const wrappedLine = match ? Number(match[1]) : 2;
  const lineNumber = Math.max(1, Math.min(model.getLineCount(), wrappedLine - 1));
  const column = match && match[2] ? Math.max(1, Number(match[2])) : 1;
  monaco.editor.setModelMarkers(model, "strudel-core", [
    {
      severity: monaco.MarkerSeverity.Error,
      message: text,
      startLineNumber: lineNumber,
      startColumn: column,
      endLineNumber: lineNumber,
      endColumn: Math.max(column + 1, model.getLineMaxColumn(lineNumber)),
    },
  ]);
}

function runtimeLabel(state) {
  const runtime = state.runtime || {};
  const source = runtime.source || "pattern";
  const version = runtime.version ? ` ${runtime.version}` : "";
  return `${source}${version} · ${state.sampleRateHz} Hz · cps ${state.cpsNumerator}/${state.cpsDenominator}`;
}

function applyState(state, replaceEditor) {
  if (!state) return;
  if (replaceEditor) {
    lastCommittedSource = state.source || starter;
    setCurrentValue(lastCommittedSource);
  }
  els.revision.textContent = `revision ${state.revision}`;
  els.runtime.textContent = runtimeLabel(state);
}

function noteEdited() {
  clearMarkers();
  if (!submitting) {
    const dirty = currentValue() !== lastCommittedSource;
    setStatus(
      dirty ? "idle" : "ok",
      dirty ? "edited · audio unchanged" : "committed",
      dirty
        ? "Submit the expression to replace the host pattern. The browser does not synthesize audio."
        : "The host-owned QuickJS pattern is active.",
    );
  }
  updatePosition();
}

function createPlainEditor() {
  plainEditor = document.createElement("textarea");
  plainEditor.className = "plain-editor";
  plainEditor.spellcheck = false;
  plainEditor.value = starter;
  plainEditor.setAttribute("aria-label", "JavaScript pattern expression");
  els.patternEditor.appendChild(plainEditor);
  plainEditor.addEventListener("input", noteEdited);
  plainEditor.addEventListener("keyup", updatePosition);
  plainEditor.addEventListener("click", updatePosition);
  updatePosition();
}

function instrumentCatalogText() {
  const catalogRoot = window.__TRUEOS_INSTRUMENT_CATALOG;
  const catalog = catalogRoot && Array.isArray(catalogRoot.entries) ? catalogRoot.entries : [];
  if (!catalog.length) {
    return `// TRUEOS instruments\n// Catalog unavailable; patterns still run normally.\n`;
  }
  const lines = [
  ];
  for (const entry of catalog) {
    lines.push(String(entry.snippet || ""));
  }
  return lines.join("\n");
}

function createPlainInstrumentEditor() {
  plainInstrumentEditor = document.createElement("textarea");
  plainInstrumentEditor.className = "plain-editor instrument-source";
  plainInstrumentEditor.spellcheck = false;
  plainInstrumentEditor.readOnly = true;
  plainInstrumentEditor.value = instrumentCatalogText();
  plainInstrumentEditor.setAttribute("aria-label", "Read-only TRUEOS instrument notation");
  els.instrumentEditor.appendChild(plainInstrumentEditor);
}

async function readJson(response) {
  const text = await response.text();
  try {
    return text ? JSON.parse(text) : {};
  } catch (_) {
    throw new Error(text || `HTTP ${response.status}`);
  }
}

async function loadState() {
  setStatus("busy", "connecting", "Reading the active host pattern…");
  const response = await fetch(`${API}/state`, { cache: "no-store" });
  const json = await readJson(response);
  if (!response.ok || !json.ok) throw new Error(json.error || `HTTP ${response.status}`);
  applyState(json.state, true);
  setStatus("ok", "host engine ready", "Edit one JavaScript Pattern expression and submit with Ctrl+Enter.");
}

async function submitPattern() {
  if (submitting) return;
  submitting = true;
  els.submit.disabled = true;
  clearMarkers();
  setStatus("busy", "submitting", "QuickJS is validating the expression on the TRUEOS host…");

  try {
    const source = currentValue();
    const response = await fetch(`${API}/submit`, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ source }),
    });
    const json = await readJson(response);
    if (!response.ok || !json.ok) {
      if (json.state) applyState(json.state, false);
      throw new Error(json.error || `HTTP ${response.status}`);
    }

    lastCommittedSource = source;
    applyState(json.state, false);
    setStatus(
      "ok",
      `revision ${json.state.revision} committed`,
      `The host accepted ${source.length} characters. Existing PCM lookahead drains without a browser-side audio reset.`,
    );
  } catch (error) {
    const message = error && error.message ? error.message : String(error);
    markError(message);
    setStatus("error", "pattern rejected · previous audio retained", message);
  } finally {
    submitting = false;
    els.submit.disabled = false;
    updatePosition();
  }
}

function upgradeToMonaco() {
  monacoEditor = monaco.editor.create(els.patternEditor, {
    value: plainEditor.value,
    language: "javascript",
    automaticLayout: true,
    fontFamily: "JetBrains Mono, Menlo, Consolas, monospace",
    fontSize: 14,
    lineHeight: 22,
    minimap: { enabled: false },
    renderWhitespace: "selection",
    scrollBeyondLastLine: false,
    tabSize: 2,
    theme: "vs-dark",
  });
  plainEditor.hidden = true;
  monacoEditor.onDidChangeCursorPosition(updatePosition);
  monacoEditor.onDidChangeModelContent(noteEdited);
  monacoEditor.addAction({
    id: "strudel-core-submit",
    label: "Submit Strudel Pattern",
    keybindings: [monaco.KeyMod.CtrlCmd | monaco.KeyCode.Enter],
    run: submitPattern,
  });
  monacoInstrumentEditor = monaco.editor.create(els.instrumentEditor, {
    value: plainInstrumentEditor.value,
    language: "javascript",
    readOnly: true,
    domReadOnly: true,
    automaticLayout: true,
    fontFamily: "JetBrains Mono, Menlo, Consolas, monospace",
    fontSize: 13,
    lineHeight: 21,
    minimap: { enabled: false },
    folding: true,
    glyphMargin: false,
    lineNumbersMinChars: 3,
    renderWhitespace: "none",
    scrollBeyondLastLine: false,
    theme: "vs-dark",
    wordWrap: "on",
  });
  plainInstrumentEditor.hidden = true;
  updatePosition();
}

function bootMonaco() {
  if (!window.require || typeof require.config !== "function") {
    setStatus("idle", "plain editor active", "Monaco loader is unavailable; Ctrl+Enter still submits.");
    return;
  }

  require.config({ paths: { vs: "/monaco/vs" } });
  require(
    ["vs/editor/editor.main"],
    () => {
      try {
        upgradeToMonaco();
      } catch (error) {
        monacoEditor = null;
        monacoInstrumentEditor = null;
        plainEditor.hidden = false;
        plainInstrumentEditor.hidden = false;
        setStatus("error", "Monaco failed; plain editor active", error.message || String(error));
      }
    },
    (error) => {
      monacoEditor = null;
      monacoInstrumentEditor = null;
      plainEditor.hidden = false;
      plainInstrumentEditor.hidden = false;
      setStatus("error", "Monaco failed; plain editor active", error.message || String(error));
    },
  );
}

els.submit.addEventListener("click", submitPattern);
window.addEventListener("keydown", (event) => {
  if ((event.ctrlKey || event.metaKey) && event.key === "Enter") {
    event.preventDefault();
    submitPattern();
  }
});

createPlainEditor();
createPlainInstrumentEditor();
loadState()
  .catch((error) => {
    setCurrentValue(starter);
    setStatus("error", "host engine unavailable", error.message || String(error));
  })
  .finally(bootMonaco);

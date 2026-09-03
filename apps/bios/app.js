(() => {
  "use strict";

  const $ = (id) => document.getElementById(id);
  const state = { schema: null, formsetIndex: 0, formId: null, selectedQuestionKey: null };
  const els = {
    app: $("app"), apiChip: $("api-chip"), search: $("search"), searchResults: $("search-results"),
    formsetCount: $("formset-count"), formsetNav: $("formset-nav"), breadcrumbs: $("breadcrumbs"),
    formTitle: $("form-title"), formHelp: $("form-help"), formMeta: $("form-meta"), notice: $("notice"),
    formContent: $("form-content"), inspectorBody: $("inspector-body"), footerState: $("footer-state"), toast: $("toast"),
  };

  function escapeHtml(value) {
    return String(value ?? "").replaceAll("&", "&amp;").replaceAll("<", "&lt;")
      .replaceAll(">", "&gt;").replaceAll('"', "&quot;").replaceAll("'", "&#039;");
  }

  function hex16(value) {
    const n = Number(value);
    return Number.isFinite(n) ? `0x${n.toString(16).toUpperCase().padStart(4, "0")}` : "—";
  }

  function maybe(obj, camel, snake) { return obj ? (obj[camel] ?? obj[snake]) : undefined; }
  function formsets() { return Array.isArray(state.schema?.formsets) ? state.schema.formsets : []; }
  function formsetIndexOf(fs, fallback) {
    const value = maybe(fs, "index", "formset_index");
    return Number.isFinite(Number(value)) ? Number(value) : fallback;
  }
  function formsFor(fs) { return Array.isArray(fs?.forms) ? fs.forms : []; }
  function formIdOf(form) { return Number(maybe(form, "formId", "form_id")); }
  function findFormset(index) { return formsets().find((fs, i) => formsetIndexOf(fs, i) === Number(index)) ?? null; }
  function findForm(fs, id) { return formsFor(fs).find((form) => formIdOf(form) === Number(id)) ?? null; }
  function currentFormset() { return findFormset(state.formsetIndex) ?? formsets()[0] ?? null; }
  function currentForm() {
    const fs = currentFormset();
    return fs ? (findForm(fs, state.formId) ?? formsFor(fs)[0] ?? null) : null;
  }

  function presentationNodes() {
    const nodes = state.schema?.presentation?.nodes ?? state.schema?.presentationNodes ??
      state.schema?.orderedIfrNodes ?? state.schema?.ifrNodes;
    return Array.isArray(nodes) ? nodes : [];
  }

  function nodesForForm(fsIndex, formId) {
    return presentationNodes().filter((node) =>
      Number(maybe(node, "formsetIndex", "formset_index")) === Number(fsIndex) &&
      Number(maybe(node, "formId", "form_id")) === Number(formId)
    ).sort((a, b) => Number(maybe(a, "sourceOffset", "source_offset") ?? 0) - Number(maybe(b, "sourceOffset", "source_offset") ?? 0));
  }

  function showToast(message) {
    els.toast.textContent = message;
    els.toast.hidden = false;
    clearTimeout(showToast.timer);
    showToast.timer = setTimeout(() => { els.toast.hidden = true; }, 2800);
  }

  function selectForm(fsIndex, formId) {
    state.formsetIndex = Number(fsIndex);
    state.formId = Number(formId);
    state.selectedQuestionKey = null;
    render();
  }

  function selectFirstForm() {
    const fs = currentFormset();
    const forms = formsFor(fs);
    state.formId = forms.length ? formIdOf(forms[0]) : null;
  }

  function renderSidebar() {
    const sets = formsets();
    els.formsetCount.textContent = String(sets.length);
    els.formsetNav.innerHTML = "";
    sets.forEach((fs, i) => {
      const index = formsetIndexOf(fs, i);
      const group = document.createElement("div");
      group.className = `formset-group${index === state.formsetIndex ? " active" : ""}`;
      const button = document.createElement("button");
      button.type = "button";
      button.className = "formset-button";
      button.innerHTML = `<strong>${escapeHtml(fs.title || `Form set ${index}`)}</strong><span>${formsFor(fs).length}</span>`;
      button.addEventListener("click", () => {
        state.formsetIndex = index;
        const first = formsFor(fs)[0];
        state.formId = first ? formIdOf(first) : null;
        state.selectedQuestionKey = null;
        render();
      });
      const list = document.createElement("div");
      list.className = "form-list";
      formsFor(fs).forEach((form) => {
        const id = formIdOf(form);
        const formButton = document.createElement("button");
        formButton.type = "button";
        formButton.className = `form-button${index === state.formsetIndex && id === state.formId ? " active" : ""}`;
        formButton.textContent = form.title || `Form ${hex16(id)}`;
        formButton.addEventListener("click", () => selectForm(index, id));
        list.appendChild(formButton);
      });
      group.append(button, list);
      els.formsetNav.appendChild(group);
    });
  }

  function formQuestions(form) { return Array.isArray(form?.questions) ? form.questions : []; }
  function questionKey(question) {
    return question?.recordKey ?? question?.key ??
      `${state.formsetIndex}:${state.formId}:${maybe(question, "questionId", "question_id")}:${maybe(question, "sourceOffset", "source_offset")}`;
  }

  function findCanonicalQuestion(form, node) {
    const d = node?.details || {};
    const qid = Number(maybe(d, "questionId", "question_id"));
    const offset = Number(maybe(node, "sourceOffset", "source_offset"));
    const questions = formQuestions(form);
    return questions.find((q) => Number(maybe(q, "sourceOffset", "source_offset")) === offset && Number(maybe(q, "questionId", "question_id")) === qid) ??
      questions.find((q) => Number(maybe(q, "questionId", "question_id")) === qid) ?? null;
  }

  function pseudoQuestion(node) {
    const d = node?.details || {};
    return {
      prompt: d.prompt, help: d.help, questionId: maybe(d, "questionId", "question_id"),
      sourceOffset: maybe(node, "sourceOffset", "source_offset"), kind: maybe(node, "opcodeName", "opcode_name"),
      options: [], defaults: [], storage: { backend: "presentation-only", validated: false },
      policy: { trueosWrite: "locked", callback: false, requiresReset: false, firmwareReadOnly: false },
      currentValue: "captured-redacted-not-decoded-in-this-cycle",
    };
  }

  function kindOf(question) { return String(question?.kind ?? "unknown"); }

  function renderControl(question) {
    const kind = kindOf(question);
    const options = Array.isArray(question?.options) ? question.options : [];
    const numeric = question?.numericRange ?? question?.numeric ?? null;
    const stringLimits = question?.stringLimits ?? question?.string_limits ?? null;
    if (kind === "one-of") {
      const labels = options.map((o) => o.text).filter(Boolean);
      return `<div class="locked-value"><span>Current value redacted</span><span class="chip">${labels.length} options</span></div>`;
    }
    if (kind === "checkbox") {
      return `<div class="locked-value"><span>Current value redacted</span><span class="unknown-toggle" aria-label="unknown current value"></span></div>`;
    }
    if (kind === "numeric") {
      const range = numeric ? `${numeric.minimum ?? "?"} – ${numeric.maximum ?? "?"}${numeric.step ? ` / step ${numeric.step}` : ""}` : "numeric value";
      return `<input class="locked-input" disabled placeholder="${escapeHtml(range)}">`;
    }
    if (kind === "string") {
      const hint = stringLimits?.maximumChars ?? stringLimits?.maximum_chars;
      return `<input class="locked-input" disabled placeholder="Value redacted${hint ? ` · max ${escapeHtml(hint)} chars` : ""}">`;
    }
    if (kind === "password") return `<input class="locked-input" type="password" disabled placeholder="Firmware secret not exposed">`;
    if (kind === "action") return `<button class="locked-button" type="button" disabled>Action locked</button>`;
    return `<div class="locked-value"><span>Read-only metadata</span><span class="chip">${escapeHtml(kind)}</span></div>`;
  }

  function renderQuestion(question) {
    const key = questionKey(question);
    const qid = maybe(question, "questionId", "question_id");
    const row = document.createElement("article");
    row.className = `question-row${key === state.selectedQuestionKey ? " selected" : ""}`;
    row.tabIndex = 0;
    row.innerHTML = `<div><div class="question-title"><strong>${escapeHtml(question.prompt || `(unnamed ${kindOf(question)} question)`)}</strong><span class="chip">${escapeHtml(kindOf(question))}</span>${question?.policy?.requiresReset ? `<span class="chip chip-warn">RESET</span>` : ""}</div><div class="question-help">${escapeHtml(question.help || "No help text supplied by firmware.")}</div><div class="question-id">${escapeHtml(hex16(qid))} · source ${escapeHtml(maybe(question, "sourceOffset", "source_offset") ?? "—")}</div></div><div class="control-shell">${renderControl(question)}</div>`;
    const select = () => {
      state.selectedQuestionKey = key;
      renderInspector(question);
      document.querySelectorAll(".question-row.selected").forEach((el) => el.classList.remove("selected"));
      row.classList.add("selected");
    };
    row.addEventListener("click", select);
    row.addEventListener("keydown", (event) => {
      if (event.key === "Enter" || event.key === " ") { event.preventDefault(); select(); }
    });
    return row;
  }

  function renderReference(node, fs) {
    const d = node.details || {};
    const targetForm = Number(maybe(d, "targetFormId", "target_form_id"));
    const targetGuid = maybe(d, "targetFormsetGuid", "target_formset_guid");
    const button = document.createElement("button");
    button.type = "button";
    button.className = "nav-row";
    button.innerHTML = `<div><strong>${escapeHtml(d.prompt || "Open firmware form")}</strong><span>${escapeHtml(d.help || `Navigate to form ${hex16(targetForm)}`)}</span></div><div class="nav-arrow" aria-hidden="true">→</div>`;
    button.addEventListener("click", () => {
      let targetFs = fs;
      if (targetGuid) targetFs = formsets().find((candidate) => candidate.guid === targetGuid) ?? fs;
      const targetFsIndex = formsetIndexOf(targetFs, state.formsetIndex);
      if (findForm(targetFs, targetForm)) selectForm(targetFsIndex, targetForm);
      else showToast(`Firmware reference target ${hex16(targetForm)} is not present in this capture.`);
    });
    return button;
  }

  function renderInfoNode(node) {
    const d = node.details || {};
    const item = document.createElement("div");
    item.className = "info-row";
    const one = d.prompt || "";
    const two = d.text_two ?? d.textTwo ?? "";
    const help = d.help || "";
    item.innerHTML = `${one ? `<strong>${escapeHtml(one)}</strong>` : ""}${two ? `<span>${escapeHtml(two)}</span>` : ""}${help && help !== two ? `<span>${escapeHtml(help)}</span>` : ""}`;
    return item;
  }

  function emptyForm(title, detail) {
    const div = document.createElement("div");
    div.className = "empty-form";
    div.innerHTML = `<strong>${escapeHtml(title)}</strong><span>${escapeHtml(detail)}</span>`;
    return div;
  }

  function renderOrderedForm(fs, form, nodes) {
    const fragment = document.createDocumentFragment();
    let visible = 0;
    nodes.forEach((node) => {
      const name = String(maybe(node, "opcodeName", "opcode_name") ?? "");
      if (name === "subtitle") {
        const prompt = node.details?.prompt;
        if (prompt) { const h = document.createElement("h2"); h.className = "section-title"; h.textContent = prompt; fragment.appendChild(h); visible++; }
        return;
      }
      if (name === "text") { fragment.appendChild(renderInfoNode(node)); visible++; return; }
      if (name === "ref") { fragment.appendChild(renderReference(node, fs)); visible++; return; }
      if (["one-of", "checkbox", "numeric", "string", "password", "action"].includes(name)) {
        fragment.appendChild(renderQuestion(findCanonicalQuestion(form, node) ?? pseudoQuestion(node))); visible++; return;
      }
      if (name === "guid-extension" && (node.details?.extend_name ?? node.details?.extendName) === "label") {
        const anchor = document.createElement("span"); anchor.hidden = true; anchor.id = `ifr-label-${node.details?.value ?? "unknown"}`; fragment.appendChild(anchor);
      }
    });
    if (!visible) fragment.appendChild(emptyForm("No visible statements in this captured form", "The form contains only structural labels or metadata. Nothing is invented from loose strings."));
    els.formContent.appendChild(fragment);
  }

  function renderFallbackForm(form) {
    const questions = formQuestions(form);
    if (questions.length) { questions.forEach((q) => els.formContent.appendChild(renderQuestion(q))); return; }
    els.formContent.appendChild(emptyForm("Presentation-only form", "The live v1 Blueprint snapshot exposes validated questions but not source-order REF/TEXT/SUBTITLE nodes. This UI will render them automatically when the ordered presentation snapshot is exported."));
  }

  function renderInspector(question) {
    const storage = question?.storage || {};
    const policy = question?.policy || {};
    const options = Array.isArray(question?.options) ? question.options : [];
    const defaults = Array.isArray(question?.defaults) ? question.defaults : [];
    const visibility = question?.visibility ?? question?.conditions ?? [];
    const optionText = options.length ? options.map((o) => `${o.text ?? "?"} = ${o.value?.display ?? o.value?.unsigned ?? "?"}`).join(" · ") : "none";
    const defaultText = defaults.length ? defaults.map((d) => `${d.label ?? d.defaultId ?? "default"}: ${d.value?.display ?? d.value?.unsigned ?? "?"}`).join(" · ") : "none";
    els.inspectorBody.innerHTML = `<h2 class="inspect-title">${escapeHtml(question.prompt || "Firmware question")}</h2><p class="inspect-help">${escapeHtml(question.help || "No firmware help text.")}</p><div class="inspect-section"><h3>Question</h3><dl class="kv"><dt>ID</dt><dd class="code">${escapeHtml(hex16(maybe(question, "questionId", "question_id")))}</dd><dt>Kind</dt><dd>${escapeHtml(kindOf(question))}</dd><dt>Current</dt><dd>redacted / not decoded</dd><dt>Conditions</dt><dd>${escapeHtml(Array.isArray(visibility) ? visibility.length : 0)}</dd></dl></div><div class="inspect-section"><h3>Storage</h3><dl class="kv"><dt>Backend</dt><dd>${escapeHtml(storage.backend ?? "none")}</dd><dt>Variable</dt><dd class="code">${escapeHtml(storage.variable ?? "—")}</dd><dt>Varstore</dt><dd class="code">${escapeHtml(hex16(storage.varstoreId ?? storage.varstore_id))}</dd><dt>Offset</dt><dd class="code">${escapeHtml(storage.offset ?? "—")}</dd><dt>Width</dt><dd>${escapeHtml(storage.width ?? "—")}</dd><dt>Validated</dt><dd>${storage.validated === true ? "yes" : storage.validated === false ? "no" : "—"}</dd></dl></div><div class="inspect-section"><h3>Policy</h3><dl class="kv"><dt>TRUEOS write</dt><dd>${escapeHtml(policy.trueosWrite ?? policy.trueos_write ?? "locked")}</dd><dt>Firmware RO</dt><dd>${policy.firmwareReadOnly ?? policy.firmware_ro ? "yes" : "no"}</dd><dt>Callback</dt><dd>${policy.callback ? "yes (not invoked)" : "no"}</dd><dt>Reset</dt><dd>${policy.requiresReset ?? policy.requires_reset ? "required by firmware" : "no"}</dd></dl></div><div class="inspect-section"><h3>Options</h3><p class="muted">${escapeHtml(optionText)}</p></div><div class="inspect-section"><h3>Defaults</h3><p class="muted">${escapeHtml(defaultText)}</p></div>`;
  }

  function renderForm() {
    const fs = currentFormset(); const form = currentForm();
    els.formContent.innerHTML = ""; els.notice.hidden = true;
    if (!fs || !form) {
      els.formTitle.textContent = "No captured BIOS form"; els.formHelp.textContent = ""; els.breadcrumbs.textContent = ""; els.formMeta.innerHTML = "";
      els.formContent.appendChild(emptyForm("No forms available", "The kernel snapshot did not expose a form for this form set.")); return;
    }
    els.breadcrumbs.textContent = `${fs.title || "Firmware"}  /  Form ${hex16(formIdOf(form))}`;
    els.formTitle.textContent = form.title || fs.title || `Form ${hex16(formIdOf(form))}`;
    els.formHelp.textContent = form.help || fs.help || "";
    const fsIndex = formsetIndexOf(fs, state.formsetIndex); const nodes = nodesForForm(fsIndex, formIdOf(form));
    els.formMeta.innerHTML = `<span class="chip">${escapeHtml(fs.guid || "no-guid")}</span><span class="chip">${formQuestions(form).length} questions</span><span class="chip">${nodes.length ? `${nodes.length} IFR nodes` : "schema v1"}</span>`;
    if (nodes.length) renderOrderedForm(fs, form, nodes); else renderFallbackForm(form);
  }

  function searchEntries() {
    const entries = [];
    formsets().forEach((fs, i) => {
      const fsIndex = formsetIndexOf(fs, i);
      formsFor(fs).forEach((form) => {
        const formId = formIdOf(form); const base = { fsIndex, formId, context: `${fs.title || "Firmware"} / ${form.title || hex16(formId)}` };
        if (form.title) entries.push({ ...base, label: form.title, type: "form" });
        formQuestions(form).forEach((question) => { const label = question.prompt || question.help; if (label) entries.push({ ...base, label, type: kindOf(question), question }); });
        nodesForForm(fsIndex, formId).forEach((node) => {
          const name = String(maybe(node, "opcodeName", "opcode_name") ?? "");
          if (!["subtitle", "text", "ref"].includes(name)) return;
          const d = node.details || {}; const label = d.prompt || d.text_two || d.textTwo || d.help;
          if (label) entries.push({ ...base, label, type: name });
        });
      });
    });
    const seen = new Set();
    return entries.filter((entry) => { const key = `${entry.fsIndex}:${entry.formId}:${entry.type}:${entry.label}`; if (seen.has(key)) return false; seen.add(key); return true; });
  }

  function renderSearch() {
    const query = els.search.value.trim().toLocaleLowerCase();
    if (!query) { els.searchResults.hidden = true; els.searchResults.innerHTML = ""; return; }
    const results = searchEntries().filter((entry) => `${entry.label} ${entry.context} ${entry.type}`.toLocaleLowerCase().includes(query)).slice(0, 40);
    els.searchResults.innerHTML = ""; els.searchResults.hidden = false;
    if (!results.length) { const none = document.createElement("div"); none.className = "search-none"; none.textContent = "question_match=none"; els.searchResults.appendChild(none); return; }
    results.forEach((entry) => {
      const button = document.createElement("button"); button.type = "button"; button.className = "search-result";
      button.innerHTML = `<strong>${escapeHtml(entry.label)}</strong><span>${escapeHtml(entry.type)} · ${escapeHtml(entry.context)}</span>`;
      button.addEventListener("click", () => { els.search.value = ""; els.searchResults.hidden = true; selectForm(entry.fsIndex, entry.formId); if (entry.question) { state.selectedQuestionKey = questionKey(entry.question); renderInspector(entry.question); } });
      els.searchResults.appendChild(button);
    });
  }

  function renderStatus() {
    const schema = state.schema || {};
    els.apiChip.textContent = schema.api || "BIOS schema";
    const forms = formsets().reduce((n, fs) => n + formsFor(fs).length, 0);
    els.footerState.textContent = `${schema.state || "unknown"} · ${schema.stats?.formsets ?? formsets().length} formsets · ${schema.stats?.forms ?? forms} forms · ${schema.stats?.questions ?? "?"} questions · active_write_path=none`;
  }

  function render() { if (state.formId == null) selectFirstForm(); renderSidebar(); renderForm(); renderStatus(); }

  async function loadSchema() {
    try {
      const response = await fetch("/api/bios/schema", { method: "GET", headers: { Accept: "application/json" }, cache: "no-store" });
      if (!response.ok) throw new Error(`HTTP ${response.status}`);
      const schema = await response.json();
      if (schema.readOnly !== true || schema.activeWritePath !== "none") throw new Error("kernel snapshot did not advertise the locked read-only boundary");
      state.schema = schema;
      const first = formsets()[0]; state.formsetIndex = first ? formsetIndexOf(first, 0) : 0; state.formId = formsFor(first)[0] ? formIdOf(formsFor(first)[0]) : null;
      els.app.setAttribute("aria-busy", "false"); render();
    } catch (error) {
      els.apiChip.textContent = "schema unavailable"; els.formTitle.textContent = "BIOS schema unavailable"; els.formHelp.textContent = String(error);
      els.footerState.textContent = "The localhost server is running, but the kernel BIOS snapshot could not be read.";
      els.formContent.innerHTML = ""; els.formContent.appendChild(emptyForm("No firmware data", "The browser UI remains read-only; retry after the TRUEOS BIOS snapshot ABI is available."));
      els.app.setAttribute("aria-busy", "false");
    }
  }

  els.search.addEventListener("input", renderSearch);
  els.search.addEventListener("keydown", (event) => { if (event.key === "Escape") { els.search.value = ""; renderSearch(); } });
  document.addEventListener("click", (event) => { if (!els.searchResults.hidden && !event.target.closest(".search-box") && !event.target.closest(".search-results")) els.searchResults.hidden = true; });
  window.addEventListener("keydown", (event) => {
    const saveKey = event.key === "F10" || ((event.ctrlKey || event.metaKey) && event.key.toLocaleLowerCase() === "s");
    if (saveKey) { event.preventDefault(); event.stopPropagation(); showToast("Save is intentionally unavailable. TRUEOS exposes no BIOS write path."); }
  }, { capture: true });

  loadSchema();
})();

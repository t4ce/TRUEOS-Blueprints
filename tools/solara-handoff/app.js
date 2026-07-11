const scenarios = {
  coherent: {
    title: "Coherent frame",
    description: "A paint update references live resources and follows the GPU's accepted epoch.",
    severity: "ACCEPT",
    className: "accepted",
    packet: { tx: 1043, base: 1042, ops: 4, crc: "9AE2", damage: "48 × 32" },
    ops: [
      "UpdatePrimitive(card.17, color)",
      "SetScrollOffset(main, 240)",
      "TouchResource(atlas@88)",
      "Present(Vsync::Next)",
    ],
    events: [
      ["CHECK", "ok", "base epoch 1042 matches accepted state"],
      ["STAGE", "info", "4 operations staged; atlas@88 is resident"],
      ["PUBLISH", "ok", "epoch 1043 became atomically visible"],
      ["PRESENT", "ok", "damage [48,32] submitted behind fence 732"],
    ],
  },
  stale: {
    title: "Stale epoch",
    description: "A delayed producer submits a delta based on state the GPU has already replaced.",
    severity: "REJECT",
    className: "rejected",
    packet: { tx: 1042, base: 1041, ops: 2, crc: "1B7C", damage: "64 × 20" },
    ops: [
      "UpdatePrimitive(label.9, text)",
      "Present(Vsync::Next)",
    ],
    events: [
      ["CHECK", "error", "expected base 1042; received 1041"],
      ["REJECT", "error", "StaleEpoch { accepted: 1042 }"],
      ["SIGNAL", "info", "producer asked to rebase or send snapshot"],
      ["STATE", "ok", "retained scene remains unchanged at 1042"],
    ],
  },
  lifetime: {
    title: "Early release",
    description: "CPU drops its texture handle while an already-published frame can still sample it.",
    severity: "DEFER",
    className: "warned",
    packet: { tx: 1043, base: 1042, ops: 2, crc: "7F04", damage: "0 × 0" },
    ops: [
      "ReleaseResource(image@88)",
      "Present(Vsync::Next)",
    ],
    events: [
      ["CHECK", "warn", "image@88 is referenced by in-flight fence 731"],
      ["RETIRE", "info", "release queued after fence 731"],
      ["PUBLISH", "ok", "logical ownership ended at epoch 1043"],
      ["RECLAIM", "ok", "physical memory freed after fence signal"],
    ],
  },
  damage: {
    title: "Bad damage",
    description: "The transaction changes pixels outside the producer's declared dirty region.",
    severity: "WARN",
    className: "warned",
    packet: { tx: 1043, base: 1042, ops: 3, crc: "CC18", damage: "12 × 12" },
    ops: [
      "UpdatePrimitive(panel.2, shadow)",
      "DeclareDamage([48, 96, 12, 12])",
      "Present(Vsync::Next)",
    ],
    events: [
      ["CHECK", "warn", "computed change exceeds declared damage"],
      ["PROMOTE", "info", "damage expanded to surface bounds"],
      ["PUBLISH", "ok", "scene accepted; visual correctness preserved"],
      ["DIAG", "warn", "DamageUnderflow emitted for producer panel.2"],
    ],
  },
};

const byId = (id) => document.getElementById(id);
const ui = {
  base: byId("base-id"), commit: byId("commit-button"), commitLabel: byId("commit-label"),
  cpuDamage: byId("cpu-damage"), cpuEpoch: byId("cpu-epoch"), crc: byId("crc-value"),
  damage: byId("damage-value"), gate: byId("protocol-gate"), gateStatus: byId("gate-status"),
  gpuDamage: byId("gpu-damage"), gpuEpoch: byId("gpu-epoch"), logCount: byId("log-count"),
  logRows: byId("log-rows"), ops: byId("ops-count"), opList: byId("op-list"),
  packet: byId("packet"), reset: byId("reset-button"), resource: byId("resource-gen"),
  resident: byId("resident-value"), scenarioDescription: byId("scenario-description"),
  scenarioTitle: byId("scenario-title"), severity: byId("severity"), tx: byId("tx-id"),
  fence: byId("fence-value"),
};

let selected = "coherent";
let eventCount = 0;
let busy = false;
let gpuEpoch = 1042;
let fence = 731;

function setScenario(name) {
  if (busy) return;
  selected = name;
  const scenario = scenarios[name];
  document.querySelectorAll(".scenario").forEach((button) => {
    const active = button.dataset.scenario === name;
    button.classList.toggle("active", active);
    button.setAttribute("aria-pressed", String(active));
  });
  ui.scenarioTitle.textContent = scenario.title;
  ui.scenarioDescription.textContent = scenario.description;
  ui.severity.textContent = scenario.severity;
  ui.severity.className = `severity ${scenario.className}`;
  const baseEpoch = name === "stale" ? gpuEpoch - 1 : gpuEpoch;
  ui.tx.textContent = `#${baseEpoch + 1}`;
  ui.base.textContent = `#${baseEpoch}`;
  ui.ops.textContent = scenario.packet.ops;
  ui.crc.textContent = scenario.packet.crc;
  ui.damage.textContent = scenario.packet.damage;
  ui.opList.innerHTML = scenario.ops.map((op) => `<li><code>${op}</code></li>`).join("");
  ui.gateStatus.textContent = "READY";
  ui.gateStatus.style.color = "";
  ui.commitLabel.textContent = scenario.className === "rejected" ? "Test transaction" : "Commit transaction";

  const size = name === "damage" ? [12, 12] : name === "lifetime" ? [0, 0] : name === "stale" ? [64, 20] : [48, 32];
  [ui.cpuDamage, ui.gpuDamage].forEach((box) => {
    box.style.width = `${size[0]}px`;
    box.style.height = `${size[1]}px`;
    box.style.opacity = size[0] ? "" : "0";
  });
}

function appendEvent(kind, tone, message, index) {
  if (ui.logRows.querySelector(".empty-log")) ui.logRows.innerHTML = "";
  eventCount += 1;
  const row = document.createElement("div");
  row.className = `log-row ${tone}`;
  row.style.animationDelay = `${index * 45}ms`;
  const now = String(eventCount).padStart(3, "0");
  row.innerHTML = `<time>T+${now}</time><b>${kind}</b><p>${message}</p>`;
  ui.logRows.appendChild(row);
  ui.logRows.scrollTop = ui.logRows.scrollHeight;
  ui.logCount.textContent = `${eventCount} event${eventCount === 1 ? "" : "s"}`;
}

function finishCommit(scenario) {
  const rejected = scenario.className === "rejected";
  if (!rejected) {
    gpuEpoch += 1;
    ui.gpuEpoch.textContent = gpuEpoch;
    ui.cpuEpoch.textContent = gpuEpoch;
    fence += 1;
    ui.fence.textContent = fence;
  }
  ui.gateStatus.textContent = rejected ? "REJECTED" : scenario.severity === "WARN" ? "PROMOTED" : "ACCEPTED";
  ui.gateStatus.style.color = rejected ? "var(--orange)" : scenario.className === "warned" ? "#ffd06c" : "var(--mint)";
  ui.commit.disabled = false;
  ui.commitLabel.textContent = rejected ? "Rejected safely" : "Committed";
  busy = false;
}

function commit() {
  if (busy) return;
  busy = true;
  const scenario = scenarios[selected];
  const txBase = selected === "stale" ? gpuEpoch - 1 : gpuEpoch;
  const txNext = txBase + 1;
  const events = scenario.events.map((event) => [...event]);
  if (selected === "coherent") {
    events[0][2] = `base epoch ${txBase} matches accepted state`;
    events[2][2] = `epoch ${txNext} became atomically visible`;
    events[3][2] = `damage [48,32] submitted behind fence ${fence + 1}`;
  } else if (selected === "stale") {
    events[0][2] = `expected base ${gpuEpoch}; received ${txBase}`;
    events[1][2] = `StaleEpoch { accepted: ${gpuEpoch} }`;
    events[3][2] = `retained scene remains unchanged at ${gpuEpoch}`;
  } else if (selected === "lifetime") {
    events[0][2] = `image@88 is referenced by in-flight fence ${fence + 1}`;
    events[1][2] = `release queued after fence ${fence + 1}`;
    events[2][2] = `logical ownership ended at epoch ${txNext}`;
  }
  ui.commit.disabled = true;
  ui.commitLabel.textContent = "Validating…";
  ui.gateStatus.textContent = "VALIDATING";
  ui.gate.classList.remove("transmitting");
  void ui.gate.offsetWidth;
  ui.gate.classList.add("transmitting");

  const gpuScene = document.querySelector(".gpu-scene");
  gpuScene.classList.remove("flash", "scanning");
  void gpuScene.offsetWidth;
  gpuScene.classList.add("flash", "scanning");

  events.forEach((event, index) => {
    window.setTimeout(() => appendEvent(event[0], event[1], event[2], index), 230 + index * 230);
  });
  window.setTimeout(() => {
    finishCommit(scenario);
    const nextBase = selected === "stale" ? gpuEpoch - 1 : gpuEpoch;
    ui.base.textContent = `#${nextBase}`;
    ui.tx.textContent = `#${nextBase + 1}`;
  }, 230 + events.length * 230);
}

function reset() {
  if (busy) return;
  gpuEpoch = 1042;
  fence = 731;
  eventCount = 0;
  ui.cpuEpoch.textContent = gpuEpoch;
  ui.gpuEpoch.textContent = gpuEpoch;
  ui.fence.textContent = fence;
  ui.resource.textContent = "88";
  ui.resident.textContent = "24.8 MiB";
  ui.logRows.innerHTML = '<div class="empty-log">Commit a transaction to inspect the contract.</div>';
  ui.logCount.textContent = "0 events";
  ui.commit.disabled = false;
  setScenario(selected);
}

document.querySelectorAll(".scenario").forEach((button) => {
  button.addEventListener("click", () => setScenario(button.dataset.scenario));
});
ui.commit.addEventListener("click", commit);
ui.reset.addEventListener("click", reset);
window.addEventListener("keydown", (event) => {
  if (event.code === "Space" && !event.repeat && !["INPUT", "TEXTAREA", "BUTTON"].includes(document.activeElement.tagName)) {
    event.preventDefault();
    commit();
  }
});

setScenario(selected);

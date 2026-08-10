"use strict";

const ui = {
  missionButtons: [...document.querySelectorAll("[data-demo]")],
  directoryLinks: [...document.querySelectorAll("[data-select-demo]")],
  kicker: document.querySelector("#demo-kicker"),
  title: document.querySelector("#demo-title"),
  outcome: document.querySelector("#demo-outcome"),
  requires: document.querySelector("#demo-requires"),
  policy: document.querySelector("#demo-policy"),
  terminal: document.querySelector("#terminal-lines"),
  phases: [...document.querySelectorAll("[data-phase]")],
  play: document.querySelector("#play-toggle"),
  restart: document.querySelector("#restart-demo"),
  status: document.querySelector("#demo-status"),
  progress: document.querySelector("#demo-progress-bar"),
  copyButton: document.querySelector("[data-copy-target]"),
  copyStatus: document.querySelector("#copy-status"),
  themeButtons: [...document.querySelectorAll("[data-theme-value]")],
  machineKeys: [...document.querySelectorAll("[data-machine-target]")],
  screenDeck: document.querySelector("#screen-deck"),
  screenPanels: [...document.querySelectorAll("[data-screen-panel]")],
  screenLinks: [...document.querySelectorAll("[data-screen-link]")],
  themeColor: document.querySelector('meta[name="theme-color"]'),
  year: document.querySelector("#current-year")
};

const phaseOrder = ["request", "review", "run"];
const screenOrder = ["home", "proof", "programs", "catalog", "policy", "install"];
const screenHashes = {
  home: "#screen-home",
  proof: "#screen-proof",
  programs: "#screen-programs",
  catalog: "#screen-catalog",
  policy: "#screen-policy",
  install: "#screen-install"
};
const themeStorageKey = "t4e-site-theme";
const themeColors = {
  future: "#071014",
  amber: "#120b00",
  green_screen: "#020b04",
  terracotta: "#141413"
};
const reducedMotion = window.matchMedia("(prefers-reduced-motion: reduce)");
const state = {
  demos: new Map(),
  active: null,
  eventIndex: 0,
  playing: false,
  timer: null,
  screen: "home"
};
let wheelDelta = 0;
let wheelResetTimer = null;
let wheelLockedUntil = 0;
let touchStartY = null;

function savedTheme() {
  try {
    return window.localStorage.getItem(themeStorageKey);
  } catch {
    return null;
  }
}

function applyTheme(requestedTheme, persist = true) {
  const migratedTheme = ["default", "cyan"].includes(requestedTheme)
    ? "future"
    : requestedTheme;
  const theme = Object.hasOwn(themeColors, migratedTheme) ? migratedTheme : "amber";
  document.documentElement.dataset.theme = theme;
  ui.themeButtons.forEach((button) => {
    button.setAttribute("aria-pressed", String(button.dataset.themeValue === theme));
  });
  ui.themeColor?.setAttribute("content", themeColors[theme]);

  if (persist) {
    try {
      window.localStorage.setItem(themeStorageKey, theme);
    } catch {
      // The selected theme still applies when storage is unavailable.
    }
  }
}

function setCurrentNavigation(screen) {
  const activePanel = ui.screenPanels.find((panel) => panel.dataset.screenPanel === screen);
  const activeGroup = activePanel?.dataset.screenGroup || screen;
  ui.machineKeys.forEach((link) => {
    const isCurrent = link.dataset.machineTarget === activeGroup;
    link.classList.toggle("is-selected", isCurrent);
    if (isCurrent) {
      link.setAttribute("aria-current", "location");
    } else {
      link.removeAttribute("aria-current");
    }
  });
}

function screenFromLocation() {
  if (/^#demo=/.test(window.location.hash) || ["#missions", "#screen-programs"].includes(window.location.hash)) {
    return "programs";
  }
  if (["#home-proof", "#screen-proof"].includes(window.location.hash)) return "proof";
  if (["#catalog", "#screen-catalog"].includes(window.location.hash)) return "catalog";
  if (["#boundaries", "#screen-policy"].includes(window.location.hash)) return "policy";
  if (["#install", "#screen-install"].includes(window.location.hash)) return "install";
  return "home";
}

function showScreen(requestedScreen, options = {}) {
  const available = new Set(ui.screenPanels.map((panel) => panel.dataset.screenPanel));
  const screen = available.has(requestedScreen) ? requestedScreen : "home";
  const previousIndex = screenOrder.indexOf(state.screen);
  const nextIndex = screenOrder.indexOf(screen);
  const direction = options.direction || (nextIndex < previousIndex ? -1 : 1);
  state.screen = screen;

  ui.screenPanels.forEach((panel) => {
    const isActive = panel.dataset.screenPanel === screen;
    panel.hidden = !isActive;
    panel.classList.remove("is-screen-active");
    panel.classList.toggle("is-screen-backward", isActive && direction < 0);
    if (isActive && options.animate !== false) {
      void panel.offsetWidth;
      panel.classList.add("is-screen-active");
    }
  });

  ui.screenDeck.scrollTop = 0;
  setCurrentNavigation(screen);
  if (options.hash) window.history.replaceState(null, "", options.hash);
  window.requestAnimationFrame(() => {
    window.scrollTo({ top: 0, left: 0, behavior: "instant" });
  });
}

function stepScreen(direction) {
  const currentIndex = screenOrder.indexOf(state.screen);
  const nextIndex = Math.max(0, Math.min(screenOrder.length - 1, currentIndex + direction));
  if (nextIndex === currentIndex) return;
  const nextScreen = screenOrder[nextIndex];
  showScreen(nextScreen, { direction, hash: screenHashes[nextScreen] });
}

function canScrollWithin(target, direction) {
  let element = target instanceof Element ? target : target?.parentElement;
  while (element && element !== ui.screenDeck) {
    const overflowY = window.getComputedStyle(element).overflowY;
    const isScrollable = /^(auto|scroll)$/.test(overflowY)
      && element.scrollHeight > element.clientHeight + 1;
    if (isScrollable) {
      const atStart = element.scrollTop <= 1;
      const atEnd = element.scrollTop + element.clientHeight >= element.scrollHeight - 1;
      if ((direction < 0 && !atStart) || (direction > 0 && !atEnd)) return true;
    }
    element = element.parentElement;
  }
  return false;
}

function handleScreenWheel(event) {
  const multiplier = event.deltaMode === WheelEvent.DOM_DELTA_LINE ? 16 : 1;
  const delta = event.deltaY * multiplier;
  const direction = delta > 0 ? 1 : -1;
  if (canScrollWithin(event.target, direction)) {
    wheelDelta = 0;
    return;
  }
  event.preventDefault();
  if (performance.now() < wheelLockedUntil) return;
  wheelDelta += delta;
  window.clearTimeout(wheelResetTimer);
  wheelResetTimer = window.setTimeout(() => { wheelDelta = 0; }, 140);
  if (Math.abs(wheelDelta) < 42) return;
  const stepDirection = wheelDelta > 0 ? 1 : -1;
  wheelDelta = 0;
  wheelLockedUntil = performance.now() + 420;
  stepScreen(stepDirection);
}

function createTerminalLine(event, text, isNewest) {
  const line = document.createElement("div");
  line.className = `terminal-line tone-${event.tone || "muted"}`;
  if (isNewest) line.classList.add("is-new");

  const label = document.createElement("span");
  label.className = "line-label";
  label.textContent = event.label;

  const content = document.createElement("span");
  content.textContent = text;

  line.append(label, content);
  return line;
}

function renderTerminal() {
  const events = state.active.events.slice(0, state.eventIndex + 1);
  const fragment = document.createDocumentFragment();

  events.forEach((event, eventIndex) => {
    event.lines.forEach((line, lineIndex) => {
      const newest = eventIndex === events.length - 1 && lineIndex === event.lines.length - 1;
      fragment.append(createTerminalLine(event, line, newest));
    });
  });

  ui.terminal.replaceChildren(fragment);
  requestAnimationFrame(() => {
    ui.terminal.scrollTop = ui.terminal.scrollHeight;
  });
}

function renderProgress() {
  const event = state.active.events[state.eventIndex];
  const currentPhase = phaseOrder.indexOf(event.phase);

  ui.phases.forEach((phase, index) => {
    phase.classList.toggle("is-active", index === currentPhase);
    phase.classList.toggle("is-complete", index < currentPhase);
  });

  const completed = state.eventIndex + 1;
  const total = state.active.events.length;
  ui.progress.style.width = `${(completed / total) * 100}%`;
  ui.status.textContent = state.playing
    ? `Playing step ${completed} of ${total}`
    : completed === total
      ? `Demo complete — ${total} steps`
      : `Paused at step ${completed} of ${total}`;
}

function renderControls() {
  ui.play.textContent = state.playing ? "Ⅱ" : "▶";
  ui.play.setAttribute("aria-label", state.playing ? "Pause demo" : "Play demo");
  renderProgress();
}

function stopTimer() {
  window.clearTimeout(state.timer);
  state.timer = null;
}

function scheduleNext() {
  stopTimer();
  if (!state.playing) return;

  if (state.eventIndex >= state.active.events.length - 1) {
    state.playing = false;
    renderControls();
    return;
  }

  const event = state.active.events[state.eventIndex];
  const delay = event.phase === "run" ? 1250 : 1450;
  state.timer = window.setTimeout(() => {
    state.eventIndex += 1;
    renderTerminal();
    renderControls();
    scheduleNext();
  }, delay);
}

function updateMissionButtons(id) {
  ui.missionButtons.forEach((button) => {
    const active = button.dataset.demo === id;
    button.classList.toggle("is-active", active);
    button.setAttribute("aria-pressed", String(active));
  });
}

function updateHash(id) {
  const url = new URL(window.location.href);
  url.hash = `demo=${id}`;
  window.history.replaceState(null, "", url);
}

function selectDemo(id, options = {}) {
  const demo = state.demos.get(id) || state.demos.values().next().value;
  if (!demo) return;

  stopTimer();
  state.active = demo;
  state.eventIndex = reducedMotion.matches ? demo.events.length - 1 : 0;
  state.playing = !reducedMotion.matches && options.autoplay !== false;

  ui.kicker.textContent = demo.kicker;
  ui.title.textContent = demo.title;
  ui.outcome.textContent = demo.outcome;
  ui.requires.textContent = demo.requires;
  ui.policy.textContent = demo.policy;
  updateMissionButtons(demo.id);
  renderTerminal();
  renderControls();
  if (options.updateUrl !== false) updateHash(demo.id);
  scheduleNext();
}

function requestedDemoId() {
  const match = window.location.hash.match(/^#demo=([a-z0-9-]+)$/);
  return match ? match[1] : null;
}

function handleMissionKeys(event) {
  if (!['ArrowDown', 'ArrowRight', 'ArrowUp', 'ArrowLeft'].includes(event.key)) return;
  event.preventDefault();
  const current = ui.missionButtons.indexOf(event.currentTarget);
  const direction = event.key === "ArrowDown" || event.key === "ArrowRight" ? 1 : -1;
  const next = (current + direction + ui.missionButtons.length) % ui.missionButtons.length;
  ui.missionButtons[next].focus();
  ui.missionButtons[next].click();
}

async function loadDemos() {
  try {
    const response = await fetch("./demos.json", { cache: "no-cache" });
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    const payload = await response.json();
    if (!Array.isArray(payload.demos) || payload.demos.length === 0) {
      throw new Error("No demo scenarios found");
    }

    payload.demos.forEach((demo) => state.demos.set(demo.id, demo));
    const requested = requestedDemoId();
    selectDemo(requested || "pipeline", { updateUrl: Boolean(requested) });
  } catch (error) {
    console.error("T4E demo data could not be loaded", error);
    ui.status.textContent = "Demo data unavailable — static preview shown";
    ui.play.disabled = true;
    ui.restart.disabled = true;
  }
}

async function copyText(text) {
  if (navigator.clipboard && window.isSecureContext) {
    await navigator.clipboard.writeText(text);
    return;
  }

  const input = document.createElement("textarea");
  input.value = text;
  input.setAttribute("readonly", "");
  input.style.position = "fixed";
  input.style.opacity = "0";
  document.body.append(input);
  input.select();
  const copied = document.execCommand("copy");
  input.remove();
  if (!copied) throw new Error("Clipboard copy failed");
}

ui.missionButtons.forEach((button) => {
  button.addEventListener("click", () => selectDemo(button.dataset.demo));
  button.addEventListener("keydown", handleMissionKeys);
});

ui.directoryLinks.forEach((link) => {
  link.addEventListener("click", (event) => {
    event.preventDefault();
    showScreen("programs", { hash: `#demo=${link.dataset.selectDemo}` });
    selectDemo(link.dataset.selectDemo);
  });
});

ui.themeButtons.forEach((button) => {
  button.addEventListener("click", () => applyTheme(button.dataset.themeValue));
});

ui.machineKeys.forEach((key) => {
  key.addEventListener("click", (event) => {
    key.classList.add("is-pressed");
    window.setTimeout(() => key.classList.remove("is-pressed"), 150);
    if (key.dataset.machineTarget === "source") return;
    event.preventDefault();
    showScreen(key.dataset.machineTarget, { hash: key.getAttribute("href") });
  });
});

ui.screenLinks.forEach((link) => {
  link.addEventListener("click", (event) => {
    event.preventDefault();
    showScreen(link.dataset.screenLink, { hash: link.getAttribute("href") });
  });
});

ui.screenDeck.addEventListener("wheel", handleScreenWheel, { passive: false });
ui.screenDeck.addEventListener("touchstart", (event) => {
  touchStartY = event.changedTouches[0]?.clientY ?? null;
}, { passive: true });
ui.screenDeck.addEventListener("touchend", (event) => {
  if (touchStartY === null) return;
  const endY = event.changedTouches[0]?.clientY ?? touchStartY;
  const distance = touchStartY - endY;
  touchStartY = null;
  if (Math.abs(distance) < 48) return;
  const direction = distance > 0 ? 1 : -1;
  if (!canScrollWithin(event.target, direction)) stepScreen(direction);
}, { passive: true });
ui.screenDeck.addEventListener("keydown", (event) => {
  if (event.target !== ui.screenDeck) return;
  if (["ArrowDown", "PageDown"].includes(event.key)) {
    event.preventDefault();
    stepScreen(1);
  } else if (["ArrowUp", "PageUp"].includes(event.key)) {
    event.preventDefault();
    stepScreen(-1);
  }
});

ui.play.addEventListener("click", () => {
  if (!state.active) return;
  if (state.eventIndex >= state.active.events.length - 1) state.eventIndex = 0;
  state.playing = !state.playing;
  renderTerminal();
  renderControls();
  scheduleNext();
});

ui.restart.addEventListener("click", () => {
  if (!state.active) return;
  state.eventIndex = reducedMotion.matches ? state.active.events.length - 1 : 0;
  state.playing = !reducedMotion.matches;
  renderTerminal();
  renderControls();
  scheduleNext();
});

ui.copyButton.addEventListener("click", async () => {
  const target = document.querySelector(`#${CSS.escape(ui.copyButton.dataset.copyTarget)}`);
  if (!target) return;
  try {
    await copyText(target.textContent.trim());
    ui.copyButton.textContent = "COPIED";
    ui.copyStatus.textContent = "Install command copied to clipboard.";
  } catch {
    ui.copyStatus.textContent = "Copy failed. Select the command and copy it manually.";
  }
  window.setTimeout(() => {
    ui.copyButton.textContent = "COPY";
  }, 2200);
});

document.addEventListener("visibilitychange", () => {
  if (document.hidden && state.playing) {
    state.playing = false;
    stopTimer();
    renderControls();
  }
});

window.addEventListener("load", () => {
  window.scrollTo({ top: 0, left: 0, behavior: "instant" });
}, { once: true });

window.addEventListener("hashchange", () => {
  const id = requestedDemoId();
  showScreen(screenFromLocation());
  if (id && state.demos.has(id) && state.active?.id !== id) selectDemo(id);
});

reducedMotion.addEventListener("change", () => {
  if (state.active) selectDemo(state.active.id, { autoplay: false });
});

applyTheme(savedTheme(), false);
showScreen(screenFromLocation(), { animate: false });
ui.year.textContent = String(new Date().getFullYear());
loadDemos();

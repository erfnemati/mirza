// Mirza settings window. Settings are saved as soon as they change, and the
// running app picks them up right away.

const { invoke } = window.__TAURI__.core;

const PROVIDERS = [
  { id: "soniox", name: "Soniox", blurb: "Real-time, strong in Persian", env: "SONIOX_API_KEY", strict: true,
    keyUrl: "https://console.soniox.com", usageUrl: "https://console.soniox.com" },
  { id: "elevenlabs", name: "ElevenLabs", blurb: "Scribe v2 real-time", env: "ELEVENLABS_API_KEY", strict: true,
    keyUrl: "https://elevenlabs.io/app/settings/api-keys", usageUrl: "https://elevenlabs.io/app/usage" },
  { id: "openai", name: "OpenAI", blurb: "GPT transcribe and Whisper", env: "OPENAI_API_KEY", strict: false,
    keyUrl: "https://platform.openai.com/api-keys", usageUrl: "https://platform.openai.com/usage" },
];

/** AI providers for revising dictation, the recommended one first; the key of "local" is optional. */
const AI_PROVIDERS = [
  { id: "claude", name: "Claude", blurb: "Anthropic's Haiku", env: "ANTHROPIC_API_KEY", recommended: true,
    keyUrl: "https://console.anthropic.com/settings/keys", models: ["claude-haiku-5-5", "claude-sonnet-5-5"] },
  { id: "openai", name: "OpenAI", blurb: "GPT, fast and cheap", env: "OPENAI_API_KEY",
    keyUrl: "https://platform.openai.com/api-keys", models: ["gpt-6-luna"] },
  { id: "gemini", name: "Gemini", blurb: "Google's Flash-Lite", env: "GEMINI_API_KEY",
    keyUrl: "https://aistudio.google.com/apikey", models: ["gemini-3.5-flash-lite", "gemini-3.8-flash"] },
  { id: "openrouter", name: "OpenRouter", blurb: "Many models, one key", env: "OPENROUTER_API_KEY",
    keyUrl: "https://openrouter.ai/settings/keys",
    models: ["openai/gpt-6-luna", "google/gemini-3.5-flash-lite", "anthropic/claude-haiku-5.5"] },
  { id: "local", name: "Local server", blurb: "vLLM, Ollama, LM Studio", env: "MIRZA_LOCAL_API_KEY", optionalKey: true, models: [] },
];

/** How much AI revision may change, and the ready-made rules (ids match the config). */
const REVISION_STYLES = [
  { id: "clean", name: "Clean only", blurb: "Fillers, repeats, punctuation. Your words stay." },
  { id: "light", name: "Light polish", blurb: "Also grammar and run-on sentences, few changes." },
  { id: "full", name: "Full polish", blurb: "Reads like written text. Every point stays." },
];
/** Persian inside an English sentence, kept right to left so its «» don't flip. */
const fa = (text) => h("bdi", { textContent: text, attrs: { dir: "rtl" } });
const REVISION_PRESETS = [
  { id: "plain_words", name: "Plain words", desc: () => "No words that sound AI-written (delve, leverage, robust…) and no jargon or bookish words." },
  { id: "formal_persian", name: "Formal Persian", desc: () => h("span", {}, "Written Persian instead of spoken: ", fa("«می‌خوام»"), " becomes ", fa("«می‌خواهم»"), ".") },
  { id: "digits", name: "Numbers as digits", desc: () => h("span", {}, fa("«صد و پنجاه هزار»"), " becomes ", fa("«۱۵۰ هزار»"), ", “twenty” becomes 20.") },
  { id: "no_dashes", name: "No dashes or emoji", desc: () => "Commas and periods instead of dashes between sentences." },
];

const SECTIONS = [
  { id: "provider", label: "Speech service", icon: "waves" },
  { id: "shortcuts", label: "Shortcuts", icon: "keyboard" },
  { id: "revision", label: "AI revision", icon: "sparkles" },
  { id: "tasks", label: "Voice tasks", icon: "tasks" },
  { id: "typing", label: "Typing", icon: "type" },
  { id: "listening", label: "Listening", icon: "mic" },
  { id: "usage", label: "Usage", icon: "chart" },
  { id: "history", label: "Recent text", icon: "history" },
  { id: "general", label: "General", icon: "sliders" },
];

/** The fixed list of shortcuts, each an action (and mode for dictation). */
const SHORTCUTS = [
  { action: "dictate", mode: "toggle", id: "dictate-toggle", label: "Start / stop dictation",
    desc: (c) => `Press once to start talking, press again to stop.${c.silence_stop_sec ? ` Mirza also stops after ${c.silence_stop_sec} seconds of silence.` : ""}` },
  { action: "dictate", mode: "hold", id: "dictate-hold", label: "Hold to talk",
    desc: () => "Hold the keys while you talk. The text is typed when you let go." },
  { action: "revise", mode: "toggle", id: "revise-toggle", label: "Dictate and revise",
    desc: (c) => `Like start / stop, but AI cleans up the text before it's typed.${c.revision.provider ? "" : " Choose an AI provider in AI revision first."}` },
  { action: "revise", mode: "hold", id: "revise-hold", label: "Hold, then revise",
    desc: () => "Like hold to talk, but AI cleans up the text before it's typed." },
  { action: "task", mode: "toggle", id: "task-toggle", label: "Say a task",
    desc: () => "Press once, say a task, press again. It goes to Google Tasks, not to the window you're in." },
  { action: "task", mode: "hold", id: "task-hold", label: "Hold to say a task",
    desc: () => "Hold the keys while you say the task." },
  { action: "cancel", id: "cancel", label: "Cancel", desc: () => "Stop right away. Text that's already typed stays." },
  { action: "panel", id: "panel", label: "Open settings", desc: () => "Open this window." },
  { action: "next_provider", id: "next-provider", label: "Switch service", desc: () => "Switch to the next speech service." },
];

const state = {
  cfg: null,
  keys: {},
  platform: "linux",
  desktop: "",
  wayland: false,
  configPath: "",
  configError: "",
  version: "",
  snap: null,
  section: "provider",
  models: {}, // provider -> {list, error, loading}
  aiModels: {}, // AI provider -> {list, error, loading}
  tryText: "",
  tryResult: null, // {text, ms} or {error}
  languages: [],
  mics: [],
  editingKey: false,
  editingBalance: false,
  usageRefreshing: false,
  usageFresh: false,
  tasks: null, // what tasks_state returned
  taskLists: null, // {loading, error} while reading the lists from Google
  taskEdit: null, // id of the task in the edit form
  taskTryText: "",
  taskTry: null, // {title, detail, note, guesses, ms} or {error}
  signIn: null, // {url} while waiting for Google
  signInNote: "",
  changingClient: false,
};

const provider = () => PROVIDERS.find((p) => p.id === state.cfg.active_provider) || PROVIDERS[0];
const pcfg = () => state.cfg.providers[provider().id];
const money = (v) => (v !== 0 && Math.abs(v) < 0.01 ? `$${v.toFixed(4)}` : `$${v.toFixed(2)}`);

async function call(cmd, args) {
  try {
    return await invoke(cmd, args);
  } catch (e) {
    toast(String(e), true);
    throw e;
  }
}

async function loadState() {
  const s = await invoke("load");
  Object.assign(state, {
    cfg: s.config,
    keys: s.keys,
    platform: s.platform,
    desktop: s.desktop,
    wayland: s.wayland,
    configPath: s.config_path,
    configError: s.config_error,
    version: s.version,
  });
}

/** Saves one setting; re-renders the page only if asked (so typing isn't interrupted). */
async function save(path, value, { rerender = false, quiet = false } = {}) {
  await call("set_setting", { path, value });
  await loadState();
  if (!quiet) toast("Saved");
  if (rerender) renderPage();
  renderNav();
}

// ---- Frame -----------------------------------------------------------------------

function renderNav() {
  const nav = document.getElementById("nav");
  const attention = {
    provider: !state.keys[provider().id]?.present,
    shortcuts: !state.cfg.shortcuts.some((s) => s.action === "dictate" && s.keys),
    typing: !!(state.snap && state.snap.typing_problem),
    tasks: !!(state.tasks && (state.tasks.waiting.some((w) => w.error) || (!state.tasks.signed_in && state.cfg.shortcuts.some((s) => s.action === "task" && s.keys)))),
    revision: (() => {
      const a = AI_PROVIDERS.find((x) => x.id === state.cfg.revision.provider);
      return !!a && !a.optionalKey && !state.keys[a.id]?.present;
    })(),
  };
  nav.replaceChildren(
    ...SECTIONS.map((s) =>
      h(
        "button",
        {
          class: "nav-item",
          type: "button",
          attrs: { "aria-current": state.section === s.id ? "page" : null },
          onclick: () => go(s.id),
        },
        icon(s.icon),
        h("span", { textContent: s.label }),
        attention[s.id] ? h("i", { class: "badge", attrs: { "aria-label": "Needs attention" } }) : null,
      ),
    ),
  );
}

function go(section) {
  closeMenus();
  state.section = section;
  state.editingKey = false;
  state.editingBalance = false;
  try {
    localStorage.setItem("mirza.section", section);
  } catch {}
  renderNav();
  renderPage();
  document.getElementById("content").scrollTop = 0;
}

function renderHeader() {
  const dot = document.querySelector("#status .dot");
  const text = document.getElementById("status-text");
  const btn = document.getElementById("main-action");
  const s = state.snap && state.snap.status;
  const update = state.snap && state.snap.update;
  const upd = document.getElementById("update");
  upd.classList.toggle("hidden", !update);
  if (update) {
    upd.replaceChildren(icon("download"), `Update to ${update.version}`);
    upd.onclick = () => call("open_url", { url: update.url });
  }
  btn.classList.add("hidden");
  if (!s) {
    dot.className = "dot off";
    text.textContent = "Not running";
    btn.className = "btn btn-sm btn-primary";
    btn.replaceChildren(icon("play"), "Start Mirza");
    btn.onclick = async () => {
      await call("start_daemon");
      setTimeout(poll, 1200);
    };
    return;
  }
  const name = (PROVIDERS.find((p) => p.id === s.provider) || { name: s.provider }).name;
  const map = {
    idle: ["ready", `Ready · ${name}`],
    connecting: ["busy", `Connecting to ${name}…`],
    listening: ["listening", `Listening · ${name}`],
    finishing: ["busy", "Finishing…"],
    revising: ["busy", "Revising with AI…"],
  };
  const [cls, label] = map[s.state] || map.idle;
  dot.className = "dot " + cls;
  text.textContent = label;
  if (s.state === "listening") {
    btn.className = "btn btn-sm btn-stop";
    btn.replaceChildren(icon("stop"), "Stop");
    btn.onclick = () => call("send", { request: { cmd: "stop" } }).then(poll);
  }
}

/** Problems worth showing on every page. */
function globalNotices() {
  const out = [];
  if (state.configError) {
    out.push(notice(`The settings file has an error, so defaults are shown. ${state.configError}`, {
      error: true,
      actions: [button("Open settings file", () => call("open_config"), { size: "sm" })],
    }));
  }
  if (!state.snap) {
    out.push(notice("Mirza isn't running, so your shortcuts won't work.", {
      actions: [button("Start Mirza", async () => (await call("start_daemon"), setTimeout(poll, 1200)), { size: "sm", variant: "primary" })],
    }));
  } else if (state.snap.status.last_error) {
    out.push(notice(`The last dictation failed: ${state.snap.status.last_error}`, { error: true }));
  }
  return out;
}

function renderPage() {
  const content = document.getElementById("content");
  const page = h("div", { class: "page" });
  const build = { provider: pageProvider, shortcuts: pageShortcuts, revision: pageRevision, tasks: pageTasks, typing: pageTyping, listening: pageListening, usage: pageUsage, history: pageHistory, general: pageGeneral }[state.section];
  page.append(...build().filter(Boolean));
  page.insertBefore(h("div", {}, ...globalNotices()), page.children[1] || null);
  content.replaceChildren(page);
}

// ---- Speech service -----------------------------------------------------------

function pageProvider() {
  const p = provider();
  const c = pcfg();
  const key = state.keys[p.id] || {};
  const models = state.models[p.id];
  if (!models) loadModels(p.id);

  const choices = h(
    "div",
    { class: "choices", attrs: { role: "radiogroup", "aria-label": "Speech service" } },
    ...PROVIDERS.map((it) =>
      h(
        "button",
        {
          class: "choice",
          type: "button",
          attrs: { role: "radio", "aria-checked": String(it.id === p.id) },
          onclick: () => it.id !== p.id && setProvider(it.id),
        },
        h("strong", { textContent: it.name }),
        h("span", { textContent: it.blurb }),
      ),
    ),
  );

  const keyControl = keyField(p, () => {
    delete state.models[p.id];
    call("send", { request: { cmd: "refresh_usage" } }).then(() => setTimeout(poll, 1500), () => {});
  });

  // Model: pick from the provider's list, or type any name.
  const modelSuggestions = () =>
    ((state.models[p.id] && state.models[p.id].list) || []).map((m) => ({
      value: m.id,
      label: m.id,
      meta: m.alias_of ? `same as ${m.alias_of}` : m.name,
    }));
  const model = combobox({
    value: c.model,
    suggestions: modelSuggestions,
    placeholder: "Model name",
    label: "Model",
    onCommit: (v) => save(`providers.${p.id}.model`, v, { rerender: true }),
  });
  const modelHint = h("p", { class: "hint" });
  if (!models || models.loading) modelHint.textContent = "Loading the list of models…";
  else if (models.error) modelHint.textContent = `Couldn't load ${p.name}'s list (${models.error}), so only known models are shown. You can still type any model name.`;
  else modelHint.textContent = "Pick one from the list or type a model name.";

  // Languages: search and pick.
  const current = ((models && models.list) || []).find((m) => m.id === c.model);
  const langList = current && current.languages.length ? current.languages : state.languages;
  const languages = multiSelect({
    options: langList.map((l) => ({ value: l.code, label: l.name })),
    values: c.languages.slice(),
    placeholder: "Search languages…",
    label: "Languages",
    onChange: (v) => save(`providers.${p.id}.languages`, v, { quiet: true }),
  });

  const terms = h("textarea", { class: "input", rows: 2, placeholder: "Names, product names, jargon…", value: c.terms.join(", "), attrs: { dir: "auto", "aria-label": "Words to expect" } });
  terms.addEventListener("change", () => save(`providers.${p.id}.terms`, terms.value.split(/[,،\n]/).map((s) => s.trim()).filter(Boolean)));

  return [
    pageHead("Speech service", "Mirza sends your voice to one of these services and types what it hears."),
    !key.present ? notice(`Add your ${p.name} API key to start dictating.`) : null,
    group(row({ label: "Service", stack: true, control: choices })),
    group(
      row({ label: "API key", stack: true, control: keyControl }),
      row({ label: "Model", stack: true, control: h("div", {}, model, modelHint) }),
      row({
        label: "Languages",
        desc: "The languages you speak. Leave empty to let the service work it out.",
        stack: true,
        control: languages,
      }),
      p.strict
        ? row({
            label: "Only these languages",
            desc: "Never guess another language.",
            info: "Normally the languages above are hints: the service listens for them first but can still recognize others. Turn this on if it sometimes hears the wrong language, like writing your Persian as Arabic.",
            control: switchEl(c.strict_languages, (on) => save(`providers.${p.id}.strict_languages`, on), "Only these languages"),
          })
        : null,
    ),
    group(
      row({
        label: "Words to expect",
        desc: "Names and terms the service should listen for, separated by commas.",
        stack: true,
        control: terms,
      }),
    ),
  ];
}

/** The API key: hidden once saved, with a button to change it. */
function keyField(p, onSaved, { optional = false } = {}) {
  const key = state.keys[p.id] || {};
  if (key.present && !state.editingKey) {
    return h(
      "div",
      {},
      h(
        "div",
        { class: "input-row" },
        h("div", { class: "input mono", attrs: { "aria-label": "Saved API key" }, style: "display:flex;align-items:center;color:var(--muted-foreground)" }, key.masked),
        button("Edit", () => ((state.editingKey = true), renderPage()), { iconName: "pencil" }),
        key.source === "file" ? button("", () => removeKey(p.id), { variant: "ghost", size: "icon", title: "Remove this key", iconName: "x" }) : null,
      ),
      key.source === "env" ? h("p", { class: "hint", textContent: `Set by the ${p.env} environment variable, which is used instead of a saved key.` }) : null,
    );
  }
  const input = h("input", { class: "input mono", type: "password", placeholder: optional ? "Only if your server asks for one" : `Paste your ${p.name} API key`, spellcheck: false, attrs: { autocomplete: "off", "aria-label": "API key" } });
  const reveal = button("", () => {
    input.type = input.type === "password" ? "text" : "password";
    reveal.replaceChildren(icon(input.type === "password" ? "eye" : "eyeOff"));
  }, { variant: "ghost", size: "icon", title: "Show or hide", iconName: "eye" });
  const saveKey = async () => {
    const v = input.value.trim();
    if (!v) return toast("Paste a key first", true);
    await call("set_key", { provider: p.id, key: v });
    state.editingKey = false;
    await loadState();
    toast("Key saved");
    onSaved();
    renderNav();
    renderPage();
  };
  input.addEventListener("keydown", (e) => e.key === "Enter" && saveKey());
  const link = p.keyUrl
    ? h("button", { class: "btn btn-ghost btn-sm", type: "button", onclick: () => call("open_url", { url: p.keyUrl }), style: "padding:0;height:auto;color:var(--primary)" }, `Get a ${p.name} key`, icon("external"))
    : null;
  if (!optional) setTimeout(() => state.editingKey && input.focus(), 0);
  return h(
    "div",
    {},
    h(
      "div",
      { class: "input-row" },
      input,
      reveal,
      button("Save", saveKey, { variant: "primary" }),
      key.present ? button("Cancel", () => ((state.editingKey = false), renderPage())) : null,
    ),
    h("p", { class: "hint" }, "Stored only on this computer. ", link),
  );
}

async function setProvider(id) {
  if (state.snap) await call("send", { request: { cmd: "set_provider", id } });
  else await call("set_setting", { path: "active_provider", value: id });
  state.editingKey = false;
  await loadState();
  renderNav();
  renderPage();
  poll();
}

async function removeKey(id) {
  await call("set_key", { provider: id, key: "" });
  await loadState();
  toast("Key removed");
  delete state.models[id];
  delete state.aiModels[id];
  renderNav();
  renderPage();
}

async function loadModels(id) {
  state.models[id] = { loading: true, list: [] };
  const r = await invoke("list_models", { provider: id }).catch((e) => ({ models: [], error: String(e) }));
  state.models[id] = { loading: false, list: r.models, error: r.error };
  if (state.section === "provider" && provider().id === id && !document.querySelector("#content .dropdown:focus-within, #content :focus")) renderPage();
}

// ---- Shortcuts --------------------------------------------------------------------

function pageShortcuts() {
  const list = state.cfg.shortcuts;
  const bound = new Map((state.snap && state.snap.shortcuts) || []);
  const desktopOwnsKeys = state.platform === "linux" && state.wayland;
  const hasMode = (a) => a === "dictate" || a === "revise" || a === "task";
  const find = (s) => list.find((x) => x.action === s.action && (!hasMode(s.action) || (x.mode || "toggle") === s.mode));

  const setKeys = (s, keys) => {
    const next = list.filter((x) => x !== find(s));
    if (keys) next.push({ action: s.action, keys, mode: s.mode || "toggle" });
    saveShortcuts(next);
  };

  const rows = SHORTCUTS.map((s) => {
    const entry = find(s);
    const recorder = keyRecorder({
      value: entry && entry.keys,
      lone: !desktopOwnsKeys,
      label: s.label,
      onChange: (v) => setKeys(s, v),
    });
    const clear = entry ? button("", () => setKeys(s, null), { variant: "ghost", size: "icon", iconName: "x", title: "Remove this shortcut" }) : null;
    const r = row({ label: s.label, desc: s.desc(state.cfg), control: h("div", { class: "row-control" }, recorder, clear) });
    const actual = bound.get(s.id);
    const norm = (k) => k.toLowerCase().replace(/\s/g, "");
    if (entry && actual && desktopOwnsKeys && norm(actual) !== norm(entry.keys)) {
      r.querySelector(".row-text").append(h("p", { class: "hint", textContent: `Your desktop is using ${actual} for this.` }));
    }
    return r;
  });

  // The shortest-hold setting belongs with hold-to-talk.
  const holdSub = h(
    "div",
    { class: "sub" },
    "Ignore presses shorter than",
    numberInput(state.cfg.min_hold_ms, (v) => save("min_hold_ms", v), { label: "Shortest hold in milliseconds", max: 2000 }),
    h("span", { class: "unit", textContent: "ms" }),
  );

  const note = desktopOwnsKeys
    ? h(
        "p",
        { class: "hint" },
        "On Linux your desktop runs global shortcuts. If one doesn't work, another app may be using those keys: pick different ones here, or check ",
        h("button", { class: "btn btn-ghost btn-sm", type: "button", style: "padding:0;height:auto;color:var(--primary)", onclick: () => call("open_shortcut_settings") }, "System Settings → Shortcuts"),
        ".",
      )
    : h("p", { class: "hint", textContent: "Tip: a single key works too, like Right Ctrl. Using it with other keys (Right Ctrl+C) won't trigger Mirza." });

  return [
    pageHead("Shortcuts", "Use these anywhere, even while this window is closed. Click a shortcut, then press the new keys."),
    !list.some((x) => x.action === "dictate" && x.keys) ? notice("Set a key for dictation: click “Set shortcut” and press the keys you want.") : null,
    h("div", { class: "group" }, rows[0], h("div", {}, rows[1], holdSub), ...rows.slice(2, 4)),
    h("div", { class: "group" }, ...rows.slice(4, 6)),
    h("div", { class: "group" }, ...rows.slice(6)),
    note,
  ];
}

async function saveShortcuts(list) {
  await call("set_shortcuts", { list });
  await loadState();
  toast("Saved");
  renderNav();
  renderPage();
  setTimeout(poll, 800);
}

// ---- AI revision ------------------------------------------------------------------

function pageRevision() {
  const r = state.cfg.revision;
  const a = AI_PROVIDERS.find((x) => x.id === r.provider);
  const shortcutSet = state.cfg.shortcuts.some((x) => x.action === "revise" && x.keys);

  const choices = h(
    "div",
    { class: "choices", attrs: { role: "radiogroup", "aria-label": "AI provider" } },
    ...[{ id: "", name: "Off", blurb: "Type exactly what was heard" }, ...AI_PROVIDERS].map((it) =>
      h(
        "button",
        {
          class: "choice",
          type: "button",
          attrs: { role: "radio", "aria-checked": String(it.id === r.provider) },
          onclick: async () => {
            if (it.id === r.provider) return;
            state.editingKey = false;
            state.tryResult = null;
            await save("revision.provider", it.id, { rerender: true });
          },
        },
        h("strong", { textContent: it.name }),
        h("span", { textContent: it.blurb }),
        it.recommended ? h("em", { class: "tag", textContent: "Recommended" }) : null,
      ),
    ),
  );

  const out = [
    pageHead("AI revision", "When you stop talking, an AI model cleans up the text before it's typed: it drops fillers and repeats, keeps only your last correction, and fixes punctuation and half-spaces. It never translates."),
    a && !shortcutSet
      ? notice("Set a “Dictate and revise” shortcut to use it. Your usual dictation shortcuts stay as they are.", {
          actions: [button("Go to Shortcuts", () => go("shortcuts"), { size: "sm" })],
        })
      : null,
    group(row({ label: "Provider", stack: true, control: choices })),
  ];
  if (!a) return out;

  const c = state.cfg.ai[a.id];
  const models = state.aiModels[a.id];
  if (!models) loadAiModels(a.id);
  const key = state.keys[a.id] || {};
  const keyControl = keyField(a, () => delete state.aiModels[a.id], { optional: a.optionalKey });
  const model = combobox({
    value: c.model,
    suggestions: () => {
      const list = (models && models.list && models.list.length ? models.list : a.models).slice();
      return list.map((m) => ({ value: m, label: m }));
    },
    placeholder: a.id === "local" ? "The server's first model" : "Model name",
    label: "Model",
    onCommit: (v) => save(`ai.${a.id}.model`, v, { rerender: true }),
  });
  const modelHint = h("p", { class: "hint" });
  if (models && models.loading) modelHint.textContent = "Loading the list of models…";
  else if (models && models.error) modelHint.textContent = `Couldn't load the list (${models.error}). You can still type a model name.`;
  else if (a.id === "local") modelHint.textContent = "Leave empty to use the first model the server lists.";
  else modelHint.textContent = "The default is fast and cheap, which is all cleaning up text needs.";

  const address = h("input", { class: "input mono", value: c.base_url, placeholder: a.id === "local" ? "http://localhost:8000/v1" : "The provider's own address", spellcheck: false, attrs: { "aria-label": "Server address" } });
  address.addEventListener("change", async () => {
    delete state.aiModels[a.id];
    await save(`ai.${a.id}.base_url`, address.value.trim(), { rerender: true });
  });
  const effort = select({
    options: [
      { value: "default", label: "The model's default" },
      { value: "none", label: "None (fastest)" },
      { value: "minimal", label: "Minimal" },
      { value: "low", label: "Low" },
      { value: "medium", label: "Medium" },
      { value: "high", label: "High (slowest)" },
    ],
    value: c.effort,
    label: "Thinking",
    onChange: (v) => save(`ai.${a.id}.effort`, v),
  });
  const addressRow = row({
    label: "Server address",
    desc: a.id === "local"
      ? "Where your server's OpenAI-compatible API is, ending in /v1. Servers on this computer or your network are reached directly, not through the proxy."
      : "Leave empty unless you use a gateway in front of the provider.",
    stack: true,
    control: address,
  });
  const advanced = h(
    "details",
    { class: "group", open: a.id === "local" || !!c.base_url || undefined },
    h("summary", { class: "row", style: "cursor:pointer", textContent: "More options" }),
    a.id === "local" ? null : addressRow,
    row({
      label: "Thinking",
      desc: "How much the model may think first. More is slower and rarely helps here.",
      control: effort,
    }),
  );

  const styles = h(
    "div",
    { class: "choices", attrs: { role: "radiogroup", "aria-label": "How much to change" } },
    ...REVISION_STYLES.map((it) =>
      h(
        "button",
        {
          class: "choice",
          type: "button",
          attrs: { role: "radio", "aria-checked": String(it.id === r.style) },
          onclick: () => it.id !== r.style && save("revision.style", it.id, { rerender: true }),
        },
        h("strong", { textContent: it.name }),
        h("span", { textContent: it.blurb }),
      ),
    ),
  );
  const presetRows = REVISION_PRESETS.map((p) =>
    row({
      label: p.name,
      desc: p.desc(),
      control: switchEl(r.presets.includes(p.id), (on) => {
        // The page isn't redrawn after a switch, so read what is saved now.
        const now = state.cfg.revision.presets;
        const ids = REVISION_PRESETS.map((x) => x.id).filter((id) => (id === p.id ? on : now.includes(id)));
        // Keep ids a newer version wrote, so switching here doesn't drop them.
        const others = now.filter((id) => !REVISION_PRESETS.some((x) => x.id === id));
        save("revision.presets", [...ids, ...others]);
      }, p.name),
    }),
  );

  const rules = h("textarea", { class: "input", rows: 3, placeholder: "For example: Use British spelling.", value: r.instructions, attrs: { dir: "auto", "aria-label": "Your own rules" } });
  rules.addEventListener("change", () => save("revision.instructions", rules.value.trim()));

  const tryBox = h("textarea", { class: "input", rows: 3, placeholder: "Type or paste something the way you'd say it, fillers and all.", value: state.tryText, attrs: { dir: "auto", "aria-label": "Text to try" } });
  tryBox.addEventListener("input", () => (state.tryText = tryBox.value));
  const result = h("div", {});
  const showResult = () => {
    const t = state.tryResult;
    if (!t) return result.replaceChildren();
    if (t.error) return result.replaceChildren(notice(t.error, { error: true }));
    result.replaceChildren(
      h("div", { class: "transcript" }, h("p", { textContent: t.text, attrs: { dir: "auto" } })),
      h("p", { class: "hint", textContent: `Took ${(t.ms / 1000).toFixed(1)} s.` }),
    );
  };
  showResult();
  const tryIt = button("Revise", async () => {
    const text = tryBox.value.trim();
    if (!text) return toast("Type something first", true);
    tryIt.disabled = true;
    result.replaceChildren(h("p", { class: "hint", textContent: "Revising…" }));
    try {
      state.tryResult = await invoke("try_revision", { text });
    } catch (e) {
      state.tryResult = { error: String(e) };
    }
    tryIt.disabled = false;
    showResult();
  }, { variant: "primary", iconName: "sparkles" });

  out.push(
    !a.optionalKey && !key.present ? notice(`Add your ${a.name} API key to use AI revision.`) : null,
    group(
      row({ label: a.optionalKey ? "API key (optional)" : "API key", stack: true, control: keyControl }),
      a.id === "local" ? addressRow : null,
      row({ label: "Model", stack: true, control: h("div", {}, model, modelHint) }),
    ),
    advanced,
    group(
      row({
        label: "How much to change",
        desc: "Polish takes a little longer. Line breaks are never added, since Enter would send a chat message.",
        stack: true,
        control: styles,
      }),
      ...presetRows,
      row({
        label: "Your own rules",
        desc: "Added to Mirza's rules and the switches above, and they win over them.",
        stack: true,
        control: rules,
      }),
    ),
    group(
      row({
        label: "Time limit",
        desc: "If the AI hasn't answered by then, the original text is typed. Long dictations get a little more.",
        control: h("div", { class: "row-control" }, numberInput(r.timeout_sec, (v) => save("revision.timeout_sec", v), { min: 1, max: 120, label: "Time limit in seconds" }), h("span", { class: "unit", textContent: "s" })),
      }),
    ),
    group(
      row({
        label: "Try it",
        desc: a.id === "local" ? "Sent to your server, not stored by Mirza." : `Sent to ${a.name}, like your dictation will be.`,
        stack: true,
        control: h("div", { style: "display:grid;gap:8px" }, tryBox, h("div", {}, tryIt), result),
      }),
    ),
  );
  return out;
}

async function loadAiModels(id) {
  state.aiModels[id] = { loading: true, list: [] };
  const r = await invoke("list_ai_models", { provider: id }).catch((e) => ({ models: [], error: String(e) }));
  state.aiModels[id] = { loading: false, list: r.models, error: r.error };
  if (state.section === "revision" && state.cfg.revision.provider === id && !document.querySelector("#content .dropdown:focus-within, #content :focus")) renderPage();
}

// ---- Voice tasks ------------------------------------------------------------------

const TASKS_GUIDE = "https://github.com/erfnemati/mirza/blob/develop/docs/voice-tasks.md";

async function loadTasks() {
  try {
    state.tasks = await invoke("tasks_state");
  } catch (e) {
    toast(String(e), true);
  }
}

async function loadTaskLists(refresh) {
  state.taskLists = { loading: true };
  if (state.section === "tasks") renderPage();
  try {
    await invoke("task_lists", { refresh });
    state.taskLists = { loading: false };
    if (refresh) toast("Lists updated");
  } catch (e) {
    state.taskLists = { loading: false, error: String(e) };
  }
  await loadTasks();
  if (state.section === "tasks") renderPage();
}

/** Opens the edit form for a task (from its notification or the list). */
async function editTask(id) {
  state.taskEdit = id;
  await loadTasks();
  go("tasks");
}

/** A task's list and day: the list name kept apart, since it may be Persian. */
function taskPlace(item) {
  return h("span", {}, h("bdi", { textContent: item.list }), item.when ? ` · ${item.when}` : "");
}

/** A link-like button, as used for "Get a key". */
function linkButton(label, onClick) {
  return h("button", { class: "btn btn-ghost btn-sm", type: "button", onclick: onClick, style: "padding:0;height:auto;color:var(--primary)" }, label, icon("external"));
}

function pageTasks() {
  const t = state.tasks;
  const head = pageHead(
    "Voice tasks",
    "Press the task shortcut and say a task, like “call Sara tomorrow at 3 about the contract”. Mirza saves it to Google Tasks right away, and to Google Calendar too when you say a time.",
  );
  if (!t) {
    loadTasks().then(() => state.section === "tasks" && renderPage());
    return [head, h("p", { class: "hint", textContent: "Loading…" })];
  }
  if (t.signed_in && !t.lists.length && !state.taskLists) loadTaskLists(false);
  const shortcutSet = state.cfg.shortcuts.some((x) => x.action === "task" && x.keys);
  return [
    head,
    state.taskEdit ? taskEditor(t) : null,
    !shortcutSet
      ? notice("Set a task shortcut to say tasks. Your dictation shortcuts stay as they are.", {
          actions: [button("Go to Shortcuts", () => go("shortcuts"), { size: "sm" })],
        })
      : null,
    googleGroup(t),
    t.signed_in ? listsGroup(t) : null,
    ...aiGroups(t),
    calendarGroup(),
    waitingGroup(t),
    recentGroup(t),
    tryTaskGroup(t),
  ];
}

function googleGroup(t) {
  const rows = [];
  if (t.client_id && !state.changingClient) {
    rows.push(
      row({
        label: "Google client",
        desc: t.built_in ? "The one that comes with this copy of Mirza." : h("span", { class: "mono", textContent: t.client_id }),
        control: state.signIn ? null : h(
          "div",
          { class: "row-control" },
          button(t.built_in ? "Use my own" : "Change", () => ((state.changingClient = true), renderPage()), { size: "sm" }),
          t.built_in ? null : button("", removeGoogleClient, { variant: "ghost", size: "icon", iconName: "x", title: "Remove this client" }),
        ),
      }),
    );
  } else {
    const box = h("textarea", {
      class: "input mono",
      rows: 3,
      spellcheck: false,
      placeholder: "{\"installed\":{\"client_id\":\"….apps.googleusercontent.com\", …}}",
      attrs: { "aria-label": "Google client file" },
    });
    const saveClient = async () => {
      if (!box.value.trim()) return toast("Paste the client file first", true);
      await call("google_client", { pasted: box.value });
      state.changingClient = false;
      await loadState();
      await loadTasks();
      toast("Client saved");
      renderPage();
    };
    rows.push(
      row({
        label: "Google client",
        desc: h(
          "span",
          {},
          "Mirza reaches your tasks through a Google Cloud client of your own. Create one with the steps in the guide, then open the JSON file Google gives you in a text editor and paste all of it here. ",
          linkButton("Open the guide", () => call("open_url", { url: TASKS_GUIDE })),
        ),
        stack: true,
        control: h(
          "div",
          { style: "display:grid;gap:8px" },
          box,
          h(
            "div",
            { class: "input-row", style: "justify-content:flex-start" },
            button("Save", saveClient, { variant: "primary" }),
            state.changingClient ? button("Cancel", () => ((state.changingClient = false), renderPage())) : null,
          ),
          t.client_id ? h("p", { class: "hint", textContent: "A new client signs you out of Google; sign in again afterwards." }) : null,
        ),
      }),
    );
  }
  if (t.client_id && !state.changingClient) rows.push(accountRow(t));
  return group(...rows);
}

function accountRow(t) {
  if (state.signIn) {
    const url = state.signIn.url;
    const address = h("input", { class: "input mono", placeholder: "http://127.0.0.1:…/?state=…&code=…", spellcheck: false, attrs: { "aria-label": "Address from the browser" } });
    const send = async () => {
      if (!address.value.trim()) return toast("Paste the address first", true);
      await call("google_sign_in_paste", { address: address.value });
      address.value = "";
    };
    address.addEventListener("keydown", (e) => e.key === "Enter" && send());
    return row({
      label: "Waiting for Google…",
      desc: "Finish signing in in your browser, then come back here.",
      stack: true,
      control: h(
        "div",
        { style: "display:grid;gap:8px" },
        url
          ? h(
              "p",
              { class: "hint" },
              "Browser didn't open? ",
              linkButton("Open the sign-in page", () => call("open_url", { url })),
              " or ",
              h("button", {
                class: "btn btn-ghost btn-sm",
                type: "button",
                style: "padding:0;height:auto;color:var(--primary)",
                textContent: "copy its address",
                onclick: async () => (await navigator.clipboard.writeText(url), toast("Copied")),
              }),
              ".",
            )
          : null,
        h("p", { class: "hint", textContent: "Signing in on another device? Paste here the address its browser ends up at (it starts with http://127.0.0.1)." }),
        h("div", { class: "input-row" }, address, button("Send", send)),
        h("div", {}, button("Cancel", () => invoke("google_sign_in_cancel"), { size: "sm" })),
      ),
    });
  }
  if (t.signed_in) {
    return row({
      label: "Google account",
      desc: state.signInNote || "Signed in. Mirza can add to your Google Tasks and Calendar.",
      control: button("Sign out", googleSignOut, { size: "sm" }),
    });
  }
  return row({
    label: "Google account",
    desc: "Not signed in. Tasks you say wait on this computer until you sign in.",
    control: button("Sign in", googleSignIn, { size: "sm", variant: "primary" }),
  });
}

async function googleSignIn() {
  state.signIn = { url: "" };
  state.signInNote = "";
  renderPage();
  try {
    const calendar = await invoke("google_sign_in");
    if (!calendar) state.signInNote = "Signed in without Calendar, so tasks with a time are saved as tasks only. Sign out and in again and tick the Calendar box to get events too.";
    toast("Signed in to Google");
    state.taskLists = null; // read them now
  } catch (e) {
    if (String(e) !== "Sign-in cancelled.") toast(String(e), true);
  }
  state.signIn = null;
  await loadTasks();
  if (state.section === "tasks") renderPage();
}

async function googleSignOut() {
  await call("google_sign_out");
  state.signInNote = "";
  state.taskLists = null;
  await loadTasks();
  toast("Signed out of Google");
  renderPage();
}

async function removeGoogleClient() {
  await call("google_client_remove");
  await loadState();
  await loadTasks();
  toast("Client removed");
  renderPage();
}

function listsGroup(t) {
  const tc = state.cfg.tasks;
  const lists = t.lists;
  const loading = state.taskLists && state.taskLists.loading;
  const error = state.taskLists && state.taskLists.error;
  const refresh = button(loading ? "Reading…" : "Read again", () => loadTaskLists(true), { size: "sm", iconName: "refresh" });
  refresh.disabled = !!loading;
  const hintRows = lists.map((l) => {
    const input = h("input", {
      class: "input",
      value: tc.list_hints[l.id] || "",
      placeholder: "Other names, separated by commas",
      attrs: { dir: "auto", "aria-label": `Other names for ${l.title}` },
      style: "width:260px",
    });
    input.addEventListener("change", () => save(`tasks.list_hints.${l.id}`, input.value.trim() || null));
    return row({ label: h("bdi", { textContent: l.title }), control: input });
  });
  const fallback = select({
    options: [{ value: "", label: "Your default list" }, ...lists.map((l) => ({ value: l.id, label: l.title }))],
    value: lists.some((l) => l.id === tc.default_list) ? tc.default_list : "",
    label: "When nothing fits",
    onChange: (v) => save("tasks.default_list", v),
  });
  return group(
    row({
      label: "Lists",
      desc: "Mirza puts each task in the list that fits what you said. Give a list other names, like a client or a project, so Mirza knows those tasks belong there too.",
      control: refresh,
    }),
    error ? h("div", { class: "row" }, notice(error, { error: true })) : null,
    !lists.length && !loading && !error ? h("p", { class: "row row-desc", textContent: "No lists read yet." }) : null,
    ...hintRows,
    lists.length ? row({ label: "When nothing fits", desc: "Tasks that fit no list go here, and Mirza says so.", control: fallback }) : null,
  );
}

/** The AI that reads tasks: its own choice, or the one AI revision uses. */
function aiGroups(t) {
  const tc = state.cfg.tasks;
  const rev = AI_PROVIDERS.find((x) => x.id === state.cfg.revision.provider);
  const a = AI_PROVIDERS.find((x) => x.id === (tc.provider || state.cfg.revision.provider));
  const choice = select({
    options: [
      { value: "", label: rev ? `Same as AI revision (${rev.name})` : "Same as AI revision (off)" },
      ...AI_PROVIDERS.map((x) => ({ value: x.id, label: x.name })),
    ],
    value: tc.provider,
    label: "AI for tasks",
    onChange: async (v) => {
      state.editingKey = false;
      state.taskTry = null;
      await save("tasks.provider", v, { rerender: false });
      await loadTasks();
      renderPage();
    },
  });
  const model = a && (state.cfg.ai[a.id].model || (a.models[0] ? a.models[0] : "the server's first model"));
  const missingKey = a && !a.optionalKey && !(state.keys[a.id] || {}).present;
  return [
    group(
      row({
        label: "AI",
        desc: a
          ? `Writes a short title, picks the list and reads the day and time. Uses ${model}, as set for ${a.name} in AI revision.`
          : "Without AI, tasks are saved word for word in the default list, with no day or time.",
        control: choice,
      }),
      missingKey
        ? row({
            label: `${a.name} API key`,
            stack: true,
            control: keyField(a, async () => (await loadTasks(), renderPage())),
          })
        : null,
      a && !missingKey && !t.ai && t.ai_problem ? h("div", { class: "row" }, notice(t.ai_problem, { error: true })) : null,
      row({
        label: "Time limit",
        desc: "If the AI hasn't answered by then, the task is saved as you said it.",
        control: h("div", { class: "row-control" }, numberInput(tc.timeout_sec, (v) => save("tasks.timeout_sec", v), { min: 5, max: 120, label: "Time limit in seconds" }), h("span", { class: "unit", textContent: "s" })),
      }),
    ),
  ];
}

function calendarGroup() {
  const tc = state.cfg.tasks;
  return group(
    row({
      label: "Event length",
      desc: "For tasks with a time, when you don't say how long.",
      control: h("div", { class: "row-control" }, numberInput(tc.event_minutes, (v) => save("tasks.event_minutes", v), { min: 5, max: 1440, step: 5, label: "Event length in minutes" }), h("span", { class: "unit", textContent: "min" })),
    }),
    row({
      label: "Due date for timed tasks",
      desc: "A task with a time is also due that day in Google Tasks. Turn off to keep the day only on its Calendar event.",
      control: switchEl(tc.timed_due_date, (on) => save("tasks.timed_due_date", on), "Due date for timed tasks"),
    }),
  );
}

function taskRow(item, { waiting }) {
  const undo = async () => {
    const r = await call("send", { request: { cmd: "undo_task", id: item.id } });
    if (!r.ok) return toast(r.message, true);
    toast(waiting ? "Deleted" : "Undone");
    await loadTasks();
    renderPage();
  };
  const read = !!item.draft; // a waiting task the AI hasn't read has no list yet
  return row({
    label: h("bdi", { textContent: item.title }),
    desc: waiting ? h("span", {}, read ? taskPlace(item) : null, read ? h("br") : null, item.error || "Waiting for its turn.") : taskPlace(item),
    control: h(
      "div",
      { class: "row-control" },
      button("Edit", () => editTask(item.id), { size: "sm", iconName: "pencil" }),
      waiting
        ? button("", undo, { variant: "ghost", size: "icon", iconName: "trash", title: "Delete; it won't be saved" })
        : button("Undo", undo, { size: "sm", iconName: "undo" }),
    ),
  });
}

function waitingGroup(t) {
  if (!t.waiting.length) return null;
  const retry = button("Try now", async () => {
    await call("send", { request: { cmd: "retry_tasks" } });
    toast("Trying again…");
    setTimeout(async () => (await loadTasks(), state.section === "tasks" && renderPage()), 3000);
  }, { size: "sm", iconName: "refresh" });
  return group(
    row({ label: "Waiting to be saved", desc: "Kept on this computer. Mirza tries again by itself.", control: retry }),
    ...t.waiting.map((w) => taskRow(w, { waiting: true })),
  );
}

function recentGroup(t) {
  return group(
    row({ label: "Saved lately", desc: "Undo deletes the task, and its Calendar event, from Google." }),
    ...(t.recent.length ? t.recent.map((s) => taskRow(s, { waiting: false })) : [h("p", { class: "row row-desc", textContent: "Nothing yet. The tasks you say show up here." })]),
  );
}

function tryTaskGroup(t) {
  const tc = state.cfg.tasks;
  const a = AI_PROVIDERS.find((x) => x.id === (tc.provider || state.cfg.revision.provider));
  if (!a) return null;
  const box = h("textarea", { class: "input", rows: 2, placeholder: "Type a task the way you'd say it.", value: state.taskTryText || "", attrs: { dir: "auto", "aria-label": "Task to try" } });
  box.addEventListener("input", () => (state.taskTryText = box.value));
  const result = h("div", {});
  const show = () => {
    const r = state.taskTry;
    if (!r) return result.replaceChildren();
    if (r.error) return result.replaceChildren(notice(r.error, { error: true }));
    result.replaceChildren(
      h(
        "div",
        { class: "transcript" },
        h(
          "div",
          {},
          h("p", {}, h("strong", { textContent: r.title, attrs: { dir: "auto" } })),
          h("p", {}, taskPlace(r)),
          r.note ? h("p", { textContent: r.note, attrs: { dir: "auto" } }) : null,
          ...r.guesses.map((g) => h("p", { class: "hint", textContent: `Guessed: ${g}`, attrs: { dir: "auto" } })),
        ),
      ),
      h("p", { class: "hint", textContent: `Took ${(r.ms / 1000).toFixed(1)} s. Nothing was saved.` }),
    );
  };
  show();
  const read = button("Read", async () => {
    const text = box.value.trim();
    if (!text) return toast("Type a task first", true);
    read.disabled = true;
    result.replaceChildren(h("p", { class: "hint", textContent: "Reading…" }));
    try {
      state.taskTry = await invoke("try_task", { text });
    } catch (e) {
      state.taskTry = { error: String(e) };
    }
    read.disabled = false;
    show();
  }, { variant: "primary", iconName: "sparkles" });
  return group(
    row({
      label: "Try it",
      desc: `Sent to ${a.id === "local" ? "your server" : a.name}, like the tasks you say. Nothing goes to Google.`,
      stack: true,
      control: h("div", { style: "display:grid;gap:8px" }, box, h("div", {}, read), result),
    }),
  );
}

/** The edit form, at the top of the page. */
function taskEditor(t) {
  const id = state.taskEdit;
  const item = t.recent.find((x) => x.id === id) || t.waiting.find((x) => x.id === id);
  const close = () => ((state.taskEdit = null), renderPage());
  if (!item) {
    return notice("That task isn't among Mirza's recent tasks any more; change it in Google Tasks.", {
      error: true,
      actions: [button("Close", close, { size: "sm" })],
    });
  }
  const d = item.draft || { title: item.text, note: "", list: "", date: null, time: null, minutes: 0 };
  const field = (props) => h("input", { class: "input", ...props });
  const title = field({ value: d.title, attrs: { dir: "auto", "aria-label": "Title" } });
  const note = field({ value: d.note, placeholder: "Extra details, if any", attrs: { dir: "auto", "aria-label": "Note" } });
  const day = field({ type: "date", value: d.date || "", attrs: { "aria-label": "Day" }, style: "flex:none;width:170px" });
  const time = field({ type: "time", value: (d.time || "").slice(0, 5), attrs: { "aria-label": "Time" }, style: "flex:none;width:120px" });
  const minutes = field({ type: "number", min: 0, max: 1440, step: 5, value: d.minutes || "", placeholder: String(state.cfg.tasks.event_minutes), attrs: { "aria-label": "Length in minutes" }, style: "flex:none;width:90px" });
  let list = t.lists.some((l) => l.id === d.list) ? d.list : "";
  const lists = select({
    options: [{ value: "", label: "Your default list" }, ...t.lists.map((l) => ({ value: l.id, label: l.title }))],
    value: list,
    label: "List",
    onChange: (v) => (list = v),
  });
  const saveEdit = async () => {
    const edit = {
      title: title.value.trim(),
      note: note.value.trim(),
      list,
      date: day.value || null,
      time: time.value ? `${time.value.slice(0, 5)}:00` : null,
      minutes: parseInt(minutes.value, 10) || 0,
    };
    if (!edit.title) return toast("A task needs a title", true);
    if (edit.date && !/^\d{4}-\d{2}-\d{2}$/.test(edit.date)) return toast("Write the day as YYYY-MM-DD", true);
    if (edit.time && !/^\d{2}:\d{2}:00$/.test(edit.time)) return toast("Write the time as HH:MM", true);
    if (edit.time && !edit.date) return toast("A time needs a day", true);
    saveBtn.disabled = true;
    try {
      const r = await call("send", { request: { cmd: "edit_task", id, edit } });
      if (!r.ok) return toast(r.message, true);
      toast(r.data && r.data.state === "saved" ? "Task changed" : "Changed; Mirza saves it when it can");
      state.taskEdit = null;
      await loadTasks();
      renderPage();
    } finally {
      saveBtn.disabled = false;
    }
  };
  const saveBtn = button("Save", saveEdit, { variant: "primary" });
  return h(
    "div",
    { class: "group", style: "border-color:var(--primary)" },
    row({ label: "Edit task", desc: h("span", {}, "You said: ", h("bdi", { textContent: item.text })) }),
    row({ label: "Title", stack: true, control: title }),
    row({ label: "Note", stack: true, control: note }),
    row({ label: "List", control: lists }),
    row({
      label: "When",
      desc: "Leave the time empty for a task without an event. The length is in minutes.",
      stack: true,
      control: h("div", { class: "input-row", style: "justify-content:flex-start;flex-wrap:wrap" }, day, time, minutes, h("span", { class: "unit", textContent: "min" })),
    }),
    h("div", { class: "row", style: "justify-content:flex-end" }, h("div", { class: "row-control" }, button("Cancel", close), saveBtn)),
  );
}

// ---- Typing -------------------------------------------------------------------------

function pageTyping() {
  const t = state.cfg.typing;
  const longOn = t.paste_over_chars > 0;
  const longSub = h(
    "div",
    { class: "sub" },
    "Longer than",
    numberInput(t.paste_over_chars || 200, (v) => save("typing.paste_over_chars", Math.max(1, v)), { min: 1, label: "Characters" }),
    h("span", { class: "unit", textContent: "characters" }),
  );
  return [
    pageHead("Typing", "Mirza types each phrase into the window you're in, as soon as the speech service is sure of it."),
    state.snap && state.snap.typing_problem ? notice(`Mirza can't type right now: ${state.snap.typing_problem}`, { error: true }) : null,
    group(
      row({
        label: "Paste shortcut",
        desc: "The keys that paste in your apps.",
        info: "Mirza pastes instead of typing when a character isn't on any of your keyboard layouts, or for long text. Most apps paste with Ctrl+V; many terminals need Ctrl+Shift+V. Shift+Insert works almost everywhere on Linux.",
        control: keyRecorder({ value: t.paste_key, label: "Paste shortcut", global: false, onChange: (v) => save("typing.paste_key", v.toLowerCase()) }),
      }),
      h(
        "div",
        {},
        row({
          label: "Paste long text",
          desc: "Faster than typing for long phrases.",
          control: switchEl(longOn, (on) => save("typing.paste_over_chars", on ? 200 : 0, { rerender: true }), "Paste long text"),
        }),
        longOn ? longSub : null,
      ),
      row({
        label: "Keep a copy in the clipboard",
        desc: "After each dictation, the whole text is also in your clipboard.",
        control: switchEl(t.keep_on_clipboard, (on) => save("typing.keep_on_clipboard", on), "Keep a copy in the clipboard"),
      }),
      row({
        label: "Persian letters",
        // Each letter is isolated so it doesn't reorder the English sentence around it.
        desc: h("span", {}, "Write ", h("bdi", { textContent: "ی" }), " and ", h("bdi", { textContent: "ک" }), " instead of the Arabic ", h("bdi", { textContent: "ي" }), " and ", h("bdi", { textContent: "ك" }), "."),
        info: "Some speech services write Persian with Arabic letters. Mirza changes them to the Persian ones, and Arabic digits to Persian digits.",
        control: switchEl(t.persian_letters, (on) => save("typing.persian_letters", on), "Persian letters"),
      }),
      state.snap && state.snap.focus_tracking
        ? row({
            label: "Pause if I switch windows",
            desc: "Text never lands in the wrong app.",
            info: "If you switch to another window while Mirza is typing, it stops typing and puts the rest in your clipboard. Go back to the first window to continue.",
            control: switchEl(t.follow_focus, (on) => save("typing.follow_focus", on), "Pause if I switch windows"),
          })
        : null,
    ),
  ];
}

// ---- Listening ----------------------------------------------------------------------

function pageListening() {
  const c = state.cfg;
  const options = [{ value: "", label: "System default" }, ...state.mics.map((m) => ({ value: m.id, label: m.name }))];
  if (c.mic_device && !state.mics.some((m) => m.id === c.mic_device)) options.push({ value: c.mic_device, label: c.mic_device });
  const silenceOn = c.silence_stop_sec > 0;
  const limitOn = c.max_session_sec > 0;
  return [
    pageHead("Listening", "Which microphone Mirza uses, and when it stops listening."),
    group(
      row({ label: "Microphone", stack: true, control: select({ label: "Microphone", value: c.mic_device, options, onChange: (v) => save("mic_device", v) }) }),
      h(
        "div",
        {},
        row({
          label: "Stop when I stop talking",
          desc: "Ends dictation after a stretch of silence.",
          control: switchEl(silenceOn, (on) => save("silence_stop_sec", on ? 60 : 0, { rerender: true }), "Stop when I stop talking"),
        }),
        silenceOn
          ? h(
              "div",
              { class: "sub" },
              "After",
              numberInput(c.silence_stop_sec, (v) => save("silence_stop_sec", Math.max(5, v)), { min: 5, label: "Seconds of silence" }),
              h("span", { class: "unit", textContent: "seconds of silence" }),
            )
          : null,
      ),
      h(
        "div",
        {},
        row({
          label: "Time limit",
          desc: "The longest one dictation can run.",
          control: switchEl(limitOn, (on) => save("max_session_sec", on ? 900 : 0, { rerender: true }), "Time limit"),
        }),
        limitOn
          ? h(
              "div",
              { class: "sub" },
              "Stop after",
              numberInput(Math.round(c.max_session_sec / 60), (v) => save("max_session_sec", Math.max(1, v) * 60), { min: 1, label: "Minutes" }),
              h("span", { class: "unit", textContent: "minutes" }),
            )
          : null,
      ),
    ),
  ];
}

// ---- Usage -----------------------------------------------------------------------------

function pageUsage() {
  const p = provider();
  const snap = state.snap;
  const loading = state.usageRefreshing || !!(snap && snap.usage_loading);
  const refresh = button(loading ? "Refreshing…" : "Refresh", refreshUsage, { size: "sm", iconName: "refresh" });
  refresh.disabled = loading;
  refresh.classList.toggle("loading", loading);
  const head = pageHead("Usage", `What you've spent on ${p.name}.`, p.id === "soniox" && snap ? refresh : null);
  if (p.id !== "soniox") {
    return [
      head,
      h(
        "div",
        { class: "empty-state" },
        h("p", { style: "margin:0 0 10px", textContent: `${p.name} doesn't share usage with apps, so Mirza can't show it.` }),
        button(`Open ${p.name} usage`, () => call("open_url", { url: p.usageUrl }), { iconName: "external", size: "sm" }),
      ),
    ];
  }
  const u = snap && snap.usage && snap.usage.provider === p.id ? snap.usage : null;
  const error = snap && snap.usage_error;
  const stat = (label, value) => h("div", { class: "stat" }, h("div", { class: "label", textContent: label }), h("div", { class: "value", textContent: value }));
  const fresh = state.usageFresh;
  state.usageFresh = false;
  return [
    head,
    u && error ? notice(`Usage wasn't updated: ${error}`, { error: true }) : null,
    u
      ? h(
          "div",
          { class: "stats" + (fresh ? " fresh" : "") },
          stat("This month", money(u.month)),
          stat("Today", money(u.today)),
          stat("Audio this month", `${Math.round(u.month_minutes)} min`),
          stat("Dictations", String(u.requests)),
        )
      : h("div", { class: "empty-state", style: "margin-bottom:16px", textContent: !snap ? "Start Mirza to see usage." : error ? `Couldn't get usage: ${error}` : "Loading usage…" }),
    u && snap.usage_updated ? h("p", { class: "fineprint", textContent: `Updated ${when(snap.usage_updated)}` }) : null,
    balanceGroup(p, u),
  ];
}

/** "at 14:32", or with the date when it wasn't today. */
function when(unixSecs) {
  const d = new Date(unixSecs * 1000);
  const time = d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  if (d.toDateString() === new Date().toDateString()) return `at ${time}`;
  return `on ${d.toLocaleDateString(undefined, { month: "short", day: "numeric" })} at ${time}`;
}

/** Asks for fresh usage, waits for it, and says how it went. */
async function refreshUsage() {
  if (state.usageRefreshing) return;
  state.usageRefreshing = true;
  renderPage();
  try {
    const r = await call("send", { request: { cmd: "refresh_usage" } });
    if (!r.ok) throw new Error(r.message);
    const started = Date.now();
    let snap = null;
    do {
      await new Promise((done) => setTimeout(done, 400));
      snap = await invoke("snapshot").catch(() => null);
    } while (snap && snap.usage_loading && Date.now() - started < 60000);
    if (!snap) throw new Error("Mirza isn't running");
    state.snap = snap;
    lastSnap = JSON.stringify(snap); // so the next poll doesn't redraw over the highlight
    if (snap.usage_error) {
      toast(`Usage wasn't updated: ${snap.usage_error}`, true);
    } else {
      state.usageFresh = true;
      toast("Usage updated");
    }
  } catch (e) {
    if (e instanceof Error) toast(e.message, true); // call() already showed its own errors
  } finally {
    state.usageRefreshing = false;
    if (state.section === "usage") renderPage();
  }
}

/** Remaining balance: Soniox doesn't report it, so the user enters it once and
 *  Mirza subtracts what's been used since. */
function balanceGroup(p, u) {
  const credit = (state.cfg.credit || {})[p.id];
  const tracking = credit && credit.amount > 0 && credit.since;
  if (tracking && !state.editingBalance) {
    const left = u && u.since_credit != null ? money(credit.amount - u.since_credit) : "…";
    const since = new Date(credit.since + "T00:00:00").toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
    return group(
      h(
        "div",
        { class: "row" },
        h(
          "div",
          { class: "row-text" },
          h("div", { class: "row-label", textContent: "Balance left" }),
          h("div", { style: "font-size:26px;font-weight:600;letter-spacing:-0.02em;margin:2px 0", textContent: left }),
          h("p", { class: "row-desc", textContent: `You had ${money(credit.amount)} on ${since}. Mirza subtracts what you've used since then.` }),
        ),
        h(
          "div",
          { class: "row-control" },
          button("Update", () => ((state.editingBalance = true), renderPage()), { size: "sm" }),
          button("Stop tracking", async () => {
            await save(`credit.${p.id}.amount`, null, { quiet: true });
            await save(`credit.${p.id}.since`, null, { rerender: true });
          }, { size: "sm", variant: "ghost" }),
        ),
      ),
    );
  }
  const amount = h("input", { class: "input", type: "number", min: 0, step: "0.01", placeholder: "0.00", value: tracking ? credit.amount : "", attrs: { "aria-label": "Current balance in dollars" }, style: "max-width:160px" });
  const saveBalance = async () => {
    const v = parseFloat(amount.value);
    if (!(v > 0)) return toast("Enter the balance in dollars", true);
    const d = new Date();
    const today = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
    state.editingBalance = false;
    await save(`credit.${p.id}.amount`, v, { quiet: true });
    await save(`credit.${p.id}.since`, today, { rerender: true });
    call("send", { request: { cmd: "refresh_usage" } }).then(() => setTimeout(poll, 1500), () => {});
  };
  amount.addEventListener("keydown", (e) => e.key === "Enter" && saveBalance());
  return group(
    row({
      label: "Track your balance",
      desc: `${p.name} doesn't tell apps how much credit you have left. Enter the balance your ${p.name} account shows now, and Mirza will count down from it.`,
      stack: true,
      control: h(
        "div",
        { class: "input-row", style: "justify-content:flex-start" },
        h("span", { class: "unit", textContent: "$" }),
        amount,
        button("Save", saveBalance, { variant: "primary" }),
        state.editingBalance ? button("Cancel", () => ((state.editingBalance = false), renderPage())) : null,
      ),
    }),
  );
}

// ---- Recent text ---------------------------------------------------------------------

function pageHistory() {
  const raw = (state.snap && state.snap.history_raw) || [];
  const items = ((state.snap && state.snap.history) || []).map((text, i) => ({ text, raw: raw[i] || "" })).reverse();
  const clear = button("Clear", async () => {
    const r = await call("send", { request: { cmd: "clear_history" } });
    if (!r.ok) return toast(r.message, true);
    await poll();
    renderPage();
    toast("Recent text cleared");
  }, { size: "sm", iconName: "trash", variant: "danger" });
  return [
    pageHead("Recent text", "The last things you dictated, in case something didn't get typed. Kept in memory only, never saved to disk.", items.length ? clear : null),
    items.length
      ? h(
          "div",
          { class: "transcripts" },
          ...items.map((t) => {
            const copy = (text, title) =>
              button("", async () => {
                await navigator.clipboard.writeText(text);
                toast("Copied");
              }, { variant: "ghost", size: "icon", iconName: "copy", title });
            const before = t.raw
              ? h(
                  "details",
                  { class: "hint" },
                  h("summary", { textContent: "Before AI revision" }),
                  h("div", { class: "transcript" }, h("p", { textContent: t.raw, attrs: { dir: "auto" } }), copy(t.raw, "Copy the original")),
                )
              : null;
            return h(
              "div",
              {},
              h("div", { class: "transcript" }, h("p", { textContent: t.text, attrs: { dir: "auto" } }), copy(t.text, "Copy")),
              before,
            );
          }),
        )
      : h("div", { class: "empty-state", textContent: "Nothing yet. What you dictate shows up here." }),
    h("div", { style: "height:16px" }),
    group(
      row({
        label: "How many to keep",
        desc: "0 keeps none.",
        control: numberInput(state.cfg.history_size, (v) => save("history_size", v), { max: 200, label: "How many to keep" }),
      }),
    ),
  ];
}

// ---- General -------------------------------------------------------------------------

function pageGeneral() {
  const c = state.cfg;
  const update = state.snap && state.snap.update;
  const testSound = () => call("send", { request: { cmd: "test_sound" } }).catch(() => {});
  const proxy = h("input", { class: "input mono", value: c.proxy, placeholder: "Use the system proxy", spellcheck: false, attrs: { "aria-label": "Proxy" } });
  const proxyStatus = h("p", { class: "row-desc", style: "margin-top:8px" });
  const showProxy = () =>
    invoke("proxy_in_use")
      .then((p) => {
        proxyStatus.style.color = "";
        proxyStatus.textContent = p ? `In use now: ${p}` : "In use now: none, Mirza connects directly.";
      })
      .catch((e) => {
        proxyStatus.style.color = "var(--destructive)";
        proxyStatus.textContent = String(e);
      });
  showProxy();
  proxy.addEventListener("change", () => save("proxy", proxy.value.trim()).then(showProxy));
  return [
    pageHead("General"),
    group(
      row({ label: "Start when I log in", control: switchEl(c.start_on_login, (on) => save("start_on_login", on), "Start when I log in") }),
      row({ label: "Notifications", desc: "Short messages when dictation starts and stops. Errors always show.", control: switchEl(c.notifications, (on) => save("notifications", on), "Notifications") }),
      h(
        "div",
        {},
        row({ label: "Sounds", desc: "A short sound when dictation starts and stops.", control: switchEl(c.sounds, (on) => save("sounds", on, { rerender: true }), "Sounds") }),
        c.sounds
          ? h(
              "div",
              { class: "sub" },
              "Volume",
              numberInput(c.sound_volume, (v) => save("sound_volume", v).then(testSound), { min: 1, max: 100, label: "Sound volume" }),
              h("span", { class: "unit", textContent: "%" }),
              button("Play", testSound, { size: "sm", iconName: "play" }),
            )
          : null,
      ),
    ),
    group(
      h(
        "div",
        {},
        row({
          label: "Check for updates",
          desc: update ? `Version ${update.version} is out. You have ${state.version}.` : `Once a day, Mirza looks for a newer version on GitHub. You have ${state.version}.`,
          info: "Mirza only asks GitHub for the latest version number. Nothing about you or what you say is sent.",
          control: switchEl(c.check_updates, (on) => save("check_updates", on, { rerender: true }), "Check for updates"),
        }),
        update
          ? h("div", { class: "sub" }, button(`Download ${update.version}`, () => call("open_url", { url: update.url }), { size: "sm", variant: "primary", iconName: "download" }))
          : null,
      ),
    ),
    group(
      row({
        label: "Proxy",
        desc: "Only needed if the speech service is blocked where you are.",
        info: "Leave it empty to use your system's proxy: the one your VPN app sets in Windows or macOS settings, or HTTPS_PROXY. Type none to always connect directly. Formats: http://host:port, http://user:password@host:port, socks5://host:port.",
        stack: true,
        control: h("div", {}, proxy, proxyStatus),
      }),
    ),
    group(
      row({
        label: "Settings file",
        desc: "Everything here is also in this file, for editing by hand.",
        stack: true,
        control: h("div", { class: "input-row" }, h("span", { class: "path mono", style: "flex:1", textContent: state.configPath }), button("Open", () => call("open_config"), { iconName: "file", size: "sm" })),
      }),
      row({
        label: "Quit Mirza",
        desc: "Shortcuts stop working until you start it again.",
        control: button("Quit", async () => {
          await call("send", { request: { cmd: "quit" } }).catch(() => {});
          setTimeout(poll, 500);
        }, { iconName: "power", size: "sm", variant: "danger" }),
      }),
    ),
  ];
}

// ---- Live updates ----------------------------------------------------------------------

let lastSnap = "";
let taskPolls = 0;
async function poll() {
  try {
    state.snap = await invoke("snapshot");
  } catch {
    state.snap = null;
  }
  renderHeader();
  // The task queue lives on disk: look again now and then while it's shown.
  if (state.section === "tasks" && state.cfg && ++taskPolls % 5 === 0 && !state.taskEdit) {
    const before = JSON.stringify(state.tasks);
    await loadTasks();
    if (JSON.stringify(state.tasks) !== before) lastSnap = "";
  }
  const key = JSON.stringify(state.snap);
  if (key === lastSnap) return;
  lastSnap = key;
  renderNav();
  // Refresh the page when its live data changed, unless the user is busy with it.
  const busy = document.querySelector("#content :focus, .recorder.recording, .menu");
  if (!busy && state.cfg) renderPage();
}

(async function main() {
  await loadState();
  state.languages = await invoke("languages").catch(() => []);
  state.mics = await invoke("list_mics").catch(() => []);
  const fromHash = location.hash.slice(1);
  let saved = null;
  try {
    saved = localStorage.getItem("mirza.section");
  } catch {}
  state.section = SECTIONS.some((s) => s.id === fromHash) ? fromHash : SECTIONS.some((s) => s.id === saved) ? saved : "provider";
  await loadTasks();
  // Opened from a task's Edit button.
  const editId = await invoke("edit_task_on_start").catch(() => null);
  if (editId) Object.assign(state, { section: "tasks", taskEdit: editId });
  const { listen } = window.__TAURI__.event;
  listen("edit-task", (e) => editTask(e.payload));
  listen("google-sign-in", (e) => {
    if (!state.signIn) return;
    state.signIn.url = e.payload;
    if (state.section === "tasks") renderPage();
  });
  await poll();
  renderNav();
  renderPage();
  setInterval(poll, 1000);
  if (await invoke("debug_enabled").catch(() => false)) setTimeout(reportLayout, 500);
})();

/** Debug aid: logs the page and window widths and anything wider than the window. */
function reportLayout() {
  const cw = document.documentElement.clientWidth;
  const wide = [...document.querySelectorAll("body *")]
    .map((e) => [e, e.getBoundingClientRect()])
    .filter(([, r]) => r.right > cw + 1)
    .slice(0, 8)
    .map(([e, r]) => `${e.tagName.toLowerCase()}${e.id ? "#" + e.id : ""}.${e.className} right=${Math.round(r.right)} w=${Math.round(r.width)}`);
  invoke("log", { msg: JSON.stringify({ innerWidth, clientWidth: cw, scrollWidth: document.documentElement.scrollWidth, dpr: devicePixelRatio, wide }) });
}

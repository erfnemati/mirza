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

const SECTIONS = [
  { id: "provider", label: "Speech service", icon: "waves" },
  { id: "shortcuts", label: "Shortcuts", icon: "keyboard" },
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
  snap: null,
  section: "provider",
  models: {}, // provider -> {list, error, loading}
  languages: [],
  mics: [],
  editingKey: false,
  editingBalance: false,
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
  const build = { provider: pageProvider, shortcuts: pageShortcuts, typing: pageTyping, listening: pageListening, usage: pageUsage, history: pageHistory, general: pageGeneral }[state.section];
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

  // API key: hidden once saved, with a button to change it.
  let keyControl;
  if (key.present && !state.editingKey) {
    keyControl = h(
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
  } else {
    const input = h("input", { class: "input mono", type: "password", placeholder: `Paste your ${p.name} API key`, spellcheck: false, attrs: { autocomplete: "off", "aria-label": "API key" } });
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
      delete state.models[p.id];
      renderNav();
      renderPage();
      call("send", { request: { cmd: "refresh_usage" } }).then(() => setTimeout(poll, 1500), () => {});
    };
    input.addEventListener("keydown", (e) => e.key === "Enter" && saveKey());
    const link = h("button", { class: "btn btn-ghost btn-sm", type: "button", onclick: () => call("open_url", { url: p.keyUrl }), style: "padding:0;height:auto;color:var(--primary)" }, `Get a ${p.name} key`, icon("external"));
    keyControl = h(
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
    setTimeout(() => state.editingKey && input.focus(), 0);
  }

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
  const find = (s) => list.find((x) => x.action === s.action && (s.action !== "dictate" || (x.mode || "toggle") === s.mode));

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
    h("div", { class: "group" }, rows[0], h("div", {}, rows[1], holdSub), ...rows.slice(2)),
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
        control: keyRecorder({ value: t.paste_key, label: "Paste shortcut", onChange: (v) => save("typing.paste_key", v.toLowerCase()) }),
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
  const refresh = button("Refresh", () => call("send", { request: { cmd: "refresh_usage" } }).then(() => setTimeout(poll, 1500)), { size: "sm", iconName: "refresh" });
  const head = pageHead("Usage", `What you've spent on ${p.name}.`, p.id === "soniox" && state.snap ? refresh : null);
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
  const u = state.snap && state.snap.usage && state.snap.usage.provider === p.id ? state.snap.usage : null;
  const stat = (label, value) => h("div", { class: "stat" }, h("div", { class: "label", textContent: label }), h("div", { class: "value", textContent: value }));
  return [
    head,
    u
      ? h(
          "div",
          { class: "stats" },
          stat("This month", money(u.month)),
          stat("Today", money(u.today)),
          stat("Audio this month", `${Math.round(u.month_minutes)} min`),
          stat("Dictations", String(u.requests)),
        )
      : h("div", { class: "empty-state", style: "margin-bottom:16px", textContent: state.snap ? "Loading usage…" : "Start Mirza to see usage." }),
    balanceGroup(p, u),
  ];
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
  const items = ((state.snap && state.snap.history) || []).slice().reverse();
  return [
    pageHead("Recent text", "The last things you dictated, in case something didn't get typed. Kept in memory only, never saved to disk."),
    items.length
      ? h(
          "div",
          { class: "transcripts" },
          ...items.map((t) =>
            h(
              "div",
              { class: "transcript" },
              h("p", { textContent: t, attrs: { dir: "auto" } }),
              button("", async () => {
                await navigator.clipboard.writeText(t);
                toast("Copied");
              }, { variant: "ghost", size: "icon", iconName: "copy", title: "Copy" }),
            ),
          ),
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
  const proxy = h("input", { class: "input mono", value: c.proxy, placeholder: "Use the system proxy", spellcheck: false, attrs: { "aria-label": "Proxy" } });
  proxy.addEventListener("change", () => save("proxy", proxy.value.trim()));
  return [
    pageHead("General"),
    group(
      row({ label: "Start when I log in", control: switchEl(c.start_on_login, (on) => save("start_on_login", on), "Start when I log in") }),
      row({ label: "Notifications", desc: "Short messages when dictation starts and stops. Errors always show.", control: switchEl(c.notifications, (on) => save("notifications", on), "Notifications") }),
    ),
    group(
      row({
        label: "Proxy",
        desc: "Only needed if the speech service is blocked where you are.",
        info: "Leave it empty to use your system's HTTPS_PROXY setting, or type none to always connect directly. Formats: http://host:port, http://user:password@host:port, socks5://host:port.",
        stack: true,
        control: proxy,
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
async function poll() {
  try {
    state.snap = await invoke("snapshot");
  } catch {
    state.snap = null;
  }
  renderHeader();
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

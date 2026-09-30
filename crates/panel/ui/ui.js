// Small UI toolkit for the settings window: elements, switches, dropdowns,
// a combobox, a multi-select, a key recorder, tooltips and toasts.

/** Creates an element. `props` sets properties; `on*` adds listeners;
 *  `attrs` sets attributes; `class` is the class name. */
function h(tag, props = {}, ...children) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props || {})) {
    if (v == null || v === false) continue;
    if (k === "class") el.className = v;
    else if (k === "attrs") for (const [a, av] of Object.entries(v)) av != null && el.setAttribute(a, av);
    else if (k.startsWith("on") && typeof v === "function") el.addEventListener(k.slice(2).toLowerCase(), v);
    else el[k] = v;
  }
  for (const c of children.flat()) if (c != null && c !== false) el.append(c);
  return el;
}

// ---- Toast and tooltip -------------------------------------------------------

function toast(msg, error = false) {
  const t = document.getElementById("toast");
  t.textContent = msg;
  t.className = "toast show" + (error ? " error" : "");
  clearTimeout(toast.timer);
  toast.timer = setTimeout(() => (t.className = "toast" + (error ? " error" : "")), error ? 4500 : 1600);
}

/** An (i) button that explains something on hover or focus. */
function infoTip(text) {
  return h("button", { class: "info", type: "button", attrs: { "aria-label": text, "data-tip": text } }, icon("info"));
}

(function tooltips() {
  let current = null;
  const show = (el) => {
    const tip = document.getElementById("tooltip");
    tip.textContent = el.dataset.tip;
    tip.classList.add("show");
    const r = el.getBoundingClientRect();
    const t = tip.getBoundingClientRect();
    let left = Math.min(Math.max(8, r.left + r.width / 2 - t.width / 2), innerWidth - t.width - 8);
    let top = r.top - t.height - 8;
    if (top < 8) top = r.bottom + 8;
    tip.style.left = left + "px";
    tip.style.top = top + "px";
    current = el;
  };
  const hide = () => {
    document.getElementById("tooltip").classList.remove("show");
    current = null;
  };
  document.addEventListener("mouseover", (e) => {
    const el = e.target.closest && e.target.closest("[data-tip]");
    if (el && el !== current) show(el);
    else if (!el && current) hide();
  });
  document.addEventListener("focusin", (e) => e.target.dataset && e.target.dataset.tip && show(e.target));
  document.addEventListener("focusout", hide);
  document.addEventListener("scroll", hide, true);
})();

// ---- Layout helpers ------------------------------------------------------------

/** A settings row: label and description on the left, a control on the right.
 *  With `stack`, the control goes under the text at full width. */
function row({ label, desc, info, control, stack = false, id }) {
  const text = h(
    "div",
    { class: "row-text" },
    h("div", { class: "row-label" }, label, info ? infoTip(info) : null),
    desc ? h("p", { class: "row-desc" }, desc) : null,
  );
  if (stack) return h("div", { class: "row stack", id }, text, control);
  return h("div", { class: "row", id }, text, h("div", { class: "row-control" }, control));
}

function group(...rows) {
  return h("div", { class: "group" }, ...rows);
}

function pageHead(title, desc, action) {
  return h("div", { class: "page-head" }, h("div", {}, h("h1", { textContent: title }), desc ? h("p", { textContent: desc }) : null), action || null);
}

function notice(text, { error = false, actions = [] } = {}) {
  return h(
    "div",
    { class: "notice" + (error ? " error" : "") },
    icon("alert"),
    h(
      "div",
      { class: "notice-body" },
      h("p", { textContent: text }),
      actions.length ? h("div", { class: "notice-actions" }, ...actions) : null,
    ),
  );
}

function button(label, onClick, { variant = "", size = "", iconName, title } = {}) {
  const cls = ["btn", variant && "btn-" + variant, size && "btn-" + size].filter(Boolean).join(" ");
  return h("button", { class: cls, type: "button", onclick: onClick, attrs: title ? { "data-tip": title, "aria-label": title } : null }, iconName ? icon(iconName) : null, label || null);
}

// ---- Switch and number ------------------------------------------------------------

function switchEl(checked, onChange, label) {
  const sw = h("button", { class: "switch", type: "button", attrs: { role: "switch", "aria-checked": String(!!checked), "aria-label": label } });
  sw.onclick = () => {
    const on = sw.getAttribute("aria-checked") !== "true";
    sw.setAttribute("aria-checked", String(on));
    onChange(on);
  };
  return sw;
}

function numberInput(value, onCommit, { min = 0, max, step = 1, label } = {}) {
  const inp = h("input", { class: "input num", type: "number", value: value ?? "", min, max, step, attrs: { "aria-label": label } });
  inp.addEventListener("change", () => {
    let v = parseInt(inp.value, 10);
    if (!Number.isFinite(v)) v = min;
    v = Math.max(min, max != null ? Math.min(max, v) : v);
    inp.value = v;
    onCommit(v);
  });
  return inp;
}

// ---- Dropdowns -----------------------------------------------------------------------

let openMenu = null;
function closeMenus() {
  if (openMenu) {
    openMenu.close();
    openMenu = null;
  }
}
document.addEventListener("mousedown", (e) => {
  if (openMenu && !openMenu.root.contains(e.target)) closeMenus();
});

/** A list popover under `root`. `items` are {value, label, meta, selected}. */
function menuFor(root, { items, onPick, empty = "Nothing found" }) {
  let active = 0;
  const menu = h("div", { class: "menu", attrs: { role: "listbox" } });
  const render = () => {
    menu.replaceChildren();
    if (!items.length) return menu.append(h("div", { class: "menu-empty", textContent: empty }));
    items.forEach((it, i) => {
      const el = h(
        "div",
        { class: "menu-item" + (i === active ? " active" : ""), attrs: { role: "option", "aria-selected": String(!!it.selected) } },
        h("span", { textContent: it.label }),
        it.meta ? h("span", { class: "meta", textContent: it.meta }) : it.selected ? h("span", { class: "check" }, icon("check")) : null,
      );
      el.addEventListener("mousedown", (e) => {
        e.preventDefault();
        onPick(it);
      });
      el.addEventListener("mousemove", () => {
        if (active !== i) {
          active = i;
          render();
        }
      });
      menu.append(el);
    });
    menu.children[active]?.scrollIntoView({ block: "nearest" });
  };
  render();
  root.append(menu);
  const api = {
    root,
    close: () => menu.remove(),
    setItems(next) {
      items = next;
      active = Math.min(active, Math.max(0, items.length - 1));
      render();
    },
    key(e) {
      if (e.key === "ArrowDown") active = Math.min(items.length - 1, active + 1);
      else if (e.key === "ArrowUp") active = Math.max(0, active - 1);
      else if (e.key === "Enter" && items[active]) {
        e.preventDefault();
        onPick(items[active]);
        return true;
      } else return false;
      e.preventDefault();
      render();
      return true;
    },
  };
  closeMenus();
  openMenu = api;
  return api;
}

/** A dropdown with fixed choices: {value, label, meta}. */
function select({ options, value, onChange, label }) {
  const root = h("div", { class: "dropdown" });
  const current = () => options.find((o) => o.value === value) || { label: value || "Choose…" };
  const btn = h("button", { class: "field", type: "button", attrs: { "aria-label": label, "aria-haspopup": "listbox" } });
  const paint = () => btn.replaceChildren(h("span", { class: "value", textContent: current().label }), h("span", { class: "chev" }, icon("chevron")));
  paint();
  btn.onclick = () => {
    if (openMenu && openMenu.root === root) return closeMenus();
    menuFor(root, {
      items: options.map((o) => ({ ...o, selected: o.value === value })),
      onPick: (it) => {
        value = it.value;
        paint();
        closeMenus();
        onChange(it.value);
      },
    });
  };
  btn.onkeydown = (e) => {
    if (openMenu && openMenu.root === root) {
      if (e.key === "Escape") closeMenus();
      else openMenu.key(e);
    } else if (e.key === "ArrowDown") btn.click();
  };
  root.append(btn);
  return root;
}

/** A text field with suggestions; any typed value is allowed. Suggestions
 *  are {value, label, meta}. `onCommit` gets the value on Enter, pick or blur. */
function combobox({ value, suggestions, onCommit, placeholder, label }) {
  const root = h("div", { class: "dropdown" });
  const input = h("input", { value: value || "", placeholder, spellcheck: false, attrs: { "aria-label": label, autocomplete: "off" } });
  const field = h("div", { class: "field mono" }, input, h("span", { class: "chev" }, icon("chevron")));
  let committed = value || "";
  const commit = (v) => {
    v = v.trim();
    closeMenus();
    input.value = v;
    if (v && v !== committed) {
      committed = v;
      onCommit(v);
    }
  };
  const items = () => {
    const q = input.value.trim().toLowerCase();
    const all = typeof suggestions === "function" ? suggestions() : suggestions;
    const list = q && q !== committed.toLowerCase() ? all.filter((s) => (s.value + " " + (s.label || "")).toLowerCase().includes(q)) : all;
    return list.map((s) => ({ ...s, selected: s.value === committed }));
  };
  const open = () => menuFor(root, { items: items(), onPick: (it) => commit(it.value), empty: "Press Enter to use this name" });
  field.onclick = () => {
    input.focus();
    if (!(openMenu && openMenu.root === root)) open();
  };
  input.addEventListener("input", () => (openMenu && openMenu.root === root ? openMenu.setItems(items()) : open()));
  input.addEventListener("keydown", (e) => {
    if (e.key === "Escape") {
      input.value = committed;
      return closeMenus();
    }
    if (openMenu && openMenu.root === root && openMenu.key(e)) return;
    if (e.key === "Enter") commit(input.value);
    if (e.key === "ArrowDown") open();
  });
  input.addEventListener("blur", () => setTimeout(() => document.activeElement !== input && commit(input.value), 120));
  root.append(field);
  root.refresh = (next) => {
    suggestions = next;
    if (openMenu && openMenu.root === root) openMenu.setItems(items());
  };
  return root;
}

/** Chips with a search box: pick several from `options` ({value, label}). */
function multiSelect({ options, values, onChange, placeholder, label }) {
  const root = h("div", { class: "dropdown" });
  const input = h("input", { placeholder, spellcheck: false, attrs: { "aria-label": label, autocomplete: "off" } });
  const field = h("div", { class: "field multi" });
  const nameOf = (v) => (options.find((o) => o.value === v) || { label: v }).label;
  const paint = () => {
    field.replaceChildren(
      ...values.map((v) =>
        h(
          "span",
          { class: "chip" },
          nameOf(v),
          h("button", { type: "button", attrs: { "aria-label": `Remove ${nameOf(v)}` }, onclick: (e) => (e.stopPropagation(), set(values.filter((x) => x !== v))) }, icon("x")),
        ),
      ),
      input,
    );
    input.placeholder = values.length ? "Add…" : placeholder;
  };
  const set = (next) => {
    values = next;
    paint();
    onChange(values);
    if (openMenu && openMenu.root === root) openMenu.setItems(items());
    input.focus();
  };
  // Best matches first: the code itself, then names starting with the text,
  // then names with a word starting with it, then anything containing it.
  const rank = (o, q) => {
    const name = o.label.toLowerCase();
    if (o.value.toLowerCase() === q) return 0;
    if (name.startsWith(q)) return 1;
    if (name.split(/[\s(-]+/).some((w) => w.startsWith(q))) return 2;
    if (name.includes(q)) return 3;
    return 9;
  };
  const items = () => {
    const q = input.value.trim().toLowerCase();
    return options
      .filter((o) => !values.includes(o.value))
      .map((o) => ({ o, r: q ? rank(o, q) : 5 }))
      .filter((x) => x.r < 9)
      .sort((a, b) => a.r - b.r)
      .map(({ o }) => ({ value: o.value, label: o.label, meta: o.value }));
  };
  const open = () =>
    menuFor(root, {
      items: items(),
      onPick: (it) => {
        input.value = "";
        set([...values, it.value]);
      },
    });
  field.onclick = () => {
    input.focus();
    if (!(openMenu && openMenu.root === root)) open();
  };
  input.addEventListener("input", () => (openMenu && openMenu.root === root ? openMenu.setItems(items()) : open()));
  input.addEventListener("keydown", (e) => {
    if (e.key === "Escape") return closeMenus();
    if (e.key === "Backspace" && !input.value && values.length) return set(values.slice(0, -1));
    if (openMenu && openMenu.root === root) openMenu.key(e);
    else if (e.key === "ArrowDown") open();
  });
  paint();
  root.append(field);
  return root;
}

// ---- Keys ---------------------------------------------------------------------------------

const PLATFORM = navigator.userAgent.includes("Mac") ? "macos" : navigator.userAgent.includes("Windows") ? "windows" : "linux";

/** How a key name shows on this system's keyboards. */
function keyLabel(k) {
  const lower = k.toLowerCase();
  const side = lower.startsWith("left") ? "Left " : lower.startsWith("right") ? "Right " : "";
  const base = lower.replace(/^(left|right)/, "");
  const names = {
    ctrl: PLATFORM === "macos" ? "Control" : "Ctrl",
    control: "Ctrl",
    alt: PLATFORM === "macos" ? "Option" : "Alt",
    option: "Option",
    shift: "Shift",
    meta: PLATFORM === "macos" ? "Cmd" : PLATFORM === "windows" ? "Win" : "Meta",
    super: "Meta",
    cmd: "Cmd",
    space: "Space",
    enter: "Enter",
    escape: "Esc",
    insert: "Insert",
    delete: "Delete",
    backspace: "Backspace",
    tab: "Tab",
    pageup: "Page Up",
    pagedown: "Page Down",
  };
  return side + (names[base] || (base.length === 1 ? base.toUpperCase() : base[0].toUpperCase() + base.slice(1)));
}

function kbds(combo) {
  return h("span", { class: "kbd-group" }, ...combo.split("+").filter(Boolean).map((k) => h("kbd", { textContent: keyLabel(k.trim()) })));
}

const MODIFIER_CODES = {
  ControlLeft: "LeftCtrl", ControlRight: "RightCtrl", AltLeft: "LeftAlt", AltRight: "RightAlt",
  ShiftLeft: "LeftShift", ShiftRight: "RightShift", MetaLeft: "LeftMeta", MetaRight: "RightMeta",
  OSLeft: "LeftMeta", OSRight: "RightMeta",
};
const KEY_CODES = {
  Space: "Space", Enter: "Enter", Tab: "Tab", Backquote: "`", Minus: "-", Equal: "=", BracketLeft: "[",
  BracketRight: "]", Backslash: "\\", Semicolon: ";", Quote: "'", Comma: ",", Period: ".", Slash: "/",
  Insert: "Insert", Delete: "Delete", Home: "Home", End: "End", PageUp: "PageUp", PageDown: "PageDown",
  ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right", Pause: "Pause",
  ScrollLock: "ScrollLock", CapsLock: "CapsLock", Backspace: "Backspace",
};
function codeName(code) {
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  if (/^F\d+$/.test(code)) return code;
  return KEY_CODES[code] || null;
}

function suspendShortcuts(suspend) {
  window.__TAURI__.core.invoke("send", { request: { cmd: "suspend_shortcuts", suspend } }).catch(() => {});
}

/** A button that shows a key combination and records a new one when clicked.
 *  `lone` allows a single modifier (e.g. RightCtrl). Escape cancels. */
function keyRecorder({ value, onChange, lone = false, emptyText = "Set shortcut", label }) {
  const btn = h("button", { class: "btn recorder", type: "button", attrs: { "aria-label": label } });
  const paint = () => {
    btn.classList.toggle("empty", !value);
    btn.classList.remove("recording");
    btn.replaceChildren(value ? kbds(value) : emptyText);
  };
  paint();
  btn.onclick = () => {
    if (btn.classList.contains("recording")) return;
    btn.classList.add("recording");
    // Global shortcuts would catch the keys before this window sees them.
    suspendShortcuts(true);
    btn.replaceChildren(h("span", { class: "pulse" }), "Press the keys…");
    let loneCode = null;
    const done = (v) => {
      suspendShortcuts(false);
      removeEventListener("keydown", down, true);
      removeEventListener("keyup", up, true);
      removeEventListener("mousedown", away, true);
      if (v) {
        value = v;
        onChange(v);
      }
      paint();
    };
    const down = (e) => {
      e.preventDefault();
      e.stopPropagation();
      if (e.repeat) return;
      if (MODIFIER_CODES[e.code]) {
        loneCode = e.code;
        return;
      }
      loneCode = null;
      const plain = !e.ctrlKey && !e.altKey && !e.shiftKey && !e.metaKey;
      if (e.code === "Escape" && plain) return done(null);
      const k = codeName(e.code);
      if (!k) return;
      const mods = [];
      if (e.ctrlKey) mods.push("Ctrl");
      if (e.altKey) mods.push("Alt");
      if (e.shiftKey) mods.push("Shift");
      if (e.metaKey) mods.push("Meta");
      done([...mods, k].join("+"));
    };
    const up = (e) => {
      e.preventDefault();
      if (loneCode && e.code === loneCode) {
        if (lone) done(MODIFIER_CODES[loneCode]);
        else {
          toast("Add a letter or other key to the modifier, e.g. Meta+H", true);
          done(null);
        }
      }
    };
    const away = (e) => !btn.contains(e.target) && done(null);
    addEventListener("keydown", down, true);
    addEventListener("keyup", up, true);
    addEventListener("mousedown", away, true);
  };
  return btn;
}

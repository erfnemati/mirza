// Line icons on a 24×24 grid, drawn with the current text colour.

const ICONS = {
  waves: '<path d="M3 10v4M7.5 6v12M12 3v18M16.5 7.5v9M21 10v4"/>',
  keyboard: '<rect x="2.5" y="5.5" width="19" height="13" rx="2"/><path d="M6.5 9.5h.01M10 9.5h.01M13.5 9.5h.01M17 9.5h.01M8 14.5h8"/>',
  type: '<path d="M4 20h16"/><path d="M15.5 4.5a2.1 2.1 0 0 1 3 3L9 17l-4 1 1-4z"/>',
  mic: '<rect x="9" y="2.5" width="6" height="12" rx="3"/><path d="M5 11a7 7 0 0 0 14 0M12 18v3.5"/>',
  chart: '<path d="M3.5 3.5v17h17"/><path d="M8 16v-4M12.5 16V8M17 16v-7"/>',
  history: '<path d="M3 12a9 9 0 1 0 2.64-6.36L3 8.3"/><path d="M3 3.5v4.8h4.8M12 7.5V12l3 2"/>',
  sliders: '<path d="M4 21v-7M4 10V3M12 21v-9M12 8V3M20 21v-5M20 12V3M1.5 14h5M9.5 8h5M17.5 16h5"/>',
  info: '<circle cx="12" cy="12" r="9.5"/><path d="M12 16.5v-5M12 8h.01"/>',
  alert: '<circle cx="12" cy="12" r="9.5"/><path d="M12 7.5v5M12 16.5h.01"/>',
  x: '<path d="M18 6 6 18M6 6l12 12"/>',
  check: '<path d="m20 6.5-10.5 11L4 12"/>',
  chevron: '<path d="m6 9 6 6 6-6"/>',
  search: '<circle cx="11" cy="11" r="7.5"/><path d="m20.5 20.5-4.2-4.2"/>',
  copy: '<rect x="8.5" y="8.5" width="13" height="13" rx="2"/><path d="M15.5 8.5V5a2 2 0 0 0-2-2H5a2 2 0 0 0-2 2v8.5a2 2 0 0 0 2 2h3.5"/>',
  pencil: '<path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L8 18l-4.5 1.5L5 15z"/>',
  eye: '<path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7S2 12 2 12z"/><circle cx="12" cy="12" r="3"/>',
  eyeOff: '<path d="M10.6 5.1A10.5 10.5 0 0 1 12 5c6.5 0 10 7 10 7a17 17 0 0 1-2.2 3.1M6.6 6.6C3.7 8.4 2 12 2 12s3.5 7 10 7a9.7 9.7 0 0 0 5.4-1.6"/><path d="M9.9 9.9a3 3 0 0 0 4.2 4.2M3 3l18 18"/>',
  external: '<path d="M14 3h7v7M10 14 21 3M19 14v5a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2h5"/>',
  file: '<path d="M14 2.5H6.5a2 2 0 0 0-2 2v15a2 2 0 0 0 2 2h11a2 2 0 0 0 2-2V8z"/><path d="M14 2.5V8h5.5"/>',
  power: '<path d="M12 2.5v9M18.4 6.6a9 9 0 1 1-12.8 0"/>',
  refresh: '<path d="M20.5 12a8.5 8.5 0 1 1-2.5-6l2.5 2.5"/><path d="M20.5 3.5v5h-5"/>',
  play: '<path d="M7 4.5v15l12-7.5z"/>',
  stop: '<rect x="6" y="6" width="12" height="12" rx="2"/>',
  wallet: '<path d="M19 7V5.5A1.5 1.5 0 0 0 17.5 4H5a2 2 0 0 0 0 4h14a1 1 0 0 1 1 1v3.5M20 16.5V19a1 1 0 0 1-1 1H5a2 2 0 0 1-2-2V6"/><path d="M21.5 12.5h-4a2 2 0 0 0 0 4h4z"/>',
};

/** An icon as an <svg> element. */
function icon(name) {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("class", "icon");
  svg.setAttribute("aria-hidden", "true");
  svg.innerHTML = ICONS[name] || "";
  return svg;
}

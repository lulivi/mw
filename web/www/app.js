// The page: picks the files and options, and shows what the worker returns.

/** Largest file read, in bytes: a phone runs out of memory on bigger ones. */
const MAX_BYTES = 500 * 1024 * 1024;

const $ = (id) => document.getElementById(id);
const form = $("options");
const worker = new Worker(new URL("worker.js", import.meta.url), { type: "module" });

let nextId = 0;
/** Whether the files added can make an image (they hold a history and agree). */
let ready = false;
let imageUrl;
/** Whether an image is being made, and whether the options changed meanwhile:
 * then one more is made, with the latest options. */
let creating = false;
let pending = false;
const smooth = () => (matchMedia("(prefers-reduced-motion: reduce)").matches ? "auto" : "smooth");

// Theme: the device's, or light or dark, remembered on this device only.
const THEMES = { "": "Device", light: "Light", dark: "Dark" };
const root = document.documentElement;
/** Names the theme on the button; the CSS shows its icon. */
function showTheme() {
  const label = `Theme: ${THEMES[root.dataset.theme] ?? "Device"}`;
  $("theme").setAttribute("aria-label", label);
  $("theme").title = label;
}
$("theme").addEventListener("click", () => {
  const names = Object.keys(THEMES);
  root.dataset.theme = names[(names.indexOf(root.dataset.theme ?? "") + 1) % names.length];
  try {
    localStorage.setItem("theme", root.dataset.theme);
  } catch {}
  showTheme();
});
showTheme();

// Color: of the page and the image, remembered on this device only.
$("color").addEventListener("change", () => {
  try {
    localStorage.setItem("color", form.color.value);
  } catch {}
  create();
});

// Time zones, with the device's selected.
const here = Intl.DateTimeFormat().resolvedOptions().timeZone || "UTC";
const zones = Intl.supportedValuesOf ? Intl.supportedValuesOf("timeZone") : [];
for (const tz of new Set([here, ...zones].sort())) form.tz.add(new Option(tz, tz, false, tz === here));

// Adding files.
$("input").addEventListener("change", (e) => {
  add(e.target.files);
  e.target.value = "";
});
const drop = $("drop");
drop.addEventListener("dragover", (e) => {
  e.preventDefault();
  drop.classList.add("over");
});
drop.addEventListener("dragleave", () => drop.classList.remove("over"));
drop.addEventListener("drop", (e) => {
  e.preventDefault();
  drop.classList.remove("over");
  add(e.dataTransfer.files);
});

/** Lists the files and sends the ones that aren't too big to the worker. */
function add(fileList) {
  const files = [];
  for (const file of fileList) {
    const id = nextId++;
    const row = fileRow(id, file.name);
    if (file.size > MAX_BYTES) {
      const mb = Math.round(file.size / 1024 / 1024);
      show(row, [], `Too big (${mb} MB; the limit is 500 MB). Export only your listening history.`);
      continue;
    }
    show(row, ["Waiting…"]);
    files.push({ id, file });
  }
  if (files.length) {
    worker.postMessage({ type: "add", files, tz: form.tz.value });
  }
}

/** A row of the file list, with a button to remove the file. */
function fileRow(id, name) {
  const row = document.createElement("li");
  row.id = `file-${id}`;
  const title = document.createElement("strong");
  title.textContent = name;
  const remove = document.createElement("button");
  remove.type = "button";
  remove.className = "remove";
  remove.textContent = "Remove";
  remove.setAttribute("aria-label", `Remove ${name}`);
  remove.addEventListener("click", () => {
    row.remove();
    worker.postMessage({ type: "remove", id, tz: form.tz.value });
  });
  const lines = document.createElement("ul");
  row.append(title, remove, lines);
  $("files").append(row);
  return row;
}

/** Shows what a file holds, or why it can't be read. */
function show(row, lines, error) {
  const list = row.querySelector("ul");
  list.replaceChildren(
    ...lines.map((l) => Object.assign(document.createElement("li"), { textContent: l })),
  );
  if (error) list.append(Object.assign(document.createElement("li"), { className: "error", textContent: error }));
}

// Options.
/** Enables the fields of the period chosen. */
function periodFields() {
  const period = form.period.value;
  form.year.disabled = period !== "year";
  form.from.disabled = form.to.disabled = period !== "range";
}
form.addEventListener("change", (e) => {
  periodFields();
  // A new time zone can change the years; the image follows the worker's answer.
  if (e.target === form.tz) worker.postMessage({ type: "years", tz: form.tz.value });
  else create();
});
form.addEventListener("submit", (e) => e.preventDefault());

/** Makes the image with the options chosen, once the files are ready; while
 * one is being made, makes one more after it. */
function create() {
  if (!ready) return;
  if (creating) {
    pending = true;
    return;
  }
  const period = form.period.value;
  const year = form.year.value;
  const options = {
    source: form.source.value,
    from: period === "year" ? `${year}-01-01` : period === "range" ? form.from.value : "",
    to: period === "year" ? `${year}-12-31` : period === "range" ? form.to.value : "",
    rankBy: form.rankBy.value,
    top: Number(form.top.value),
    color: form.color.value,
    tz: form.tz.value,
  };
  creating = true;
  $("image").classList.add("busy");
  $("bars").inert = true;
  worker.postMessage({ type: "create", options });
}

/** Once the image is made, or failed: makes the next one if the options changed. */
function created() {
  creating = false;
  $("image").classList.remove("busy");
  $("bars").inert = false;
  // The caption is the progress over the image, or the sample's until the first image.
  $("caption").hidden = !$("image").classList.contains("placeholder");
  $("caption").textContent = "Your image will appear here";
  if (pending) {
    pending = false;
    create();
  }
}

// Messages from the worker.
worker.onmessage = ({ data: msg }) => {
  if (msg.type === "progress") {
    if (!creating) $("status").textContent = msg.text;
    else if (msg.text) {
      $("caption").textContent = msg.text;
      $("caption").hidden = false;
    }
  } else if (msg.type === "file") {
    const row = $(`file-${msg.id}`);
    if (msg.error) show(row, [], msg.error);
    else show(row, msg.lines);
  } else if (msg.type === "state") {
    setReady(!msg.error && msg.years.length > 0, msg.years);
    $("status").textContent = msg.error ?? "";
    $("status").classList.toggle("error", Boolean(msg.error));
    create();
  } else if (msg.type === "result") {
    result(msg);
    created();
  } else if (msg.type === "error") {
    showBars([]);
    $("status").textContent = "";
    $("error").hidden = false;
    $("error").textContent = msg.message;
    created();
  }
};
worker.onerror = (e) => {
  created();
  $("status").textContent = `Something went wrong: ${e.message}`;
  $("status").classList.add("error");
};

/** Opens step 2 when the files can make an image, and fills the year list,
 * keeping the year chosen if it's still there. */
function setReady(value, years) {
  // With the data read, step 1 shrinks; the first time, bring the options into view.
  $("data").classList.toggle("has-data", value);
  if (value && !ready) $("options-title").scrollIntoView({ behavior: smooth() });
  ready = value;
  const chosen = form.year.value;
  form.year.replaceChildren(...years.map((y) => new Option(y, y)));
  form.year.value = years.includes(Number(chosen)) ? chosen : String(years.at(-1) ?? "");
  $("settings").disabled = !ready;
  $("needs-file").hidden = ready;
}

/** Size of the image, in px. */
const W = 1080;
const H = 1920;

/** Puts a button over each of the chart's bars, which shows its year or
 * month; none when there's a single bar. */
function showBars(bars) {
  const clickable = bars.length > 1;
  $("bar-hint").hidden = !clickable;
  const pct = (v, of) => `${(v / of) * 100}%`;
  $("bars").replaceChildren(
    ...(clickable ? bars : []).map((bar) => {
      const button = document.createElement("button");
      button.type = "button";
      Object.assign(button.style, {
        left: pct(bar.x, W),
        top: pct(bar.y, H),
        width: pct(bar.w, W),
        height: pct(bar.h, H),
      });
      button.title = `Show ${bar.label}`;
      button.setAttribute("aria-label", button.title);
      button.addEventListener("click", () => showPeriod(bar));
      return button;
    }),
  );
}

/** Sets the period to a bar's year or month and makes its image. */
function showPeriod({ from, to }) {
  const year = from.slice(0, 4);
  const wholeYear = from.endsWith("-01-01") && to.endsWith("-12-31");
  if (wholeYear && [...form.year.options].some((o) => o.value === year)) {
    form.period.value = "year";
    form.year.value = year;
  } else {
    form.period.value = "range";
    form.from.value = from;
    form.to.value = to;
  }
  periodFields();
  create();
}

/** Shows the image, with its download and share buttons and notes. */
function result({ png, notes, bars }) {
  const blob = new Blob([png], { type: "image/png" });
  if (imageUrl) URL.revokeObjectURL(imageUrl);
  imageUrl = URL.createObjectURL(blob);
  const image = $("image");
  image.src = imageUrl;
  image.alt = "Your listening recap";
  image.classList.remove("placeholder");
  $("download").href = imageUrl;
  $("notes").replaceChildren(
    ...notes.map((n) => Object.assign(document.createElement("li"), { textContent: n })),
  );

  const file = new File([blob], "wrapped.png", { type: "image/png" });
  const share = $("share");
  share.hidden = !navigator.canShare?.({ files: [file] });
  share.onclick = () => navigator.share({ files: [file] }).catch(() => {});

  showBars(bars);
  $("error").hidden = true;
  $("output").hidden = false;
}

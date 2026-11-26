// Runs mw (compiled to WebAssembly) off the page's thread, so the page stays
// responsive. Messages from the page are handled one at a time, in order.

import init, { Session } from "./pkg/mw_web.js";

/** This repository on `main`, which `mw` downloads from too. */
const REPO_URL = "https://raw.githubusercontent.com/lulivi/mw/main/";
/** The shared song database: song lengths for YouTube Music plays. */
const DB_URL = REPO_URL + "data/songs.json";
/** Where the fallback fonts for the characters Inter lacks are. */
const FONTS_URL = REPO_URL + "assets/fonts/";

const ready = init();
let session;
/** The song database download, started when the first YouTube history is added. */
let db;
/** Fallback fonts that failed to download, not tried again. */
const failedFonts = new Set();

let queue = Promise.resolve();
onmessage = (e) => {
  queue = queue.then(() => handle(e.data));
};

/** Handles one message from the page. */
async function handle(msg) {
  await ready;
  session ??= new Session();
  try {
    if (msg.type === "add") await add(msg.files, msg.tz);
    else if (msg.type === "remove") {
      session.remove(msg.id);
      state(msg.tz);
    } else if (msg.type === "years") state(msg.tz);
    else if (msg.type === "create") await create(msg.options);
  } catch (err) {
    postMessage({ type: "error", message: message(err) });
  }
}

/** Reads the files added, one by one, saying what each holds. */
async function add(files, tz) {
  for (const [i, { id, file }] of files.entries()) {
    const of = files.length > 1 ? ` (${i + 1} of ${files.length})` : "";
    progress(`Reading ${file.name}${of}…`);
    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      postMessage({ type: "file", id, lines: session.add(id, file.name, bytes) });
    } catch (err) {
      postMessage({ type: "file", id, error: message(err) });
    }
  }
  if (session.has_youtube()) db ??= loadDb();
  progress("");
  state(tz);
}

/** Tells the page which years have plays, or why the files can't be used. */
function state(tz) {
  try {
    postMessage({ type: "state", years: Array.from(session.years(tz)) });
  } catch (err) {
    postMessage({ type: "state", years: [], error: message(err) });
  }
}

/** Downloads the song database; without it YouTube Music keeps the estimate. */
async function loadDb() {
  try {
    const resp = await fetch(DB_URL);
    if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
    session.set_db(await resp.text());
  } catch (err) {
    console.warn("song database:", err);
  }
}

/** Downloads a fallback font into the session; without it its characters show as boxes. */
async function loadFont(file) {
  try {
    const resp = await fetch(FONTS_URL + file);
    if (!resp.ok) throw new Error(`HTTP ${resp.status}`);
    session.add_font(file, new Uint8Array(await resp.arrayBuffer()));
  } catch (err) {
    failedFonts.add(file);
    console.warn(`font ${file}:`, err);
  }
}

/** Makes the image. */
async function create(o) {
  if (db) {
    progress("Downloading the song database…");
    await db;
  }
  progress("Making the image…");
  const make = () => session.create(o.from, o.to, o.rankBy, o.top, o.color, o.tz);
  let r = make();
  // Names in scripts Inter lacks: download their fonts, kept for later images.
  const fonts = r.missing_fonts.filter((f) => !failedFonts.has(f));
  if (fonts.length) {
    progress("Downloading fonts…");
    await Promise.all(fonts.map(loadFont));
    progress("Making the image…");
    r = make();
  }
  const png = r.png;
  const bars = JSON.parse(r.bars);
  postMessage({ type: "result", png, text: r.text, notes: r.notes, bars }, [png.buffer]);
  progress("");
}

function progress(text) {
  postMessage({ type: "progress", text });
}

function message(err) {
  return err instanceof Error ? err.message : String(err);
}

// Runs mw (compiled to WebAssembly) off the page's thread, so the page stays
// responsive. Messages from the page are handled one at a time, in order.

import init, { Session } from "./pkg/mw_web.js";

/** The shared song database: song lengths for YouTube Music plays. */
const DB_URL = "https://raw.githubusercontent.com/lulivi/mw/main/data/songs.json";

const ready = init();
let session;
/** The song database download, started when the first YouTube history is added. */
let db;

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

/** Makes the image. */
async function create(o) {
  if (db) {
    progress("Downloading the song database…");
    await db;
  }
  progress("Making the image…");
  const r = session.create(o.source, o.from, o.to, o.rankBy, o.color, o.tz);
  const png = r.png;
  postMessage({ type: "result", png, text: r.text, notes: r.notes }, [png.buffer]);
  progress("");
}

function progress(text) {
  postMessage({ type: "progress", text });
}

function message(err) {
  return err instanceof Error ? err.message : String(err);
}

# The web page

[lulivi.github.io/mw](https://lulivi.github.io/mw/) makes the same image as
`mw`, in the browser, with nothing to install.

## Using it

1. Add your data: the ZIP Spotify or Google Takeout sends, or the JSON files in
   it. The two "How to get your … data" sections under this step say how to
   request them. Each file is read as soon as it's added, and the list says
   what it holds (or that it holds nothing to use). Files over 500 MB are
   refused; a phone would run out of memory.
2. Choose the options: the period (whole history, a year or a date range), the
   sources, the ranking, the color and the time zone (the device's by default).
   This step opens once a file is read. The options are the same as `mw`'s and
   default to the same values, except the time zone.
3. Press **Create image**, then download it or, on a phone, share it. Under the
   image, "Stats as text" has what `mw` prints in the terminal.

On a computer the image stays on the right while you scroll, with a sample in
its place until the first one is made. On a phone it comes after the options.

## Privacy

The files are read in the browser and never uploaded; nothing is stored between
visits. The page makes only these requests:

- Its own files, from the same site.
- The [song database](../data/README.md) (`data/songs.json` from this
  repository), when a YouTube history is added, to get the length of YouTube
  Music songs. It holds no data of yours, and nothing of yours is sent to get
  it. If it can't be downloaded, YouTube Music plays keep the estimate, as in
  `mw`.

No analytics, fonts or other third-party files.

## How it works

The page runs `mw`'s own Rust code compiled to WebAssembly, not a rewrite, so
it reads the files, counts and draws exactly like the command line: the same
options give a byte-identical image.

- `src/` is a library (`mw`) plus the command line (`main.rs`, behind the
  default `cli` feature, which also holds the downloads `mw` makes).
- `web/src/lib.rs` (crate `mw-web`) is what the page calls: add or remove a
  file, set the song database and create the image.
- `web/www/` is the page: plain HTML, CSS and JavaScript, with no framework or
  build step. `app.js` handles the form; `worker.js` runs the WebAssembly in a
  Web Worker, so the page stays responsive and shows progress.

## Building it

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --locked --version <the wasm-bindgen version in Cargo.lock>
./scripts/web.sh                            # builds into target/web
python3 -m http.server -d target/web 8000   # open http://localhost:8000
```

`web/Cargo.toml` pins `wasm-bindgen`, since `wasm-bindgen-cli` must be the same
version.

## Publishing

The `pages` job in `.github/workflows/ci.yml` builds the page and publishes it
to GitHub Pages each time release-plz makes a release, so the page always runs
the latest release. The song database isn't part of it: the page downloads it
from `main`, so new songs reach the page without a release.

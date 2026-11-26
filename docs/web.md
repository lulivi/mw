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
   ranking and the time zone (the device's by default). The image counts every
   file added; to leave a service out, remove its files. This step opens once a
   file is read. The options are the same as `mw`'s and default to the same
   values, except the time zone.
3. Download the image or, on a phone, share it. It's made as soon as a file
   is read, and made again with each change of the files, the options or the
   color.

Clicking a bar of the image's chart makes the image of that year or month,
the whole calendar year or month. "Back to …", next to the download button,
goes back to the period before, one click at a time; choosing a period in the
options starts over. The downloaded image is the same as `mw`'s for that
period: the bars are clickable only on the page.

On a computer the image stays on the right while you scroll, with a sample in
its place until the first one is made. On a phone it comes after the options.
The page follows the device's light or dark theme; the theme button at the top
switches it. The color, picked under the logo, is the image's accent and the
page's too. Both choices are kept on the device.

## Privacy

The files are read in the browser and never uploaded or stored. Only the theme
and the color are kept on the device, in the browser's local storage. The page
makes only these requests:

- Its own files, from the same site.
- The [song database](../data/README.md) (`data/songs.json` from this
  repository), when a YouTube history is added, to get the length of YouTube
  Music songs. It holds no data of yours, and nothing of yours is sent to get
  it. If it can't be downloaded, YouTube Music plays keep the estimate, as in
  `mw`.
- The fallback fonts (`assets/fonts/` from this repository) a name on the
  image needs, when it has characters Inter lacks (Japanese, Arabic, emoji…),
  as `mw` does. The image is made again once they're in; one that can't be
  downloaded leaves its characters as boxes, and the notes say so.

No analytics or other third-party files.

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

CI (`.github/workflows/ci.yml`) builds the page on every run, PRs included, and
publishes it to GitHub Pages each time release-plz makes a release, so the page
always runs the latest release. The song database isn't part of it: the page
downloads it from `main`, so new songs reach the page without a release.

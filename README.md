# mw — Music Wrapped

Your own "Wrapped" from your Spotify and YouTube Music data: total minutes, minutes per month, top artists and top songs, for any date range you want. It creates a 1080×1920 vertical image ready to share from your phone.

Everything runs locally: your data never leaves your computer.

- [Samples](#samples)
- [Installation](#installation)
- [Prerequisites](#prerequisites)
- [Running](#running)
- [How it counts](#how-it-counts)
- [Development](#development)
- [Credits](#credits)

## Samples

Made from a fictional listening history (2021–2025; YouTube Music from 2024).

<p>
  <a href="docs/samples/year.png"><img src="docs/samples/year.png" width="200" alt="Sample 1: 2025, Spotify only"></a>
  <a href="docs/samples/range.png"><img src="docs/samples/range.png" width="200" alt="Sample 2: Jun 2024 to May 2025, both sources"></a>
  <a href="docs/samples/history.png"><img src="docs/samples/history.png" width="200" alt="Sample 3: whole history, by year"></a>
  <a href="docs/samples/minutes.png"><img src="docs/samples/minutes.png" width="200" alt="Sample 4: 2025 ranked by minutes"></a>
</p>

1. One year, Spotify only: `--year 2025 --source spotify`
2. Custom range, both sources (estimated minutes): `--from 2024-06-01 --to 2025-05-31 --color purple`
3. Whole history; past 24 months the chart goes by year: `--color blue`
4. Top lists by minutes: `--year 2025 --rank-by minutes --color green`

## Installation

Prebuilt binaries of the latest release:

- macOS (Apple Silicon): [mw-aarch64-apple-darwin.zip](https://github.com/lulivi/mw/releases/latest/download/mw-aarch64-apple-darwin.zip)
- Windows (x64): [mw-x86_64-pc-windows-gnu.zip](https://github.com/lulivi/mw/releases/latest/download/mw-x86_64-pc-windows-gnu.zip)
- Linux (x64, static): [mw-x86_64-unknown-linux-musl.zip](https://github.com/lulivi/mw/releases/latest/download/mw-x86_64-unknown-linux-musl.zip)

Unzip it to get `mw` (`mw.exe` on Windows). The binaries aren't signed: on macOS, run `xattr -d com.apple.quarantine mw` once if it refuses to open.

With Rust installed, you can instead run `cargo install --git https://github.com/lulivi/mw`.

## Prerequisites

### Spotify

1. Open spotify.com/account/privacy in a browser (a phone browser works too).
2. Under "Download your data", tick **Extended streaming history** and click "Request".
3. Confirm the request from the email Spotify sends you. The ZIP can take up to 30 days.
4. Unzip it: inside there are `Streaming_History_Audio_*.json` files.

The basic history ("Account data", `StreamingHistory_music_*.json`) also works, but it only covers the last year.

### YouTube Music

1. On takeout.google.com, click "Deselect all" and tick only **YouTube and YouTube Music**.
2. Under "All YouTube data included", keep only **history**.
3. Under "Multiple formats", change history from HTML to **JSON**.
4. From the ZIP, take `Takeout/YouTube and YouTube Music/history/watch-history.json`.

### YouTube API key (optional)

It gives real song lengths instead of an estimate, and song names that don't depend on your account language (see [How it counts](#how-it-counts)).

1. Go to console.cloud.google.com with your Google account and create a project.
2. In "APIs & Services" → "Library", search for **YouTube Data API v3** and click "Enable".
3. In "Credentials" → "Create credentials" → "API key". Copy the key.
4. Pass it with `--yt-api-key YOUR_KEY` or set the `YT_API_KEY` environment variable.

It's free (10,000 units a day, 1 per 50 songs) and lengths and names are cached in `yt-durations.json`. Don't share your key.

## Running

```bash
# 2025 with both sources combined
./mw --spotify ./my_data/spotify --ytm ./my_data/watch-history.json --year 2025

# A custom range, Spotify only, terminal only
./mw --spotify ./my_data/spotify --from 2024-06-01 --to 2025-05-31 --source spotify --no-image

# Your whole history (no dates)
./mw --spotify ./my_data/spotify --ytm ./my_data/watch-history.json
```

On Windows use `mw.exe`; after `cargo install`, just `mw`. Run `mw --help` for every option.

## How it counts

- Spotify plays shorter than 30 seconds don't count (they're skips), same as the official Wrapped. Podcasts and audiobooks are ignored.
- YouTube Music doesn't record how long you listened to each song, only that you played it. Each play counts `--ytm-avg-minutes` minutes, which is why the total is marked as estimated when YouTube Music is included ("≈" in the terminal, "estimated minutes" in the image).
- With `--yt-api-key`, each YouTube Music song counts its real length (capped at 15 min) and that mark goes away. It assumes you listened to the whole song. Songs that can't be looked up (deleted videos, no connection, quota exhausted) keep the estimate.
- With `--yt-api-key`, song and artist names come from the video title and channel instead of the Takeout text, which carries wording like "Watched" in your account language. They're video titles, not official song credits: a channel can be a label rather than the artist. Songs that can't be looked up keep the Takeout names.
- Artists and songs are merged across sources ignoring case, accents and YouTube add-ons like " - Topic" or "(Official Video)".

## Development

```bash
cargo build --release              # target/release/mw
cargo run --release -- --help      # build and run with arguments
./scripts/test.sh                  # cargo test
./scripts/lint.sh                  # cargo fmt --check + clippy, as CI runs them
./scripts/samples.sh               # regenerate docs/samples/ after changing the image
./scripts/dist.sh                  # cross-compile the release zips into dist/ (needs Docker)
```

Layout: `src/sources/` (one reader per service) → `model.rs` (`Play`) → `stats.rs` (filters and aggregates) → `render.rs` (SVG → PNG with resvg). `youtube.rs` fetches real YouTube lengths and names, and `normalize.rs` builds the keys that merge artists and songs. The fonts are embedded in the binary so the image looks the same everywhere.

## Credits

- [Inter](https://github.com/rsms/inter) (Regular and Bold), Copyright (c) 2016 The Inter Project Authors, SIL Open Font License 1.1: `assets/OFL-Inter.txt`.
- [Tiny5 Mono](https://github.com/Gissio/font_Tiny5Mono) Regular, for the main figure, SIL Open Font License 1.1: `assets/OFL-Tiny5Mono.txt`.

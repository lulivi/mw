# mw — Music Wrapped

Your own "Wrapped" from your Spotify and YouTube Music data: total minutes, minutes per month, top artists and top songs, for any date range you want. It creates a 1080×1920 vertical image ready to share from your phone.

Everything runs locally: your data never leaves your computer.

## Download

Prebuilt binaries of the latest release (no Rust needed):

- macOS (Apple Silicon): [mw-aarch64-apple-darwin.zip](https://github.com/lulivi/mw/releases/latest/download/mw-aarch64-apple-darwin.zip)
- Windows (x64): [mw-x86_64-pc-windows-gnu.zip](https://github.com/lulivi/mw/releases/latest/download/mw-x86_64-pc-windows-gnu.zip)
- Linux (x64, static): [mw-x86_64-unknown-linux-musl.zip](https://github.com/lulivi/mw/releases/latest/download/mw-x86_64-unknown-linux-musl.zip)

Unzip it and use `./mw` (`mw.exe` on Windows) wherever the examples below say `./target/release/mw`. The binaries aren't signed: on macOS, run `xattr -d com.apple.quarantine mw` once if it refuses to open.

## Requirements (building from source)

- A recent stable Rust (`rustup update` if you already have it; otherwise see https://rustup.rs).

## 1. Get your data

**Spotify**

1. Open spotify.com/account/privacy in a browser (a phone browser works too).
2. Under "Download your data", tick **Extended streaming history** and click "Request".
3. Confirm the request from the email Spotify sends you. The ZIP can take up to 30 days.
4. Unzip it: inside there are `Streaming_History_Audio_*.json` files.

The basic history ("Account data", `StreamingHistory_music_*.json`) also works, but it only covers the last year.

**YouTube Music**

1. On takeout.google.com, click "Deselect all" and tick only **YouTube and YouTube Music**.
2. Under "All YouTube data included", keep only **history**.
3. Under "Multiple formats", change history from HTML to **JSON**.
4. From the ZIP, take `Takeout/YouTube and YouTube Music/history/watch-history.json`.

**Optional: YouTube API key (real song lengths)**

1. Go to console.cloud.google.com with your Google account and create a project.
2. In "APIs & Services" → "Library", search for **YouTube Data API v3** and click "Enable".
3. In "Credentials" → "Create credentials" → "API key". Copy the key.
4. Pass it with `--yt-api-key YOUR_KEY` or set the `YT_API_KEY` environment variable.

It's free: each request covers 50 songs and uses 1 unit of the 10,000 daily units. Lengths are cached in `yt-durations.json`, so only new songs and those still without a length (deleted videos or transient failures) are fetched. Don't share your key.

## 2. Run

```bash
cd mw
cargo build --release

# 2025 with both sources combined
./target/release/mw --spotify ./my_data/spotify --ytm ./my_data/watch-history.json --year 2025

# A custom range, Spotify only, terminal only
./target/release/mw --spotify ./my_data/spotify --from 2024-06-01 --to 2025-05-31 --source spotify --no-image

# Your whole history (no dates)
./target/release/mw --spotify ./my_data/spotify --ytm ./my_data/watch-history.json
```

On Windows the binary is `target\release\mw.exe`. You can also use `cargo run --release -- <arguments>`.

To have `mw` available from any folder, install it with `cargo install --path .` and run it directly: `mw --ytm watch-history.json --year 2025`.

### Cross-compiling

With Docker installed, `./scripts/dist.sh` builds macOS (Apple Silicon), Windows (x64) and Linux (x64, static) binaries into `dist/<target>/`, plus a `dist/mw-<target>.zip` per target with the binary and its licenses. No Rust targets or other toolchains are needed on your machine.

## Options

| Option | What it does | Default |
| --- | --- | --- |
| `--spotify <DIR>` | Folder (or file) with the Spotify JSON files; subfolders are searched too | — |
| `--ytm <FILE>` | Path to `watch-history.json` | — |
| `--source all\|spotify\|ytm` | Sources included in the stats | `all` |
| `--year <YYYY>` | Whole calendar year | — |
| `--from` / `--to <YYYY-MM-DD>` | Date range, both days included | whole history |
| `--top <N>` | Size of the top lists in the terminal (the image shows 5) | 5 |
| `--rank-by minutes\|plays` | Rank top lists by minutes or by plays; the image shows that figure for each song | `plays` |
| `--color orange\|purple\|blue\|green\|pink\|red\|gray` | Accent color of the image | `orange` |
| `--tz <zone>` | Time zone used to group by day and month | `Europe/Madrid` |
| `--ytm-avg-minutes <F>` | Minutes counted per YouTube Music play | 3.5 |
| `--yt-api-key <KEY>` | YouTube Data API v3 key to use each song's real length (also `YT_API_KEY`) | — |
| `--yt-cache <FILE>` | Durations cache | `yt-durations.json` next to `watch-history.json` |
| `--ytm-max-minutes <F>` | Cap per play with real length (for long mixes and livestreams) | 15 |
| `--out <FILE>` | PNG file name | `wrapped.png` |
| `--overwrite` | Replace the PNG if it already exists; without it, the program stops with an error | — |
| `--no-image` | Only print the stats in the terminal | — |

## How it counts

- Spotify plays shorter than 30 seconds don't count (they're skips), same as the official Wrapped. Podcasts and audiobooks are ignored.
- YouTube Music doesn't record how long you listened to each song, only that you played it. Each play counts `--ytm-avg-minutes` minutes, which is why the total is marked as estimated when YouTube Music is included ("≈" in the terminal, "estimated minutes" in the image).
- With `--yt-api-key`, each YouTube Music song counts its real length (capped at 15 min) and that mark goes away. It assumes you listened to the whole song. Songs that can't be looked up (deleted videos, no connection, quota exhausted) keep the estimate.
- Artists and songs are merged across sources ignoring case, accents, spaces and YouTube channel add-ons (" - Topic", "VEVO", "Official"). Title add-ons like "(Official Video)" are ignored, and a leading "Artist - " is removed from YouTube titles. "(Remix)" and "(feat. …)" are kept.

## Development

```bash
./scripts/test.sh     # cargo test
./scripts/lint.sh     # cargo fmt --check + clippy, as CI runs them
```

Layout: `src/sources/` (one reader per service) → `model.rs` (`Play`) → `stats.rs` (filters and aggregates) → `render.rs` (SVG → PNG with resvg). `durations.rs` fetches real YouTube lengths, and `normalize.rs` builds the keys that merge artists and songs. The fonts are embedded in the binary so the image looks the same everywhere.

## Third-party assets

The image uses the [Inter](https://github.com/rsms/inter) font (`assets/Inter-Regular.otf`, `assets/Inter-Bold.otf`), Copyright (c) 2016 The Inter Project Authors, licensed under the SIL Open Font License 1.1. The full license is in `assets/OFL-Inter.txt` and must be kept with the font, including in binary releases.

The main figure uses [Tiny5 Mono](https://github.com/Gissio/font_Tiny5Mono) Regular (`assets/Tiny5Mono-Regular.otf`), also licensed under the SIL Open Font License 1.1. Its license is in `assets/OFL-Tiny5Mono.txt` and must likewise be kept with the font.

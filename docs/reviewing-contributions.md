# Reviewing contributions

How the maintainer adds the files listeners send to the
[song database](../data/README.md).

## The steps

```bash
mw db resolve --file ytm-missing-….json                   # 1. look up lengths in MusicBrainz
# 2. open the file and review what still has a "check"
mw db import --file ytm-missing-….json --dry-run          # 3. see what would be added
mw db import --file ytm-missing-….json                    # 4. add it to data/songs.json
```

Then open a pull request with the change to `data/songs.json`. The same steps
work for `spotify-durations-….json` files.

## The contribution file

A list of songs. Each one has `artist`, `title` and `seconds`, and may have
`album`, `video_ids` (YouTube), `mbid` (MusicBrainz) and `source`. Songs that
need a look have a `check` saying why:

```json
{ "artist": "A", "title": "New", "seconds": 229 }
{ "artist": "C", "title": "Once", "check": "one full play", "seconds": "2:30" }
{ "artist": "D", "title": "Edit", "check": "full plays: 3:00 ×2, 4:00 ×2", "seconds": null }
{ "artist": "Repion", "title": "Brillante", "video_ids": ["bbbbbbbbbbb"], "check": "no length", "seconds": null }
```

| `check` | What it means |
|---|---|
| *(none)* | Ready: Spotify saw at least two full plays of the same length. |
| `one full play` | Only one play to go by. `seconds` is that play. |
| `full plays: …` | The plays don't agree (maybe two versions). No length yet. |
| `no length` | From YouTube Music, which never has the length. |

`seconds` is a number, `"m:ss"` or `null` (unknown). `check` always comes right
before `seconds`, so you can delete the whole `check` line.

## 1. Resolve

`mw db resolve --file <FILE>` looks up every song with a `check` in
[MusicBrainz](https://musicbrainz.org) and edits the file. It never touches
`data/songs.json`.

- **Found**: the song gets the MusicBrainz `seconds`, `mbid` and
  `"source": "musicbrainz"`, and its `check` is gone.
- **Found, but far from what was played**: more than 2 seconds away from every
  Spotify play. It keeps a `check` with both, e.g.
  `"musicbrainz 3:52, played 3:49"`.
- **Not found**: `"; not in musicbrainz"` is added to its `check`. The command
  prints these songs at the end.

MusicBrainz allows about one request per second, so a thousand songs take about
20 minutes. Use `--limit N` to look up only N songs. The file is saved after
every song, so you can stop it at any time (Ctrl-C) and run it again later: it
continues with the songs not looked up yet.

## 2. Review

Search the file for `"check"`. For each song:

- **Length is right**: delete the `check` line.
- **Length is wrong or missing**: set `seconds` (`229` or `"3:49"`; compare with
  the Spotify app or the video), then delete the `check` line.
- **Not sure**: leave it. Import skips it.

## 3. Import

`mw db import --file <FILE>` adds the songs without a `check` to
`data/songs.json` (`--songs <PATH>` for another file). `--dry-run` only prints
what it would do, and `--list` lists the songs added and skipped.

- A song already in the database is never changed.
- Lengths outside 30–1200 s are skipped.
- Each song keeps its `source`. Without one it's `spotify`, or `manual` if it
  has video IDs (a length you typed).
- It stops, writing nothing, if a song has `"seconds": null` without a
  `check`, or if a video ID would end up in two songs. The message names the
  songs to fix.

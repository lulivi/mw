# Song database

`songs.json` holds the real length of songs, so YouTube Music plays (whose
Takeout history has no length) count their real minutes instead of the
`--ytm-avg-minutes` estimate. `mw` downloads it from this repository on every
run and keeps a copy in its cache folder (`~/.cache/mw/songs.json` on Linux).

## Format

```json
{
  "version": 1,
  "songs": {
    "rosalia|despecha": {
      "artist": "ROSALÍA",
      "title": "DESPECHÁ",
      "seconds": 157,
      "source": "musicbrainz",
      "mbid": "<MusicBrainz recording ID>",
      "video_ids": ["<YouTube video ID>"]
    }
  }
}
```

- The key is `normalize::track_key(artist, title)`: the artist key and the
  cleaned title, joined by `|` (the artist key has only letters and digits, so the first `|` always ends it).
- `seconds`: length of the song, greater than 0.
- `source`: where the length comes from: `musicbrainz`, `spotify` or `manual`.
- `mbid` (optional): MusicBrainz recording ID.
- `video_ids` (optional): YouTube video IDs of this song. A video ID belongs to
  one song only. A play matching one also takes the song's artist and title.

A play is matched by video ID first, then by key; otherwise it keeps the
estimate. `cargo test` checks that every key matches its song, that no video ID
is in two songs and that every length is positive.

Bump `version` only for changes older `mw` releases can't read: they ignore the
database and keep the estimates.

## Attribution

Song data marked `musicbrainz` comes from [MusicBrainz](https://musicbrainz.org),
whose core data is released under [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/).

# Music Wrapped

A listening recap built from Spotify and YouTube Music data exports.

## Language

**Song length**:
How long a song is, from start to end.
_Avoid_: Real length, duration

**Song database**:
The shared list of songs with their song length, kept in this repository and downloaded at run time.
_Avoid_: Song DB cache, durations cache

**Estimate**:
The fixed length (`--ytm-avg-minutes`) a YouTube Music play counts when the song database doesn't know the song.
_Avoid_: Average, default length

**Title wording**:
The text Takeout puts around each YouTube Music title in the account language ("Watched X", "X angesehen"), detected from the file.
_Avoid_: Prefix, header

**Full play**:
A Spotify play that ran to the end of the song, so its play time is the song length.
_Avoid_: Trackdone play, complete listen

**Contribution file**:
A file of songs and their song length that a listener sends to the maintainer, without their listening history.
_Avoid_: Export file, durations file

**Check**:
A mark on an entry of a contribution file saying it needs review before it goes into the song database, and why.
_Avoid_: Flag, unverified

**Extended history**:
Spotify's full listening history, requested on its own, which says how each play ended.
_Avoid_: Full export

**Basic history**:
Spotify's last year of plays, from "Account data".
_Avoid_: Short history

**YouTube history**:
The watch history Google Takeout exports (as `watch-history.json` or `My Activity.json`), from which mw reads the YouTube Music plays.
_Avoid_: Takeout file, watch history file

**Web page**:
mw running in the browser, published on GitHub Pages.
_Avoid_: Web app, site, frontend

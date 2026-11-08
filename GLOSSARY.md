# Music Wrapped

A listening recap built from Spotify and YouTube Music data exports.

## Language

**Song database**:
The shared list of songs with their real length, kept in this repository and downloaded at run time.
_Avoid_: Song DB cache, durations cache

**Estimate**:
The fixed length (`--ytm-avg-minutes`) a YouTube Music play counts when the song database doesn't know the song.
_Avoid_: Average, default length

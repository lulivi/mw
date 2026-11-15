# Agents

## Features

Each feature has a GitHub issue `#<N>`: branch `feature/gh-<N>`, every commit
message ends with `Refs #<N>`, the PR body has `Closes #<N>`.

## Song additions

A PR that only changes `data/songs.json` uses the `chore` type, e.g.
`chore(db): add the second round of songs`, so release-plz bumps no
version. Release binaries don't ship the file; `mw` downloads it from
`main` at run time, so merging is enough to release new songs.

Each round is a feature whose issue is titled `<round> round of song updates`
with the `song-request` label.

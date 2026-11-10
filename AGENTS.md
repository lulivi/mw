# Agents

## Song additions

A PR that only changes `data/songs.json` uses the `chore` type, e.g.
`chore(db): add the second round of songs`, so release-plz bumps no
version. Release binaries don't ship the file; `mw` downloads it from
`main` at run time, so merging is enough to release new songs.

Each round gets an issue titled `<round> round of song updates` with the
`song-request` label; the branch, commits and PR reference it as for any
feature (`feature/gh-<N>`, `Refs #<N>`, `Closes #<N>`).

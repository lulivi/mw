# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.2](https://github.com/lulivi/mw/compare/v0.4.1...v0.4.2) - 2026-11-18

### Added

- *(web)* click a chart bar to see that year or month ([#84](https://github.com/lulivi/mw/pull/84))

## [0.4.1](https://github.com/lulivi/mw/compare/v0.4.0...v0.4.1) - 2026-11-18

### Added

- *(web)* make the image as the data or the options change, and show the version ([#81](https://github.com/lulivi/mw/pull/81))

### Fixed

- *(web)* read mw's version wherever the repository is checked out ([#83](https://github.com/lulivi/mw/pull/83))

## [0.4.0](https://github.com/lulivi/mw/compare/v0.3.1...v0.4.0) - 2026-11-16

### Added

- show the logo in the image and pick the color for the whole page ([#78](https://github.com/lulivi/mw/pull/78))
- *(db)* [**breaking**] merge export-spotify and export-ytm into db export ([#76](https://github.com/lulivi/mw/pull/76))

## [0.3.1](https://github.com/lulivi/mw/compare/v0.3.0...v0.3.1) - 2026-11-15

### Other

- build the release binaries in parallel and release only once they build ([#74](https://github.com/lulivi/mw/pull/74))

## [0.3.0](https://github.com/lulivi/mw/compare/v0.2.7...v0.3.0) - 2026-11-15

### Added

- *(image)* [**breaking**] rework the image and add a top 10 layout ([#70](https://github.com/lulivi/mw/pull/70))

## [0.2.7](https://github.com/lulivi/mw/compare/v0.2.6...v0.2.7) - 2026-11-14

### Fixed

- *(web)* show the favicon in the browser tab ([#60](https://github.com/lulivi/mw/pull/60))

### Other

- credit Lorena Jiménez for the logo and favicons ([#59](https://github.com/lulivi/mw/pull/59))
- install a prebuilt wasm-bindgen in the pages job ([#55](https://github.com/lulivi/mw/pull/55))

## [0.2.6](https://github.com/lulivi/mw/compare/v0.2.5...v0.2.6) - 2026-11-13

### Added

- *(web)* add the mw logo and favicon to the web page ([#53](https://github.com/lulivi/mw/pull/53))

## [0.2.5](https://github.com/lulivi/mw/compare/v0.2.4...v0.2.5) - 2026-11-12

### Added

- *(web)* make the web page clearer and cleaner ([#49](https://github.com/lulivi/mw/pull/49))

### Fixed

- *(render)* keep year labels from overlapping in long histories ([#51](https://github.com/lulivi/mw/pull/51))

## [0.2.4](https://github.com/lulivi/mw/compare/v0.2.3...v0.2.4) - 2026-11-11

### Added

- make the wrapped image in the browser ([#42](https://github.com/lulivi/mw/pull/42))

### Other

- keep release-plz working with the web crate in the workspace ([#43](https://github.com/lulivi/mw/pull/43))

## [0.2.3](https://github.com/lulivi/mw/compare/v0.2.2...v0.2.3) - 2026-11-10

### Added

- add install scripts and x64 macOS, arm64 Linux builds ([#39](https://github.com/lulivi/mw/pull/39))

### Other

- *(db)* add the second round of songs ([#36](https://github.com/lulivi/mw/pull/36))

## [0.2.2](https://github.com/lulivi/mw/compare/v0.2.1...v0.2.2) - 2026-11-10

### Added

- group the terminal chart section like the image ([#34](https://github.com/lulivi/mw/pull/34))
- *(ytmusic)* detect the Takeout title wording in any language ([#32](https://github.com/lulivi/mw/pull/32))

## [0.2.1](https://github.com/lulivi/mw/compare/v0.2.0...v0.2.1) - 2026-11-09

### Added

- *(db)* add the first round of songs ([#30](https://github.com/lulivi/mw/pull/30))

## [0.2.0](https://github.com/lulivi/mw/compare/v0.1.4...v0.2.0) - 2026-11-09

### Added

- *(db)* [**breaking**] share YouTube Music songs and resolve lengths with MusicBrainz ([#28](https://github.com/lulivi/mw/pull/28))
- *(db)* share Spotify song lengths with export-spotify / import-spotify ([#27](https://github.com/lulivi/mw/pull/27))
- *(ytmusic)* [**breaking**] replace the YouTube API with a shared song database ([#24](https://github.com/lulivi/mw/pull/24))

## [0.1.4](https://github.com/lulivi/mw/compare/v0.1.3...v0.1.4) - 2026-11-08

### Added

- *(ytmusic)* use YouTube API titles and channels for song names ([#16](https://github.com/lulivi/mw/pull/16))

## [0.1.3](https://github.com/lulivi/mw/compare/v0.1.2...v0.1.3) - 2026-11-08

### Other

- *(release)* open release PRs with a personal token ([#13](https://github.com/lulivi/mw/pull/13))

## [0.1.2](https://github.com/lulivi/mw/compare/v0.1.1...v0.1.2) - 2026-11-08

### Other

- add sample output images to the README ([#10](https://github.com/lulivi/mw/pull/10))

## [0.1.1](https://github.com/lulivi/mw/compare/v0.1.0...v0.1.1) - 2026-11-07

### Other

- *(release)* run the checks on release PRs ([#9](https://github.com/lulivi/mw/pull/9))

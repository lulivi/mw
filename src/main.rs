//! Music Wrapped (`mw`): a listening recap from Spotify and YouTube Music data
//! exports.
//!
//! This is the command line entry point: it parses the arguments, loads the
//! selected sources, computes the stats, prints them and saves the image.

use std::fs;
use std::io::{IsTerminal, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use chrono_tz::Tz;
use clap::{Args, Parser, Subcommand, ValueEnum};

use mw::model::{Play, Source};
use mw::render::{fmt_count, fmt_int};
use mw::stats::{self, Filters, RankBy};
use mw::{lengths, musicbrainz, render, songdb, sources, text};

/// Music Wrapped: your listening recap from your Spotify and YouTube Music data exports.
#[derive(Parser)]
#[command(name = "mw", version, args_conflicts_with_subcommands = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Folder, ZIP or file with Spotify's Streaming_History_Audio_*.json files
    #[arg(long, value_name = "DIR")]
    spotify: Option<PathBuf>,

    /// Folder, ZIP or file with the YouTube history from Google Takeout (watch-history.json or My Activity.json)
    #[arg(long, value_name = "PATH")]
    ytm: Option<PathBuf>,

    /// Sources included in the stats
    #[arg(long, value_enum, default_value_t = SourceArg::All)]
    source: SourceArg,

    /// Whole calendar year (shortcut for --from YYYY-01-01 --to YYYY-12-31)
    #[arg(long, value_name = "YYYY", conflicts_with_all = ["from", "to"])]
    year: Option<i32>,

    /// Start date, inclusive (YYYY-MM-DD)
    #[arg(long, value_name = "YYYY-MM-DD")]
    from: Option<NaiveDate>,

    /// End date, inclusive (YYYY-MM-DD)
    #[arg(long, value_name = "YYYY-MM-DD")]
    to: Option<NaiveDate>,

    /// Size of the top lists in the terminal (the image shows up to 5)
    #[arg(long, default_value_t = 5)]
    top: usize,

    /// How to rank the top lists
    #[arg(long, value_enum, default_value_t = RankArg::Plays)]
    rank_by: RankArg,

    /// Accent color of the image
    #[arg(long, value_enum, default_value_t = ColorArg::Orange)]
    color: ColorArg,

    /// Time zone used to group by day and month
    #[arg(long, default_value = "Europe/Madrid")]
    tz: Tz,

    /// Estimated minutes per YouTube Music play
    #[arg(long, default_value_t = 3.5)]
    ytm_avg_minutes: f64,

    #[command(flatten)]
    db: DbArgs,

    /// Output PNG
    #[arg(long, value_name = "FILE", default_value = "wrapped.png")]
    out: PathBuf,

    /// Replace the output PNG if it already exists
    #[arg(long)]
    overwrite: bool,

    /// Only print the stats in the terminal
    #[arg(long)]
    no_image: bool,
}

/// Where the song database is read from.
#[derive(Args)]
struct DbArgs {
    /// Song database with the song length of YouTube Music plays: a URL or a local file [default: the one in the mw repository]
    #[arg(long, value_name = "URL|PATH", conflicts_with = "offline")]
    db: Option<String>,

    /// Use the cached copy of the song database instead of downloading it
    #[arg(long)]
    offline: bool,
}

impl DbArgs {
    fn origin(&self) -> songdb::Origin<'_> {
        songdb::Origin::new(self.db.as_deref(), self.offline)
    }
}

/// Commands other than the recap.
#[derive(Subcommand)]
enum Command {
    /// Song database tools
    #[command(subcommand)]
    Db(DbCommand),
}

/// `mw db` commands.
#[derive(Subcommand)]
enum DbCommand {
    /// Write the song lengths of your Spotify full plays to a file you can share, without your listening history
    ExportSpotify {
        /// Folder, ZIP or file with Spotify's Streaming_History_Audio_*.json files
        #[arg(long, value_name = "DIR")]
        spotify: PathBuf,

        #[command(flatten)]
        db: DbArgs,
    },

    /// Write the YouTube Music songs the song database doesn't have to a file you can share, without your listening history
    ExportYtm {
        /// Folder, ZIP or file with the YouTube history from Google Takeout (watch-history.json or My Activity.json)
        #[arg(long, value_name = "PATH")]
        ytm: PathBuf,

        #[command(flatten)]
        db: DbArgs,
    },

    /// Look up in MusicBrainz the songs of a contribution file that have a "check", editing the file (for the maintainer)
    #[command(hide = true)]
    Resolve {
        /// A contribution file (written by `mw db export-spotify` or `export-ytm`)
        #[arg(long, value_name = "PATH")]
        file: PathBuf,

        /// Look up at most this many songs (each takes over a second)
        #[arg(long, value_name = "N")]
        limit: Option<usize>,
    },

    /// Add the songs of a contribution file to the song database file (for the maintainer)
    #[command(hide = true)]
    Import {
        /// A contribution file (written by `mw db export-spotify` or `export-ytm`)
        #[arg(long, value_name = "PATH")]
        file: PathBuf,

        /// Song database file to add the songs to
        #[arg(long, value_name = "PATH", default_value = "data/songs.json")]
        songs: PathBuf,

        /// Only print what would be added
        #[arg(long)]
        dry_run: bool,

        /// Also list the songs added and skipped, not only how many
        #[arg(long)]
        list: bool,
    },
}

/// Values of `--source`: which services count towards the stats.
#[derive(Clone, Copy, ValueEnum)]
enum SourceArg {
    All,
    Spotify,
    Ytm,
}

/// Values of `--color`: the accent used for the minutes and each song's figure.
#[derive(Clone, Copy, ValueEnum)]
enum ColorArg {
    Orange,
    Purple,
    Blue,
    Green,
    Pink,
    Red,
    Gray,
}

impl ColorArg {
    /// The color as an SVG hex string.
    fn hex(self) -> &'static str {
        match self {
            ColorArg::Orange => "#FF6B4A",
            ColorArg::Purple => "#B388FF",
            ColorArg::Blue => "#5AA9FF",
            ColorArg::Green => "#3DDC97",
            ColorArg::Pink => "#FF6FB5",
            ColorArg::Red => "#FF5A5F",
            ColorArg::Gray => "#E3E7EB",
        }
    }
}

/// Values of `--rank-by`: what orders the top lists and which figure the image shows.
#[derive(Clone, Copy, ValueEnum)]
enum RankArg {
    Minutes,
    Plays,
}

/// Runs the program: loads the plays, prints the stats and saves the image unless
/// `--no-image` was given.
///
/// Fails on invalid paths or dates, when the image already exists and `--overwrite`
/// wasn't given, or when no plays are left after filtering.
fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Some(Command::Db(cmd)) = &cli.command {
        return match cmd {
            DbCommand::ExportSpotify { spotify, db } => export_spotify(spotify, db.origin()),
            DbCommand::ExportYtm { ytm, db } => export_ytm(ytm, db.origin()),
            DbCommand::Resolve { file, limit } => resolve(file, *limit),
            DbCommand::Import {
                file,
                songs,
                dry_run,
                list,
            } => import(file, songs, *dry_run, *list),
        };
    }

    if cli.spotify.is_none() && cli.ytm.is_none() {
        bail!("Provide at least one source: --spotify <folder> and/or --ytm <watch-history.json>");
    }
    let source = match cli.source {
        SourceArg::All => None,
        SourceArg::Spotify => Some(Source::Spotify),
        SourceArg::Ytm => Some(Source::YouTubeMusic),
    };
    if source == Some(Source::Spotify) && cli.spotify.is_none() {
        bail!("--source spotify requires --spotify <folder>");
    }
    if source == Some(Source::YouTubeMusic) && cli.ytm.is_none() {
        bail!("--source ytm requires --ytm <watch-history.json>");
    }

    let (from, to) = match cli.year {
        Some(y) => (
            Some(NaiveDate::from_ymd_opt(y, 1, 1).ok_or_else(|| anyhow::anyhow!("invalid year"))?),
            NaiveDate::from_ymd_opt(y, 12, 31),
        ),
        None => (cli.from, cli.to),
    };
    if let (Some(f), Some(t)) = (from, to)
        && f > t
    {
        bail!("--from ({f}) is after --to ({t})");
    }
    if !cli.no_image && !cli.overwrite && cli.out.exists() {
        bail!(
            "{} already exists; use --overwrite or choose another name with --out",
            cli.out.display()
        );
    }

    // Load sources
    let mut plays: Vec<Play> = Vec::new();
    if wants(source, Source::Spotify)
        && let Some(dir) = &cli.spotify
    {
        plays.extend(load_spotify(dir)?);
    }
    if wants(source, Source::YouTubeMusic)
        && let Some(file) = &cli.ytm
    {
        plays.extend(load_ytm(file, cli.ytm_avg_minutes, cli.db.origin())?);
    }

    let filters = Filters {
        from,
        to,
        source,
        tz: cli.tz,
    };
    let rank_by = match cli.rank_by {
        RankArg::Minutes => RankBy::Minutes,
        RankArg::Plays => RankBy::Plays,
    };
    let Some(stats) = stats::compute(&plays, &filters, cli.top.max(1), rank_by) else {
        bail!("No plays in that date range.");
    };

    print!("\n{}", text::stats(&stats));

    if !cli.no_image {
        let svg = render::build_svg(&stats, rank_by, cli.color.hex())?;
        let png = render::png(&svg)?;
        fs::write(&cli.out, png).with_context(|| format!("cannot save {}", cli.out.display()))?;
        println!("\nImage saved to {}", cli.out.display());
    }
    Ok(())
}

/// Loads the Spotify export and reports how many plays were read.
fn load_spotify(path: &Path) -> Result<Vec<Play>> {
    let found = sources::read(&read_path(path, "Spotify")?, 0.0)?;
    let Some(loaded) = found.spotify else {
        bail!(
            "No Spotify files found in {}. Looking for Streaming_History_Audio_*.json (extended history) or StreamingHistory_music_*.json.",
            path.display()
        );
    };
    report("Spotify", loaded.plays.len(), &loaded.warnings);
    Ok(loaded.plays)
}

/// Reads the files at `path` (a folder or a file) of the `service` export.
fn read_path(path: &Path, service: &str) -> Result<Vec<sources::File>> {
    if !path.exists() {
        bail!("{service} path does not exist: {}", path.display());
    }
    sources::read_path(path)
}

/// Reads the YouTube history at `path` (a folder or a file), counting each
/// YouTube Music play as `avg_minutes`.
fn read_ytm(path: &Path, avg_minutes: f64) -> Result<sources::Loaded> {
    let found = sources::read(&read_path(path, "YouTube Music")?, avg_minutes)?;
    let Some(loaded) = found.youtube else {
        bail!(
            "No YouTube history found in {}. Looking for watch-history.json or My Activity.json from Google Takeout, in JSON format.",
            path.display()
        );
    };
    report("YouTube Music", loaded.plays.len(), &loaded.warnings);
    Ok(loaded)
}

/// Loads the YouTube Music history and replaces the estimates with the real
/// lengths in the song database, when it can be read.
fn load_ytm(path: &Path, avg_minutes: f64, db: songdb::Origin) -> Result<Vec<Play>> {
    let mut loaded = read_ytm(path, avg_minutes)?;
    let (db, warnings) = songdb::load(db, songdb::cache_path().as_deref());
    if let Some(db) = db {
        let matched = songdb::apply(&mut loaded.plays, &db);
        eprintln!(
            "YouTube Music: song length for {} of {} from the song database",
            fmt_int(matched as u64),
            fmt_count(loaded.plays.len() as u64, "play")
        );
    }
    for w in &warnings {
        eprintln!("  warning: {w}");
    }
    Ok(loaded.plays)
}

/// Writes the song lengths of the Spotify full plays that the song database
/// doesn't have to `spotify-durations-<local date and time>.json`, the unverified
/// ones with a `check`.
fn export_spotify(dir: &Path, origin: songdb::Origin) -> Result<()> {
    let plays =
        sources::spotify::full_plays(&read_path(dir, "Spotify")?, &dir.display().to_string())?;
    let (db, warnings) = songdb::load(origin, songdb::cache_path().as_deref());
    for w in &warnings {
        eprintln!("warning: {w}");
    }
    if db.is_none() {
        eprintln!("warning: no song database; exporting every song");
    }
    let m = lengths::measure(&plays, |k| db.as_ref().is_some_and(|d| d.contains(k)));

    write_contribution("spotify-durations", &m.songs)?;
    print_songs("  to check, only one full play", m.single, [], false);
    print_songs(
        "  to check, full plays don't agree on a length",
        m.inconsistent,
        [],
        false,
    );
    print_songs("Already in the song database", m.known, [], false);
    if m.single + m.inconsistent > 0 {
        println!(
            "Songs to check have a \"check\" field: set \"seconds\" if needed (seconds or \"m:ss\") and remove \"check\" to import them."
        );
    }
    Ok(())
}

/// Writes the YouTube Music songs the song database doesn't have to
/// `ytm-missing-<local date and time>.json`.
fn export_ytm(file: &Path, origin: songdb::Origin) -> Result<()> {
    let loaded = read_ytm(file, 0.0)?;
    let (db, warnings) = songdb::load(origin, songdb::cache_path().as_deref());
    for w in &warnings {
        eprintln!("warning: {w}");
    }
    if db.is_none() {
        eprintln!("warning: no song database; exporting every song");
    }
    let songs = songdb::missing(&loaded.plays, db.as_ref());
    write_contribution("ytm-missing", &songs)?;
    println!("Send this file to the maintainer.");
    Ok(())
}

/// Writes `songs` to `<prefix>-<local date and time>.json` and says so.
fn write_contribution(prefix: &str, songs: &[lengths::SongLength]) -> Result<()> {
    let file = format!(
        "{prefix}-{}.json",
        chrono::Local::now().format("%Y-%m-%d-%H%M%S")
    );
    let json = serde_json::to_string_pretty(songs)? + "\n";
    fs::write(&file, json).with_context(|| format!("cannot write {file}"))?;
    println!("Wrote {} to {file}", fmt_count(songs.len() as u64, "song"));
    Ok(())
}

/// Looks up the songs to check of a contribution file in MusicBrainz, printing
/// the songs to review as they come and, in a terminal, one status line that
/// updates in place.
fn resolve(file: &Path, limit: Option<usize>) -> Result<()> {
    let mut client =
        musicbrainz::Client::new(musicbrainz::URL, musicbrainz::DELAY, musicbrainz::BACKOFF);
    let tty = std::io::stdout().is_terminal();
    let r = musicbrainz::resolve(file, &mut client, limit, |note, status| {
        let mut out = std::io::stdout().lock();
        // Return to the start of the status line and clear it.
        let clear = if tty { "\r\x1b[K" } else { "" };
        let end = if tty { "" } else { "\n" };
        if let Some(note) = note {
            let _ = writeln!(out, "{clear}{note}");
        }
        let _ = write!(out, "{clear}{status}{end}");
        let _ = out.flush();
    })?;
    if r.found + r.far.len() + r.not_found.len() == 0 {
        println!("Nothing to resolve: every song to check was already looked up.");
        return Ok(());
    }
    if tty {
        println!();
    }
    println!("\nFound in MusicBrainz: {}", fmt_int(r.found as u64));
    print_songs(
        "To check, far from the lengths played",
        r.far.len(),
        [],
        false,
    );
    print_songs("Not in MusicBrainz", r.not_found.len(), [], false);
    if r.left > 0 {
        println!(
            "Still to look up: {} (run again to continue)",
            fmt_int(r.left as u64)
        );
    }
    Ok(())
}

/// Adds the songs of a contribution file to the song database file, and counts
/// (with `list`, lists) what it added and skipped.
fn import(path: &Path, file: &Path, dry_run: bool, list: bool) -> Result<()> {
    let songs = read_contribution(path)?;
    let r = songdb::import(file, songs, dry_run)?;

    let verb = if dry_run { "Would add to" } else { "Added to" };
    let length = |s: &lengths::SongLength| {
        format!(
            "{} · {}  ({})",
            s.artist,
            s.title,
            lengths::mmss(s.seconds.unwrap_or(0))
        )
    };
    print_songs(
        &format!("{verb} {}", file.display()),
        r.added.len(),
        r.added.iter().map(length),
        list,
    );
    print_songs("Already present", r.present, [], false);
    let (lo, hi) = (songdb::IMPORT_RANGE.start(), songdb::IMPORT_RANGE.end());
    print_songs(
        &format!("Skipped, length outside {lo}–{hi} s"),
        r.out_of_range.len(),
        r.out_of_range.iter().map(length),
        list,
    );
    print_songs("Skipped, no artist or title", r.unnamed, [], false);
    if r.to_check > 0 {
        eprintln!(
            "warning: skipped {} still marked \"check\"",
            fmt_count(r.to_check as u64, "song")
        );
    }
    Ok(())
}

/// Reads a contribution file.
fn read_contribution(path: &Path) -> Result<Vec<lengths::SongLength>> {
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    serde_json::from_str(&text)
        .with_context(|| format!("{} is not a contribution file", path.display()))
}

/// Prints `heading: <n>` unless `n` is 0, and with `list` one line per song.
fn print_songs(heading: &str, n: usize, lines: impl IntoIterator<Item = String>, list: bool) {
    if n == 0 {
        return;
    }
    println!("{heading}: {}", fmt_int(n as u64));
    if list {
        for line in lines {
            println!("  {line}");
        }
    }
}

/// Is this source included? `None` means all sources.
fn wants(filter: Option<Source>, s: Source) -> bool {
    filter.is_none_or(|f| f == s)
}

/// Prints to stderr how many plays a source loaded and its warnings.
fn report(name: &str, n: usize, warnings: &[String]) {
    eprintln!("{name}: {} loaded", fmt_count(n as u64, "play"));
    for w in warnings {
        eprintln!("  warning: {w}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wants_matches_filter() {
        assert!(wants(None, Source::Spotify));
        assert!(wants(None, Source::YouTubeMusic));
        assert!(wants(Some(Source::Spotify), Source::Spotify));
        assert!(!wants(Some(Source::Spotify), Source::YouTubeMusic));
        assert!(!wants(Some(Source::YouTubeMusic), Source::Spotify));
    }
}

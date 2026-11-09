//! Music Wrapped (`mw`): a listening recap from Spotify and YouTube Music data
//! exports.
//!
//! This is the command line entry point: it parses the arguments, loads the
//! selected sources, computes the stats, prints them and saves the image.

mod lengths;
mod model;
mod normalize;
mod render;
mod songdb;
mod sources;
mod stats;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use chrono::NaiveDate;
use chrono_tz::Tz;
use clap::{Args, Parser, Subcommand, ValueEnum};

use model::{Play, Source};
use render::{fmt_count, fmt_int, minutes, range_label};
use stats::{Filters, RankBy, Stats};

/// Music Wrapped: your listening recap from your Spotify and YouTube Music data exports.
#[derive(Parser)]
#[command(name = "mw", version, args_conflicts_with_subcommands = true)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Folder (or file) with Spotify's Streaming_History_Audio_*.json files
    #[arg(long, value_name = "DIR")]
    spotify: Option<PathBuf>,

    /// Path to watch-history.json from Google Takeout
    #[arg(long, value_name = "FILE")]
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
        /// Folder (or file) with Spotify's Streaming_History_Audio_*.json files
        #[arg(long, value_name = "DIR")]
        spotify: PathBuf,

        #[command(flatten)]
        db: DbArgs,
    },

    /// Write the YouTube Music songs the song database doesn't have to a file you can share, without your listening history
    ExportYtm {
        /// Path to watch-history.json from Google Takeout
        #[arg(long, value_name = "FILE")]
        ytm: PathBuf,

        #[command(flatten)]
        db: DbArgs,
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

    print_stats(&stats);

    if !cli.no_image {
        let svg = render::build_svg(&stats, rank_by, cli.color.hex())?;
        render::save_png(&svg, &cli.out)?;
        println!("\nImage saved to {}", cli.out.display());
    }
    Ok(())
}

/// Loads the Spotify export and reports how many plays were read.
fn load_spotify(dir: &Path) -> Result<Vec<Play>> {
    let loaded = sources::spotify::load(dir)?;
    report("Spotify", loaded.plays.len(), &loaded.warnings);
    Ok(loaded.plays)
}

/// Loads the YouTube Music history and replaces the estimates with the real
/// lengths in the song database, when it can be read.
fn load_ytm(file: &Path, avg_minutes: f64, db: songdb::Origin) -> Result<Vec<Play>> {
    let mut loaded = sources::ytmusic::load(file, avg_minutes)?;
    report("YouTube Music", loaded.plays.len(), &loaded.warnings);

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
    let plays = sources::spotify::full_plays(dir)?;
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
    let loaded = sources::ytmusic::load(file, 0.0)?;
    report("YouTube Music", loaded.plays.len(), &loaded.warnings);
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

/// Prints the full recap to stdout, including what the image leaves out (every
/// month, longer top lists and the favorite hour).
fn print_stats(s: &Stats) {
    let approx = if s.estimated_ms > 0 { "≈ " } else { "" };
    println!("\n== {} ==", range_label(s.from, s.to));
    println!(
        "Minutes listened: {approx}{}  ({})",
        fmt_int(minutes(s.total_ms)),
        fmt_count(s.plays, "play")
    );
    if s.estimated_ms > 0 {
        println!(
            "  of which estimated (YouTube Music): {}",
            fmt_int(minutes(s.estimated_ms))
        );
    }
    for (src, ms) in &s.by_source {
        let pct = *ms as f64 * 100.0 / s.total_ms as f64;
        println!(
            "  {}: {} min ({pct:.0}%)",
            src.label(),
            fmt_int(minutes(*ms))
        );
    }

    println!("\nBy month:");
    for ((y, m), ms) in &s.by_month {
        println!("  {y}-{m:02}  {:>8} min", fmt_int(minutes(*ms)));
    }

    println!("\nTop artists:");
    for (i, a) in s.top_artists.iter().enumerate() {
        println!(
            "  {:>2}. {}  ({} min, {})",
            i + 1,
            a.name,
            fmt_int(minutes(a.ms)),
            fmt_count(a.plays, "play")
        );
    }
    println!("\nTop songs:");
    for (i, t) in s.top_tracks.iter().enumerate() {
        let artist = t.detail.as_deref().unwrap_or("");
        println!(
            "  {:>2}. {} · {}  ({} min, {})",
            i + 1,
            t.name,
            artist,
            fmt_int(minutes(t.ms)),
            fmt_count(t.plays, "play")
        );
    }

    println!(
        "\nDistinct artists: {} · Distinct songs: {}",
        fmt_int(s.distinct_artists as u64),
        fmt_int(s.distinct_tracks as u64)
    );
    if let Some((day, ms)) = s.top_day {
        println!("Top day: {day} ({} min)", fmt_int(minutes(ms)));
    }
    if let Some((h, _)) = s.top_hour {
        println!("Favorite hour: {h:02}:00–{:02}:00", (h + 1) % 24);
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

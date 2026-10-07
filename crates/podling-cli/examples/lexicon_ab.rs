//! Compares two runs of one episode, without and with the pronunciation list,
//! and prepares a blind listening pack of the chunks the list changed.
//!
//! ```text
//! cargo run --release -p podling-cli --example lexicon_ab -- \
//!     --off <out dir> --on <out dir> [--cache-dir <dir> --pack <dir>]
//! ```
//!
//! This is the check behind the acceptance of `docs/ideas/natural-episode-speech.md`
//! ("WER and retry rate no worse, and a blind A/B listening pass"). It is an
//! example, not a `podling` subcommand, so the shipped binary gains nothing
//! for a one-off check.
//!
//! The exit code is the verdict: 0 pass, 1 fail, 2 inconclusive (the list
//! changed no chunk), 3 error.

use std::collections::BTreeMap;
use std::collections::hash_map::RandomState;
use std::fmt::Write as _;
use std::fs;
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::Parser;
use podling_core::DiskCache;
use podling_core::cache::BlobStore;
use podling_types::{
    ArtifactKind, AudioManifest, ChunkRecord, ContentHash, Envelope, SCHEMA_VERSION, Script,
    TurnRange,
};

/// The idea's kill criterion: "mean WER rises above 10 ‰".
const MAX_MEAN_WER_PM: u64 = 10;

#[derive(Debug, Parser)]
#[command(about = "Compare two runs of an episode, without and with the pronunciation list")]
struct Args {
    /// The output directory of the run without the pronunciation list.
    #[arg(long)]
    off: PathBuf,

    /// The output directory of the run with it.
    #[arg(long)]
    on: PathBuf,

    /// The cache both runs shared; it holds the chunk audio.
    #[arg(long)]
    cache_dir: Option<PathBuf>,

    /// Write the blind listening pack here: a new or empty directory.
    #[arg(long, requires = "cache_dir")]
    pack: Option<PathBuf>,
}

fn main() -> ExitCode {
    match run(Args::parse()) {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            eprintln!("error: {err:#}");
            ExitCode::from(3)
        }
    }
}

fn run(args: Args) -> Result<u8> {
    let (off, on) = (load_audio(&args.off)?, load_audio(&args.on)?);
    let (off_script, on_script) = (load_script(&args.off)?, load_script(&args.on)?);
    if off_script != on_script {
        bail!(
            "{} and {} hold different scripts, so chunk ids differ for more than the lexicon",
            args.off.join("script.json").display(),
            args.on.join("script.json").display()
        );
    }
    let pairs = pair_chunks(&off, &on)?;
    let (text, verdict) = report(&pairs);
    print!("{text}");

    if let (Some(pack), Some(cache_dir)) = (&args.pack, &args.cache_dir) {
        let items = plan_pack(&pairs, &mut random_coin());
        if items.is_empty() {
            println!("blind pack: nothing to listen to, the list changed no chunk");
        } else {
            let blobs = DiskCache::new(cache_dir).blobs();
            write_pack(pack, &items, &blobs, &off_script)?;
            println!("blind pack: {} pairs in {}", items.len(), pack.display());
        }
    }
    Ok(verdict.exit_code())
}

// --- loading ---------------------------------------------------------------

/// Reads `<dir>/<kind>.json` and returns the artifact's body.
///
/// `Envelope<serde_json::Value>` reads the wrapper (`schema_version`, `kind`)
/// without knowing the body's type yet, so the kind can be checked first and
/// a wrong file is named in the error before any body parsing is attempted.
fn read_artifact(dir: &Path, kind: ArtifactKind) -> Result<serde_json::Value> {
    let path = dir.join(format!("{}.json", kind.as_str()));
    let bytes = fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    let envelope: Envelope<serde_json::Value> = serde_json::from_slice(&bytes)
        .with_context(|| format!("{} is not an artifact", path.display()))?;
    if envelope.kind != kind {
        bail!(
            "{} is a {} artifact, not {}",
            path.display(),
            envelope.kind.as_str(),
            kind.as_str()
        );
    }
    if envelope.schema_version != SCHEMA_VERSION {
        bail!(
            "{} has schema version {}, this build reads {SCHEMA_VERSION}",
            path.display(),
            envelope.schema_version
        );
    }
    Ok(envelope.body)
}

fn load_audio(dir: &Path) -> Result<AudioManifest> {
    let body = read_artifact(dir, ArtifactKind::Audio)?;
    serde_json::from_value(body).with_context(|| {
        format!(
            "{} does not hold an audio manifest",
            dir.join("audio.json").display()
        )
    })
}

fn load_script(dir: &Path) -> Result<Script> {
    let body = read_artifact(dir, ArtifactKind::Script)?;
    serde_json::from_value(body).with_context(|| {
        format!(
            "{} does not hold a script",
            dir.join("script.json").display()
        )
    })
}

// --- comparing -------------------------------------------------------------

/// One chunk as the two runs made it.
struct Pair<'a> {
    off: &'a ChunkRecord,
    on: &'a ChunkRecord,
}

impl Pair<'_> {
    /// The lexicon changed this chunk: a chunk's id hashes its turns' respelt
    /// text, so the id differs exactly when a name in the chunk (or in the
    /// context it is given) was respelt, and so does the seed.
    fn affected(&self) -> bool {
        self.off.id != self.on.id
    }
}

/// Pairs the chunks of the two runs by the turns they cover, which do not
/// depend on the lexicon. Refuses runs that were not chunked alike.
fn pair_chunks<'a>(off: &'a AudioManifest, on: &'a AudioManifest) -> Result<Vec<Pair<'a>>> {
    if off.chunks.len() != on.chunks.len() {
        bail!(
            "the runs have {} and {} chunks, so they were not made from the same script",
            off.chunks.len(),
            on.chunks.len()
        );
    }
    off.chunks
        .iter()
        .zip(&on.chunks)
        .enumerate()
        .map(|(index, (off, on))| {
            if off.turns != on.turns {
                bail!(
                    "chunk {index} covers turns {}..{} in one run and {}..{} in the other",
                    off.turns.start(),
                    off.turns.end(),
                    on.turns.start(),
                    on.turns.end()
                );
            }
            Ok(Pair { off, on })
        })
        .collect()
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Stats {
    chunks: usize,
    wer_sum: u64,
    /// Chunks whose kept take is not the first. Banter chunks start with
    /// several takes, so this also counts a banter pick, not only a retry.
    retried: usize,
    unverified: usize,
}

impl Stats {
    /// `fold` threads one accumulator through the iterator: each chunk updates
    /// the running totals and hands them on, so one pass gives every figure.
    fn of<'a>(chunks: impl Iterator<Item = &'a ChunkRecord>) -> Self {
        chunks.fold(Self::default(), |mut stats, chunk| {
            stats.chunks += 1;
            stats.wer_sum += u64::from(chunk.wer_pm.get());
            stats.retried += usize::from(chunk.take > 0);
            stats.unverified += usize::from(!chunk.verified);
            stats
        })
    }

    /// `None` for no chunks, rather than a `NaN` from dividing by zero.
    fn mean_wer_pm(&self) -> Option<f64> {
        (self.chunks > 0).then(|| self.wer_sum as f64 / self.chunks as f64)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    Pass,
    Fail(Vec<String>),
    /// The list changed no chunk, so the two runs cannot differ.
    Inconclusive,
}

impl Verdict {
    fn exit_code(&self) -> u8 {
        match self {
            Verdict::Pass => 0,
            Verdict::Fail(_) => 1,
            Verdict::Inconclusive => 2,
        }
    }
}

/// The idea's kill criterion: "mean WER rises above 10 ‰, or the retry rate
/// rises". The mean is compared as a sum against `10 × chunks`, so no float
/// rounding decides a verdict.
fn verdict(off: &Stats, on: &Stats, affected: usize) -> Verdict {
    if affected == 0 {
        return Verdict::Inconclusive;
    }
    let mut causes = Vec::new();
    if on.wer_sum > MAX_MEAN_WER_PM * on.chunks as u64 {
        causes.push(format!(
            "the run with the list has a mean WER of {:.1} ‰, above {MAX_MEAN_WER_PM} ‰",
            on.mean_wer_pm().unwrap_or_default()
        ));
    }
    if on.retried > off.retried {
        causes.push(format!(
            "the run with the list kept a later take in {} chunks, against {} without it",
            on.retried, off.retried
        ));
    }
    if causes.is_empty() {
        Verdict::Pass
    } else {
        Verdict::Fail(causes)
    }
}

fn mean_cell(stats: &Stats) -> String {
    stats
        .mean_wer_pm()
        .map_or_else(|| "-".to_owned(), |mean| format!("{mean:.1}"))
}

fn stats_row(out: &mut String, set: &str, arm: &str, stats: &Stats) {
    // Writing to a `String` cannot fail, so the `fmt::Result` is dropped.
    let _ = writeln!(
        out,
        "{set:<10} {arm:<4} {:>6} {:>11} {:>8} {:>11}",
        stats.chunks,
        mean_cell(stats),
        stats.retried,
        stats.unverified
    );
}

/// The printed comparison and its verdict.
fn report(pairs: &[Pair]) -> (String, Verdict) {
    let affected: Vec<&Pair> = pairs.iter().filter(|pair| pair.affected()).collect();
    let (off_all, on_all) = (
        Stats::of(pairs.iter().map(|p| p.off)),
        Stats::of(pairs.iter().map(|p| p.on)),
    );
    let (off_some, on_some) = (
        Stats::of(affected.iter().map(|p| p.off)),
        Stats::of(affected.iter().map(|p| p.on)),
    );

    let mut out = String::new();
    let _ = writeln!(
        out,
        "{} chunks paired by turns; {} changed by the lexicon (their id differs)\n",
        pairs.len(),
        affected.len()
    );
    let _ = writeln!(
        out,
        "{:<10} {:<4} {:>6} {:>11} {:>8} {:>11}",
        "", "run", "chunks", "mean WER ‰", "take > 0", "unverified"
    );
    stats_row(&mut out, "all", "off", &off_all);
    stats_row(&mut out, "all", "on", &on_all);
    stats_row(&mut out, "changed", "off", &off_some);
    stats_row(&mut out, "changed", "on", &on_some);

    if !affected.is_empty() {
        let _ = writeln!(out, "\nchanged chunks (take, WER ‰, verified)");
        for pair in &affected {
            let cell = |chunk: &ChunkRecord| {
                format!(
                    "{} {:>4} {}",
                    chunk.take,
                    chunk.wer_pm.get(),
                    if chunk.verified { "yes" } else { "no" }
                )
            };
            let _ = writeln!(
                out,
                "turns {:>3}..{:<3} off: {:<12} on: {}",
                pair.off.turns.start(),
                pair.off.turns.end(),
                cell(pair.off),
                cell(pair.on)
            );
        }
    }

    let verdict = verdict(&off_all, &on_all, affected.len());
    let _ = match &verdict {
        Verdict::Pass => writeln!(out, "\nverdict: PASS"),
        Verdict::Inconclusive => writeln!(
            out,
            "\nverdict: INCONCLUSIVE, the list changed no chunk (no name from it in the script?)"
        ),
        Verdict::Fail(causes) => writeln!(out, "\nverdict: FAIL\n  - {}", causes.join("\n  - ")),
    };
    let _ = writeln!(
        out,
        "\ncaveats:\n\
         \x20 - one deterministic sample per run: only the {} changed chunks differ, so this is evidence, not significance\n\
         \x20 - the run with the list scores its WER with the list's `heard` variants; the other run has none\n\
         \x20 - `take > 0` also counts banter picks, which overcounts retries",
        affected.len()
    );
    (out, verdict)
}

// --- the blind pack --------------------------------------------------------

/// One pair to listen to: which blob is X and which is Y was decided at
/// random, and only `on_is_x` (never printed, only written to `key.json`)
/// remembers it.
struct PackItem<'a> {
    number: usize,
    turns: TurnRange,
    x: &'a ContentHash,
    y: &'a ContentHash,
    on_is_x: bool,
}

/// `coin` decides, per pair, whether the run with the list is X.
fn plan_pack<'a>(pairs: &[Pair<'a>], coin: &mut impl FnMut() -> bool) -> Vec<PackItem<'a>> {
    pairs
        .iter()
        .filter(|pair| pair.affected())
        .enumerate()
        .map(|(index, pair)| {
            let on_is_x = coin();
            let (x, y) = if on_is_x {
                (&pair.on.blob, &pair.off.blob)
            } else {
                (&pair.off.blob, &pair.on.blob)
            };
            PackItem {
                number: index + 1,
                turns: pair.off.turns,
                x,
                y,
                on_is_x,
            }
        })
        .collect()
}

/// A fair coin without a new dependency. `RandomState` seeds each hasher from
/// the operating system's randomness (it is what makes `HashMap` order
/// unpredictable), so the low bit of an empty hash is unpredictable too.
fn random_coin() -> impl FnMut() -> bool {
    || RandomState::new().build_hasher().finish() & 1 == 1
}

const SHEET_HEAD: &str = "# Which one is better?\n\n\
    For each pair, listen to X and Y (same words, said twice) and fill in the three lines.\n\
    Do not open key.json until you have finished.\n\n";

/// Copies each pair's two chunks to `NN-X.wav` and `NN-Y.wav`, and writes the
/// rating sheet and the key. Nothing in the sheet or the file names depends on
/// which run is which.
fn write_pack(dir: &Path, items: &[PackItem], blobs: &BlobStore, script: &Script) -> Result<()> {
    let in_use = dir.exists()
        && fs::read_dir(dir)
            .with_context(|| format!("reading {}", dir.display()))?
            .next()
            .is_some();
    if in_use {
        bail!(
            "{} is not empty; the pack needs a new or empty directory",
            dir.display()
        );
    }
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;

    let mut sheet = String::from(SHEET_HEAD);
    let mut key = BTreeMap::new();
    for item in items {
        let number = format!("{:02}", item.number);
        for (side, hash) in [("X", item.x), ("Y", item.y)] {
            let from = blobs.path_for(hash);
            fs::copy(&from, dir.join(format!("{number}-{side}.wav")))
                .with_context(|| format!("copying {}", from.display()))?;
        }
        key.insert(number.clone(), if item.on_is_x { "X" } else { "Y" });

        let Some(turns) = script.turns().get(item.turns.indices()) else {
            bail!(
                "the script has no turns {}..{}",
                item.turns.start(),
                item.turns.end()
            );
        };
        let _ = writeln!(sheet, "## {number}\n");
        for turn in turns {
            let _ = writeln!(sheet, "> {}: {}", turn.speaker.0, turn.text);
        }
        let _ = writeln!(
            sheet,
            "\n- Prefer: X / Y / same\n- Names said right: X / Y / both / neither\n- Notes:\n"
        );
    }
    fs::write(dir.join("ratings.md"), sheet)?;
    fs::write(dir.join("key.json"), serde_json::to_string_pretty(&key)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use podling_types::{Emotion, EpisodeAudio, Pace, PerMille, Speaker, SpeakerId, Turn};

    fn hash(tag: &str) -> ContentHash {
        ContentHash::of_parts(&[tag.as_bytes()])
    }

    /// A chunk over `start..end`; `id` and `blob` are named by tag.
    fn chunk(
        id: &str,
        (start, end): (usize, usize),
        blob: &ContentHash,
        take: u8,
        wer: u16,
    ) -> ChunkRecord {
        ChunkRecord {
            id: hash(id),
            turns: TurnRange::new(start, end).unwrap(),
            blob: blob.clone(),
            seed: 1,
            take,
            wer_pm: PerMille::new(wer).unwrap(),
            quote_misses: Vec::new(),
            verified: true,
        }
    }

    fn manifest(chunks: Vec<ChunkRecord>) -> AudioManifest {
        AudioManifest {
            sample_rate: 48_000,
            chunks,
            episode: EpisodeAudio {
                path: "episode.wav".into(),
                duration_ms: 1000,
                integrated_lufs: -16.0,
                true_peak_dbtp: -1.5,
                encoded: None,
            },
            voices: Vec::new(),
        }
    }

    fn script(texts: &[&str]) -> Script {
        let speaker = SpeakerId("host".to_owned());
        let turns = texts
            .iter()
            .map(|text| Turn {
                speaker: speaker.clone(),
                text: (*text).to_owned(),
                emotion: Emotion::Neutral,
                citations: Vec::new(),
                quotes: Vec::new(),
                pace: Pace::Normal,
                nonverbal: Vec::new(),
                callback_to: None,
            })
            .collect();
        let cast = vec![Speaker {
            id: speaker,
            name: "Mara".to_owned(),
            role: "host".to_owned(),
        }];
        Script::new(cast, turns).unwrap()
    }

    /// Chunk 0 is the same in both runs; chunk 1 differs (a respelt name).
    fn two_runs() -> (AudioManifest, AudioManifest) {
        let (plain, kulik_off, kulik_on) = (hash("plain"), hash("kulik-off"), hash("kulik-on"));
        let off = manifest(vec![
            chunk("a", (0, 1), &plain, 0, 4),
            chunk("b-off", (1, 2), &kulik_off, 1, 72),
        ]);
        let on = manifest(vec![
            chunk("a", (0, 1), &plain, 0, 4),
            chunk("b-on", (1, 2), &kulik_on, 0, 6),
        ]);
        (off, on)
    }

    #[test]
    fn chunks_pair_by_turns_and_differ_by_id_when_affected() {
        let (off, on) = two_runs();
        let pairs = pair_chunks(&off, &on).unwrap();
        let affected: Vec<bool> = pairs.iter().map(Pair::affected).collect();
        assert_eq!(affected, [false, true]);
    }

    #[test]
    fn runs_with_different_chunk_counts_are_refused() {
        let (off, mut on) = two_runs();
        on.chunks.pop();
        let err = pair_chunks(&off, &on).err().unwrap().to_string();
        assert!(err.contains("2 and 1 chunks"), "{err}");
    }

    #[test]
    fn a_turns_mismatch_names_the_first_chunk() {
        let (off, mut on) = two_runs();
        on.chunks[1].turns = TurnRange::new(1, 3).unwrap();
        let err = pair_chunks(&off, &on).err().unwrap().to_string();
        assert!(err.contains("chunk 1 covers turns 1..2"), "{err}");
        assert!(err.contains("1..3"), "{err}");
    }

    #[test]
    fn stats_count_wer_later_takes_and_unverified_chunks() {
        let blob = hash("blob");
        let mut unverified = chunk("c", (2, 3), &blob, 2, 100);
        unverified.verified = false;
        let chunks = [
            chunk("a", (0, 1), &blob, 0, 10),
            chunk("b", (1, 2), &blob, 1, 20),
            unverified,
        ];
        let stats = Stats::of(chunks.iter());
        assert_eq!(
            stats,
            Stats {
                chunks: 3,
                wer_sum: 130,
                retried: 2,
                unverified: 1
            }
        );
        assert_eq!(Stats::default().mean_wer_pm(), None);
        assert_eq!(stats.mean_wer_pm().map(|m| m.round()), Some(43.0));
    }

    fn stats(chunks: usize, wer_sum: u64, retried: usize) -> Stats {
        Stats {
            chunks,
            wer_sum,
            retried,
            unverified: 0,
        }
    }

    #[test]
    fn the_verdict_follows_the_kill_criterion() {
        let off = stats(10, 31, 3);
        assert_eq!(
            verdict(&off, &stats(10, 100, 3), 2),
            Verdict::Pass,
            "10 ‰ is allowed"
        );
        assert_eq!(
            verdict(&off, &stats(10, 100, 2), 2),
            Verdict::Pass,
            "fewer retries is fine"
        );

        let Verdict::Fail(causes) = verdict(&off, &stats(10, 101, 3), 2) else {
            panic!("a mean WER above 10 ‰ must fail");
        };
        assert_eq!(causes.len(), 1);
        assert!(causes[0].contains("10.1 ‰"), "{causes:?}");

        let Verdict::Fail(causes) = verdict(&off, &stats(10, 50, 4), 2) else {
            panic!("more retries must fail");
        };
        assert!(causes[0].contains("4 chunks, against 3"), "{causes:?}");

        let Verdict::Fail(causes) = verdict(&off, &stats(10, 500, 9), 2) else {
            panic!("both causes must fail");
        };
        assert_eq!(causes.len(), 2);

        assert_eq!(verdict(&off, &stats(10, 500, 9), 0), Verdict::Inconclusive);
    }

    #[test]
    fn exit_codes_are_pass_fail_inconclusive() {
        assert_eq!(Verdict::Pass.exit_code(), 0);
        assert_eq!(Verdict::Fail(vec![]).exit_code(), 1);
        assert_eq!(Verdict::Inconclusive.exit_code(), 2);
    }

    #[test]
    fn the_report_gives_both_sets_the_changed_table_and_the_caveats() {
        let (off, on) = two_runs();
        let pairs = pair_chunks(&off, &on).unwrap();
        let (text, verdict) = report(&pairs);
        assert_eq!(verdict, Verdict::Pass);
        assert!(
            text.contains("2 chunks paired by turns; 1 changed"),
            "{text}"
        );
        for row in [
            "all        off",
            "all        on",
            "changed    off",
            "changed    on",
        ] {
            assert!(text.contains(row), "{row}\n{text}");
        }
        assert!(text.contains("turns   1..2   off: 1   72 yes"), "{text}");
        assert!(text.contains("verdict: PASS"), "{text}");
        for caveat in [
            "one deterministic sample per run",
            "`heard` variants",
            "banter picks",
        ] {
            assert!(text.contains(caveat), "{caveat}\n{text}");
        }
    }

    #[test]
    fn a_failing_report_lists_its_causes() {
        let (mut off, mut on) = two_runs();
        off.chunks[1].take = 0;
        on.chunks[1].take = 2;
        on.chunks[1].wer_pm = PerMille::new(300).unwrap();
        let pairs = pair_chunks(&off, &on).unwrap();
        let (text, verdict) = report(&pairs);
        assert_eq!(verdict.exit_code(), 1);
        assert!(
            text.contains("verdict: FAIL\n  - the run with the list has a mean WER"),
            "{text}"
        );
        assert!(
            text.contains("\n  - the run with the list kept a later take in 1 chunks, against 0"),
            "{text}"
        );
    }

    #[test]
    fn a_run_the_list_did_not_change_is_inconclusive() {
        let (off, _) = two_runs();
        let pairs = pair_chunks(&off, &off).unwrap();
        let (text, verdict) = report(&pairs);
        assert_eq!(verdict, Verdict::Inconclusive);
        assert!(text.contains("INCONCLUSIVE"), "{text}");
        assert!(!text.contains("changed chunks (take"), "{text}");
    }

    fn write_artifact(dir: &Path, kind: ArtifactKind, body: serde_json::Value) {
        let envelope = Envelope::new(kind, body);
        fs::write(
            dir.join(format!("{}.json", kind.as_str())),
            serde_json::to_vec(&envelope).unwrap(),
        )
        .unwrap();
    }

    fn write_audio(dir: &Path, manifest: &AudioManifest) {
        write_artifact(
            dir,
            ArtifactKind::Audio,
            serde_json::to_value(manifest).unwrap(),
        );
    }

    fn write_script(dir: &Path, texts: &[&str]) {
        write_artifact(
            dir,
            ArtifactKind::Script,
            serde_json::to_value(script(texts)).unwrap(),
        );
    }

    #[test]
    fn a_missing_or_wrong_artifact_is_refused_naming_its_path() {
        let dir = tempfile::tempdir().unwrap();
        let missing = format!("{:#}", load_audio(dir.path()).err().unwrap());
        assert!(missing.contains("audio.json"), "{missing}");

        // A script inside an audio envelope: the kind is right, the body is not.
        write_artifact(
            dir.path(),
            ArtifactKind::Audio,
            serde_json::to_value(script(&["hi"])).unwrap(),
        );
        let wrong_body = format!("{:#}", load_audio(dir.path()).err().unwrap());
        assert!(wrong_body.contains("audio.json"), "{wrong_body}");

        // A script wrapped as `audio.json`: right file name, wrong kind.
        let bytes =
            serde_json::to_vec(&Envelope::new(ArtifactKind::Script, script(&["hi"]))).unwrap();
        fs::write(dir.path().join("audio.json"), bytes).unwrap();
        let wrong_kind = format!("{:#}", load_audio(dir.path()).err().unwrap());
        assert!(
            wrong_kind.contains("is a script artifact, not audio"),
            "{wrong_kind}"
        );

        let (off, _) = two_runs();
        write_audio(dir.path(), &off);
        assert_eq!(load_audio(dir.path()).unwrap(), off);
    }

    #[test]
    fn another_schema_version_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (off, _) = two_runs();
        let envelope = Envelope {
            schema_version: SCHEMA_VERSION - 1,
            kind: ArtifactKind::Audio,
            body: &off,
        };
        fs::write(
            dir.path().join("audio.json"),
            serde_json::to_vec(&envelope).unwrap(),
        )
        .unwrap();
        let err = format!("{:#}", load_audio(dir.path()).err().unwrap());
        assert!(err.contains("schema version"), "{err}");
    }

    /// A cache holding the three blobs of `two_runs`, and the runs with their
    /// `blob` hashes replaced by the stored ones.
    fn runs_in_a_cache(cache: &Path) -> (AudioManifest, AudioManifest) {
        let blobs = DiskCache::new(cache).blobs();
        let (mut off, mut on) = two_runs();
        off.chunks[0].blob = blobs.put(b"plain").unwrap();
        on.chunks[0].blob = off.chunks[0].blob.clone();
        off.chunks[1].blob = blobs.put(b"kulik without the list").unwrap();
        on.chunks[1].blob = blobs.put(b"kulik with the list").unwrap();
        (off, on)
    }

    fn pack_with(coin_value: bool, cache: &Path, dir: &Path) -> Result<()> {
        let (off, on) = runs_in_a_cache(cache);
        let pairs = pair_chunks(&off, &on)?;
        let items = plan_pack(&pairs, &mut || coin_value);
        write_pack(
            dir,
            &items,
            &DiskCache::new(cache).blobs(),
            &script(&["Hello.", "Leonid Kulik arrived."]),
        )
    }

    fn listing(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_pack_has_one_blind_pair_per_changed_chunk() {
        let cache = tempfile::tempdir().unwrap();
        let (on_first, off_first) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        pack_with(true, cache.path(), on_first.path()).unwrap();
        pack_with(false, cache.path(), off_first.path()).unwrap();

        // Only the changed chunk (turns 1..2) is packed, and the names say nothing about the runs.
        for dir in [on_first.path(), off_first.path()] {
            assert_eq!(
                listing(dir),
                ["01-X.wav", "01-Y.wav", "key.json", "ratings.md"]
            );
        }
        // The sheet is the same whichever way the coin fell...
        let sheet = |dir: &Path| fs::read_to_string(dir.join("ratings.md")).unwrap();
        assert_eq!(sheet(on_first.path()), sheet(off_first.path()));
        assert!(sheet(on_first.path()).contains("> host: Leonid Kulik arrived."));
        assert!(!sheet(on_first.path()).contains("> host: Hello."));
        // ...and only key.json and the audio carry the assignment.
        let key = |dir: &Path| fs::read_to_string(dir.join("key.json")).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&key(on_first.path())).unwrap()["01"],
            "X"
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&key(off_first.path())).unwrap()["01"],
            "Y"
        );
        assert_eq!(
            fs::read(on_first.path().join("01-X.wav")).unwrap(),
            b"kulik with the list"
        );
        assert_eq!(
            fs::read(off_first.path().join("01-X.wav")).unwrap(),
            b"kulik without the list"
        );
    }

    #[test]
    fn a_pack_refuses_a_directory_with_something_in_it() {
        let cache = tempfile::tempdir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("keep.txt"), "mine").unwrap();
        let err = pack_with(true, cache.path(), dir.path())
            .err()
            .unwrap()
            .to_string();
        assert!(err.contains("is not empty"), "{err}");
        assert_eq!(listing(dir.path()), ["keep.txt"]);
    }

    #[test]
    fn the_random_coin_gives_both_sides() {
        let mut coin = random_coin();
        let heads = (0..200).filter(|_| coin()).count();
        assert!((20..180).contains(&heads), "{heads} of 200");
    }

    #[test]
    fn two_runs_with_different_scripts_are_refused_before_comparing() {
        let (off_dir, on_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (off, on) = two_runs();
        write_audio(off_dir.path(), &off);
        write_audio(on_dir.path(), &on);
        write_script(off_dir.path(), &["Hello.", "Kulik."]);
        write_script(on_dir.path(), &["Hello.", "Kulik!"]);
        let args = Args {
            off: off_dir.path().into(),
            on: on_dir.path().into(),
            cache_dir: None,
            pack: None,
        };
        let err = run(args).err().unwrap().to_string();
        assert!(err.contains("different scripts"), "{err}");

        write_script(on_dir.path(), &["Hello.", "Kulik."]);
        let args = Args {
            off: off_dir.path().into(),
            on: on_dir.path().into(),
            cache_dir: None,
            pack: None,
        };
        assert_eq!(run(args).unwrap(), 0);
    }
}

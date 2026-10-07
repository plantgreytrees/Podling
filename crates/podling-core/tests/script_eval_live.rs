//! Live evaluation of the script stage: writes N scripts from one saved run's
//! ledger with a real model and measures each with `script_metrics`, so two
//! prompts (or two models) can be compared on the same input. Ignored by
//! default because it needs a model server and a saved run:
//!
//!   podling run --episode examples/titanic/episode-ollama.toml --out <run>
//!   PODLING_SCRIPT_EVAL_RUN=<run> cargo test -p podling-core --test script_eval_live \
//!     -- --ignored --nocapture script_eval
//!
//! Optional: `PODLING_SCRIPT_EVAL_N` (runs, default 5 for `script_eval` and 1
//! per topic for `topic_overlap`), `PODLING_SCRIPT_EVAL_TOPIC` (overrides the
//! episode's topic), and `PODLING_SCRIPT_EVAL_EPISODE` (an episode file whose
//! `[llm]` replaces the run's, e.g. a different server or a hosted model).
//!
//! `topic_overlap` needs `PODLING_SCRIPT_EVAL_TOPICS="<topic A>|<topic B>"`
//! and reports how far the two angles' cited claims overlap.
//!
//! Every script is written by `WriteScript::run` directly, never through the
//! cache, so each run is a fresh request.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::Instant;

use podling_core::plugin::{build_llm, Completion, CompletionRequest, LlmProvider};
use podling_core::script_metrics::{cited_claim_overlap, cited_claims, ScriptMetrics};
use podling_core::stages::{ScriptInput, WriteScript};
use podling_core::{Result, Stage};
use podling_types::{
    Chunk, ClaimId, Document, EpisodeSpec, Ledger, LlmConfig, Script, Speaker, Verdicts,
};
use serde_json::Value;

/// The phrase the script stage's rejection uses for a citation the ledger
/// doesn't hold.
const UNKNOWN_CITATION: &str = "which is not in the ledger";
/// Where `complete_validated_with` starts listing earlier rejections.
const REJECTIONS_HEADER: &str = "Your previous replies were rejected";

#[test]
#[ignore = "needs a model server and a saved run; set PODLING_SCRIPT_EVAL_RUN"]
fn script_eval() {
    let run = run_dir();
    let saved = Saved::load(&run);
    let llm = build_llm(&llm_config(&saved.episode)).unwrap();
    let topic = std::env::var("PODLING_SCRIPT_EVAL_TOPIC").unwrap_or(saved.episode.topic.clone());
    let n = runs(5);

    eprintln!(
        "script_eval: run {}, topic {topic:?}, {} min, llm {}, {} claims, {} verdicts, {n} runs",
        run.display(),
        saved.episode.target_minutes,
        llm.fingerprint(),
        saved.ledger.entries().len(),
        saved.verdicts.as_slice().len(),
    );
    println!(
        "| run | ok | attempts | unknown-citation rejections | secs | words | word ratio | turns | quotes | citations | coverage | judged cited |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|---|---|");
    let mut outcomes = Vec::new();
    for i in 1..=n {
        let outcome = write_one(llm.as_ref(), &saved, &topic);
        println!("| {i} | {} |", outcome.row());
        outcomes.push(outcome);
    }
    println!();
    println!("{}", Summary::of(&outcomes));
}

#[test]
#[ignore = "needs a model server and a saved run; set PODLING_SCRIPT_EVAL_RUN and PODLING_SCRIPT_EVAL_TOPICS"]
fn topic_overlap() {
    let run = run_dir();
    let Ok(topics) = std::env::var("PODLING_SCRIPT_EVAL_TOPICS") else {
        panic!("set PODLING_SCRIPT_EVAL_TOPICS to \"<topic A>|<topic B>\"");
    };
    let Some((topic_a, topic_b)) = topics.split_once('|') else {
        panic!("PODLING_SCRIPT_EVAL_TOPICS must hold two topics separated by '|'");
    };
    let saved = Saved::load(&run);
    let llm = build_llm(&llm_config(&saved.episode)).unwrap();
    let n = runs(1);

    let cited_by = |topic: &str| -> BTreeSet<ClaimId> {
        let mut cited = BTreeSet::new();
        for i in 1..=n {
            let outcome = write_one(llm.as_ref(), &saved, topic);
            eprintln!("{topic:?} run {i}: {}", outcome.row());
            if let Ok((script, _)) = &outcome.result {
                cited.extend(cited_claims(script));
            }
        }
        cited
    };
    let a = cited_by(topic_a);
    let b = cited_by(topic_b);
    let overlap = cited_claim_overlap(&a, &b);

    println!("| topic | distinct claims cited |");
    println!("|---|---|");
    println!("| A: {topic_a} | {} |", a.len());
    println!("| B: {topic_b} | {} |", b.len());
    println!();
    println!(
        "shared {}, only A {}, only B {}, Jaccard {:.2} ({} of {} ledger claims, {n} script(s) per topic)",
        overlap.shared,
        overlap.only_a,
        overlap.only_b,
        overlap.jaccard,
        a.union(&b).count(),
        saved.ledger.entries().len(),
    );
}

/// The saved run named by `PODLING_SCRIPT_EVAL_RUN`.
fn run_dir() -> PathBuf {
    let Ok(dir) = std::env::var("PODLING_SCRIPT_EVAL_RUN") else {
        // Fail, don't skip: an ignored test that returns early reports "ok".
        panic!("set PODLING_SCRIPT_EVAL_RUN to the --out directory of a `podling run`");
    };
    PathBuf::from(dir)
}

/// `PODLING_SCRIPT_EVAL_N`, or `default`.
fn runs(default: usize) -> usize {
    std::env::var("PODLING_SCRIPT_EVAL_N")
        .ok()
        .map(|n| n.parse().expect("PODLING_SCRIPT_EVAL_N must be a number"))
        .unwrap_or(default)
}

/// The run's own `[llm]`, or the one in `PODLING_SCRIPT_EVAL_EPISODE`.
fn llm_config(episode: &EpisodeSpec) -> LlmConfig {
    match std::env::var("PODLING_SCRIPT_EVAL_EPISODE") {
        Ok(path) => {
            let text = std::fs::read_to_string(&path).unwrap();
            let other: EpisodeSpec = toml::from_str(&text).unwrap();
            other.llm
        }
        Err(_) => episode.llm.clone(),
    }
}

/// What the pipeline wrote to `--out`: the script stage's whole input.
struct Saved {
    episode: EpisodeSpec,
    ledger: Ledger,
    verdicts: Verdicts,
    chunks: Vec<Chunk>,
    documents: Vec<Document>,
}

impl Saved {
    fn load(dir: &Path) -> Self {
        Self {
            episode: body(dir, "episode"),
            ledger: body(dir, "ledger"),
            verdicts: body(dir, "verdicts"),
            chunks: body(dir, "chunks"),
            documents: body(dir, "documents"),
        }
    }

    fn input(&self, topic: &str) -> ScriptInput {
        ScriptInput {
            topic: topic.to_owned(),
            target_minutes: self.episode.target_minutes,
            ledger: self.ledger.clone(),
            verdicts: self.verdicts.clone(),
            chunks: self.chunks.clone(),
            documents: self.documents.clone(),
            cast: self
                .episode
                .cast
                .iter()
                .map(|member| Speaker {
                    id: member.id.clone(),
                    name: member.name.clone(),
                    role: member.role.clone(),
                })
                .collect(),
            audio: self.episode.tts.is_some(),
        }
    }
}

/// The `body` of `<dir>/<kind>.json`.
fn body<T: serde::de::DeserializeOwned>(dir: &Path, kind: &str) -> T {
    let path = dir.join(format!("{kind}.json"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
    let json: Value = serde_json::from_str(&text).unwrap();
    serde_json::from_value(json["body"].clone())
        .unwrap_or_else(|err| panic!("{} has an unexpected body: {err}", path.display()))
}

/// Counts the requests the stage makes, and the rejections among them that
/// named a citation the ledger doesn't hold.
struct Counting<'a> {
    inner: &'a dyn LlmProvider,
    attempts: Cell<usize>,
    /// The most unknown-citation rejections any retry listed (each retry
    /// lists every rejection so far).
    unknown_listed: Cell<usize>,
}

impl LlmProvider for Counting<'_> {
    fn id(&self) -> &str {
        self.inner.id()
    }

    fn fingerprint(&self) -> Value {
        self.inner.fingerprint()
    }

    fn complete(&self, request: &CompletionRequest) -> Result<Completion> {
        self.attempts.set(self.attempts.get() + 1);
        if let Some((_, listed)) = request.instructions.split_once(REJECTIONS_HEADER) {
            let unknown = listed.matches(UNKNOWN_CITATION).count();
            self.unknown_listed
                .set(self.unknown_listed.get().max(unknown));
        }
        self.inner.complete(request)
    }
}

/// One script attempt and what it delivered.
struct Outcome {
    attempts: usize,
    unknown_rejections: usize,
    secs: f64,
    result: std::result::Result<(Script, ScriptMetrics), String>,
}

fn write_one(llm: &dyn LlmProvider, saved: &Saved, topic: &str) -> Outcome {
    let counting = Counting {
        inner: llm,
        attempts: Cell::new(0),
        unknown_listed: Cell::new(0),
    };
    let input = saved.input(topic);
    let started = Instant::now();
    let written = WriteScript { llm: &counting }.run(&input);
    let secs = started.elapsed().as_secs_f64();
    let mut unknown_rejections = counting.unknown_listed.get();
    let result = match written {
        Ok(script) => {
            let metrics = ScriptMetrics::of(
                &script,
                &saved.ledger,
                &saved.verdicts,
                saved.episode.target_minutes,
            );
            Ok((script, metrics))
        }
        Err(err) => {
            // The last rejection is in the error, not in any request.
            let message = err.to_string();
            unknown_rejections += message.matches(UNKNOWN_CITATION).count();
            Err(message)
        }
    };
    Outcome {
        attempts: counting.attempts.get(),
        unknown_rejections,
        secs,
        result,
    }
}

impl Outcome {
    fn passed(&self) -> bool {
        self.result.is_ok()
    }

    /// The table row after the run number.
    fn row(&self) -> String {
        let head = format!(
            "{} | {} | {} | {:.0}",
            if self.passed() { "yes" } else { "no" },
            self.attempts,
            self.unknown_rejections,
            self.secs
        );
        match &self.result {
            Ok((_, m)) => format!(
                "{head} | {} | {:.2} | {} | {} | {} | {}/{} ({:.2}) | {}/{}",
                m.words,
                m.word_ratio,
                m.turns,
                m.quotes,
                m.citations,
                m.distinct_cited,
                m.usable_claims,
                m.coverage,
                m.judged_contested_cited,
                m.judged_contested,
            ),
            Err(message) => {
                let short: String = message.chars().take(140).collect();
                format!(
                    "{head} | — | — | — | — | — | — | — ({})",
                    short.replace('|', "/")
                )
            }
        }
    }
}

/// The figures ARCH-STORY-08 compares between prompts.
struct Summary {
    runs: usize,
    passed: usize,
    first_try: usize,
    mean_attempts: f64,
    mean_word_ratio: f64,
    mean_quotes: f64,
    mean_coverage: f64,
    unknown_rejections: usize,
    judged_cited: usize,
    judged: usize,
}

impl Summary {
    fn of(outcomes: &[Outcome]) -> Self {
        let passing: Vec<&ScriptMetrics> = outcomes
            .iter()
            .filter_map(|o| o.result.as_ref().ok().map(|(_, m)| m))
            .collect();
        let mean = |f: &dyn Fn(&ScriptMetrics) -> f64| -> f64 {
            if passing.is_empty() {
                0.0
            } else {
                passing.iter().map(|m| f(m)).sum::<f64>() / passing.len() as f64
            }
        };
        Self {
            runs: outcomes.len(),
            passed: passing.len(),
            first_try: outcomes
                .iter()
                .filter(|o| o.passed() && o.attempts == 1)
                .count(),
            mean_attempts: outcomes.iter().map(|o| o.attempts as f64).sum::<f64>()
                / outcomes.len().max(1) as f64,
            mean_word_ratio: mean(&|m| m.word_ratio),
            mean_quotes: mean(&|m| m.quotes as f64),
            mean_coverage: mean(&|m| m.coverage),
            unknown_rejections: outcomes.iter().map(|o| o.unknown_rejections).sum(),
            judged_cited: passing.iter().map(|m| m.judged_contested_cited).sum(),
            judged: passing.iter().map(|m| m.judged_contested).sum(),
        }
    }
}

impl std::fmt::Display for Summary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "summary: eventual pass {}/{}, first-try pass {}/{}, mean attempts {:.2}, \
             mean word ratio {:.2} (passing runs), mean quotes {:.1}, mean coverage {:.2}, \
             unknown-citation rejections {}, judged Contested cited {}/{}",
            self.passed,
            self.runs,
            self.first_try,
            self.runs,
            self.mean_attempts,
            self.mean_word_ratio,
            self.mean_quotes,
            self.mean_coverage,
            self.unknown_rejections,
            self.judged_cited,
            self.judged,
        )
    }
}

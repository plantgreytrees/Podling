//! A DeBERTa-v3 NLI cross-encoder, run natively on the CPU with candle.
//!
//! A *cross-encoder* reads the premise and the hypothesis together, as one
//! token sequence (`[CLS] premise [SEP] hypothesis [SEP]`), and classifies
//! the pair: does the premise entail the hypothesis, contradict it, or
//! neither? Unlike an embedding model it can't score a text on its own, but it
//! judges a pair far more precisely, which is why the ledger uses it to decide
//! evidence and uses embeddings only to pick which pairs to score.
//!
//! The model is a local Hugging Face snapshot, e.g.
//! `hf download cross-encoder/nli-deberta-v3-base --local-dir <dir>`. Only
//! `config.json`, `tokenizer.json` and `model.safetensors` are read.
//! `pytorch_model.bin` is never touched: it is a Python pickle, and unpickling
//! can run code.

use std::cell::OnceCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::debertav2::{Config, DebertaV2SeqClassificationModel};
use serde_json::{Value, json};
use tokenizers::{Tokenizer, TruncationDirection, TruncationParams, TruncationStrategy};

use super::nli::{NliPair, NliProvider, NliScores};
use crate::error::{CoreError, ProviderFailure, Result};

const PLUGIN: &str = "cross_encoder";

/// DeBERTa-v3's position limit. A longer pair is truncated, longest side first.
pub const MAX_TOKENS: usize = 512;

/// The files a model directory must contain.
pub const MODEL_FILES: [&str; 3] = ["config.json", "tokenizer.json", "model.safetensors"];

/// Class probabilities for one pair, in a fixed order whatever order the
/// model's own head uses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PairScores {
    pub entailment: f32,
    pub neutral: f32,
    pub contradiction: f32,
}

/// Where each label sits in the model's output: `logits[entailment]` and so on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LabelIndex {
    entailment: usize,
    neutral: usize,
    contradiction: usize,
}

pub struct CrossEncoder {
    model: DebertaV2SeqClassificationModel,
    tokenizer: Tokenizer,
    labels: LabelIndex,
    pad_id: u32,
}

impl CrossEncoder {
    /// Loads the model from `model_dir`. Every problem with the directory
    /// (missing files, an unreadable config, weights that don't fit the
    /// architecture) is a [`CoreError::Config`] that names the directory.
    pub fn load(model_dir: &Path) -> Result<Self> {
        check_model_dir(model_dir)?;
        let bad = |what: &str, err: &dyn std::fmt::Display| {
            config_error(format!("{}: {what}: {err}", model_dir.display()))
        };

        let config_text = fs::read_to_string(model_dir.join("config.json"))
            .map_err(|err| CoreError::io(model_dir.join("config.json"), err))?;
        let config: Config =
            serde_json::from_str(&config_text).map_err(|err| bad("config.json", &err))?;
        let labels = label_index(&config)?;

        let mut tokenizer = Tokenizer::from_file(model_dir.join("tokenizer.json"))
            .map_err(|err| bad("tokenizer.json", &err))?;
        tokenizer
            .with_truncation(Some(TruncationParams {
                max_length: MAX_TOKENS,
                strategy: TruncationStrategy::LongestFirst,
                stride: 0,
                direction: TruncationDirection::Right,
            }))
            .map_err(|err| bad("tokenizer.json", &err))?;
        // Padding is done by hand in `scores`, together with the attention mask.
        tokenizer.with_padding(None);

        // `from_mmaped_safetensors` would map the file instead of reading it,
        // but it is an `unsafe fn` (the file could change underneath the
        // mapping) and this workspace forbids `unsafe`. Reading ~700 MB into
        // memory once per run is cheap on the target machine.
        let weights_path = model_dir.join("model.safetensors");
        let weights = fs::read(&weights_path).map_err(|err| CoreError::io(&weights_path, err))?;
        // A `VarBuilder` hands each layer its tensors by name. `pp` ("push
        // prefix") scopes it: the encoder's weights are stored under
        // `deberta.*`, the pooler and the classifier head at the root.
        let vb = VarBuilder::from_buffered_safetensors(weights, DType::F32, &Device::Cpu)
            .map_err(|err| bad("model.safetensors", &err))?;
        let model = DebertaV2SeqClassificationModel::load(vb.pp("deberta"), &config, None)
            .map_err(|err| bad("model.safetensors", &err))?;

        Ok(Self {
            model,
            tokenizer,
            labels,
            pad_id: config.pad_token_id.unwrap_or(0) as u32,
        })
    }

    /// Scores each `(premise, hypothesis)` pair, in order, as one batch.
    pub fn scores(&self, pairs: &[(&str, &str)]) -> Result<Vec<PairScores>> {
        if pairs.is_empty() {
            return Ok(Vec::new());
        }
        let encodings = self
            .tokenizer
            .encode_batch(pairs.to_vec(), true)
            .map_err(|err| inference_error(format!("tokenising: {err}")))?;

        // Pad every sequence to the longest one, and mask the padding out so
        // it changes nothing: a padded pair scores as it would alone.
        let width = encodings.iter().map(|e| e.len()).max().unwrap_or(0);
        let mut ids = Vec::with_capacity(pairs.len() * width);
        let mut mask = Vec::with_capacity(pairs.len() * width);
        for encoding in &encodings {
            let real = encoding.get_ids();
            ids.extend_from_slice(real);
            ids.extend(std::iter::repeat_n(self.pad_id, width - real.len()));
            mask.extend(std::iter::repeat_n(1i64, real.len()));
            mask.extend(std::iter::repeat_n(0i64, width - real.len()));
        }

        let candle = |err: candle_core::Error| inference_error(err.to_string());
        let shape = (pairs.len(), width);
        let ids = Tensor::from_vec(ids, shape, &Device::Cpu).map_err(candle)?;
        let mask = Tensor::from_vec(mask, shape, &Device::Cpu).map_err(candle)?;
        let logits = self.model.forward(&ids, None, Some(mask)).map_err(candle)?;
        let probs = candle_nn::ops::softmax(&logits, candle_core::D::Minus1)
            .and_then(|p| p.to_vec2::<f32>())
            .map_err(candle)?;

        probs
            .into_iter()
            .map(|row| {
                let at = |i: usize| {
                    row.get(i)
                        .copied()
                        .ok_or_else(|| inference_error("the model returned too few classes"))
                };
                Ok(PairScores {
                    entailment: at(self.labels.entailment)?,
                    neutral: at(self.labels.neutral)?,
                    contradiction: at(self.labels.contradiction)?,
                })
            })
            .collect()
    }
}

/// Fails, naming the download command, unless `model_dir` has every file in
/// [`MODEL_FILES`]. Cheap: it only looks at the directory.
pub fn check_model_dir(model_dir: &Path) -> Result<()> {
    let missing: Vec<&str> = MODEL_FILES
        .into_iter()
        .filter(|name| !model_dir.join(name).is_file())
        .collect();
    if missing.is_empty() {
        return Ok(());
    }
    Err(config_error(format!(
        "NLI model directory {} is missing {}; download the model with \
         `hf download cross-encoder/nli-deberta-v3-base --local-dir {}`",
        model_dir.display(),
        missing.join(", "),
        model_dir.display()
    )))
}

/// The paths whose bytes define the model, in a fixed order.
pub fn model_files(model_dir: &Path) -> [PathBuf; 3] {
    MODEL_FILES.map(|name| model_dir.join(name))
}

/// Finds entailment, neutral and contradiction among the model's labels.
/// Models disagree on the order (`cross-encoder/nli-deberta-v3-base` puts
/// contradiction first), so it is read from `id2label`, never assumed.
fn label_index(config: &Config) -> Result<LabelIndex> {
    let find = |wanted: &str| -> Result<usize> {
        config
            .id2label
            .iter()
            .flatten()
            .find(|(_, label)| label.eq_ignore_ascii_case(wanted))
            .map(|(id, _)| *id as usize)
            .ok_or_else(|| {
                config_error(format!(
                    "the NLI model's config.json has no `{wanted}` label in id2label; \
                     it needs entailment, neutral and contradiction"
                ))
            })
    };
    if config.id2label.as_ref().map_or(0, |m| m.len()) != 3 {
        return Err(config_error(
            "the NLI model must have exactly three labels: entailment, neutral, contradiction",
        ));
    }
    Ok(LabelIndex {
        entailment: find("entailment")?,
        neutral: find("neutral")?,
        contradiction: find("contradiction")?,
    })
}

fn config_error(message: impl Into<String>) -> CoreError {
    CoreError::Config {
        message: message.into(),
    }
}

fn inference_error(message: impl Into<String>) -> CoreError {
    CoreError::Provider {
        plugin: PLUGIN.into(),
        kind: ProviderFailure::Other,
        message: message.into(),
    }
}

/// Pairs per forward pass. A batch is padded to its longest pair, so small
/// batches waste little work on padding and keep memory flat.
pub const NLI_BATCH: usize = 16;

/// Bump when scoring changes in a way the weights don't show (batching that
/// alters results, a different truncation rule).
const NLI_VERSION: u32 = 1;

/// The [`NliProvider`] over a local [`CrossEncoder`].
///
/// Construction is cheap: it checks the files and fingerprints their bytes,
/// but loads nothing. The model loads on the first [`score`] call and stays
/// in the `OnceCell` until the provider is dropped, so a run whose NLI stages
/// are all cache hits never loads it at all.
///
/// [`score`]: NliProvider::score
pub struct CrossEncoderNli {
    model_dir: PathBuf,
    weights: String,
    /// `OnceCell` is a slot that can be filled once through a shared `&self`
    /// reference, after which it hands out `&CrossEncoder` borrows. That is
    /// what lets `score(&self)` load the model lazily without `&mut self`
    /// (which the trait doesn't offer) and without a lock (the pipeline is
    /// single-threaded; `OnceCell` isn't `Sync`, and needn't be).
    model: OnceCell<CrossEncoder>,
}

impl CrossEncoderNli {
    /// Checks `model_dir` and fingerprints its files. A missing file is a
    /// `Config` error naming the download command.
    pub fn new(model_dir: &Path) -> Result<Self> {
        check_model_dir(model_dir)?;
        // Hash each file, then the three hashes together, so moving bytes
        // from one file to another can't produce the same fingerprint.
        let mut combined = blake3::Hasher::new();
        for path in model_files(model_dir) {
            let mut file = fs::File::open(&path).map_err(|err| CoreError::io(&path, err))?;
            let mut hasher = blake3::Hasher::new();
            // Streams the file: the weights are hundreds of MB.
            hasher
                .update_reader(&mut file)
                .map_err(|err| CoreError::io(&path, err))?;
            combined.update(hasher.finalize().as_bytes());
        }
        Ok(Self {
            model_dir: model_dir.to_owned(),
            weights: combined.finalize().to_hex().to_string(),
            model: OnceCell::new(),
        })
    }

    /// The loaded model, loading it on first use.
    fn model(&self) -> Result<&CrossEncoder> {
        // `OnceCell::get_or_try_init` (fallible init) isn't stable yet, so:
        // return the model if loaded, else load it and store it.
        if let Some(model) = self.model.get() {
            return Ok(model);
        }
        let started = Instant::now();
        let loaded = CrossEncoder::load(&self.model_dir)?;
        tracing::info!(
            model_dir = %self.model_dir.display(),
            elapsed_ms = started.elapsed().as_millis() as u64,
            "NLI model loaded"
        );
        Ok(self.model.get_or_init(|| loaded))
    }
}

impl NliProvider for CrossEncoderNli {
    fn id(&self) -> &str {
        PLUGIN
    }

    /// The weights' hash, not the directory: the same model anywhere gives
    /// the same cache keys, and a re-downloaded, changed model does not.
    fn fingerprint(&self) -> Value {
        json!({
            "provider": PLUGIN,
            "weights_blake3": self.weights,
            "max_tokens": MAX_TOKENS,
            "version": NLI_VERSION,
        })
    }

    fn score(&self, pairs: &[NliPair<'_>]) -> Result<Vec<NliScores>> {
        let model = self.model()?;
        let mut scores = Vec::with_capacity(pairs.len());
        for batch in pairs.chunks(NLI_BATCH) {
            let span = tracing::debug_span!("nli_batch", pairs = batch.len());
            let _entered = span.enter();
            let started = Instant::now();
            let texts: Vec<(&str, &str)> =
                batch.iter().map(|p| (p.premise, p.hypothesis)).collect();
            scores.extend(model.scores(&texts)?.into_iter().map(|s| NliScores {
                entailment: s.entailment,
                neutral: s.neutral,
                contradiction: s.contradiction,
            }));
            tracing::debug!(
                elapsed_ms = started.elapsed().as_millis() as u64,
                "nli batch done"
            );
        }
        Ok(scores)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn construction_fingerprints_the_files_but_loads_nothing() {
        let dir = tempfile::tempdir().unwrap();
        for name in MODEL_FILES {
            fs::write(dir.path().join(name), format!("not a real {name}")).unwrap();
        }
        let nli = CrossEncoderNli::new(dir.path()).expect("builds without loading");
        let before = nli.fingerprint();
        assert_eq!(before["weights_blake3"].as_str().unwrap().len(), 64);

        // Loading happens on the first score, and fails there.
        let pair = NliPair {
            premise: "a",
            hypothesis: "b",
        };
        assert!(nli.score(&[pair]).is_err());

        fs::write(dir.path().join("model.safetensors"), "other bytes").unwrap();
        let after = CrossEncoderNli::new(dir.path()).unwrap().fingerprint();
        assert_ne!(before, after, "changed weights must change the fingerprint");
    }

    /// A minimal DeBERTa-v3 config with the given `id2label`.
    fn config(id2label: serde_json::Value) -> Config {
        serde_json::from_value(serde_json::json!({
            "vocab_size": 10, "hidden_size": 8, "num_hidden_layers": 1,
            "num_attention_heads": 1, "intermediate_size": 8, "hidden_act": "gelu",
            "hidden_dropout_prob": 0.0, "attention_probs_dropout_prob": 0.0,
            "max_position_embeddings": 512, "type_vocab_size": 0,
            "initializer_range": 0.02, "layer_norm_eps": 1e-7,
            "relative_attention": true, "max_relative_positions": -1,
            "position_biased_input": false, "pos_att_type": ["p2c", "c2p"],
            "id2label": id2label,
        }))
        .unwrap()
    }

    #[test]
    fn labels_are_read_from_the_config_in_any_order() {
        let cross_encoder = config(serde_json::json!(
            {"0": "contradiction", "1": "entailment", "2": "neutral"}
        ));
        assert_eq!(
            label_index(&cross_encoder).unwrap(),
            LabelIndex {
                entailment: 1,
                neutral: 2,
                contradiction: 0
            }
        );
        let upper = config(serde_json::json!(
            {"0": "ENTAILMENT", "1": "NEUTRAL", "2": "CONTRADICTION"}
        ));
        assert_eq!(label_index(&upper).unwrap().contradiction, 2);
    }

    #[test]
    fn a_model_without_the_three_nli_labels_is_a_config_error() {
        for id2label in [
            serde_json::json!({"0": "negative", "1": "positive"}),
            serde_json::json!({"0": "entailment", "1": "neutral", "2": "other"}),
            serde_json::Value::Null,
        ] {
            let err = label_index(&config(id2label)).unwrap_err();
            assert!(matches!(err, CoreError::Config { .. }), "{err}");
        }
    }

    #[test]
    fn a_missing_file_names_the_download_command() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("config.json"), "{}").unwrap();
        let err = check_model_dir(dir.path()).unwrap_err();
        let CoreError::Config { message } = err else {
            panic!("expected a Config error");
        };
        assert!(
            message.contains("tokenizer.json, model.safetensors"),
            "{message}"
        );
        assert!(message.contains("hf download"), "{message}");
        assert!(!message.contains("config.json,"), "{message}");
    }
}

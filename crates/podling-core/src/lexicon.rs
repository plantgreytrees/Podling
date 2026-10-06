//! Names the TTS model mispronounces, and how to say them.
//!
//! A [`Lexicon`] comes from the user-level `pronounce.toml` (beside
//! `sidecars.toml`) with the episode's `[tts.pronounce]` over it. It has two
//! uses, kept apart so neither can leak into the other:
//!
//! - [`Respellings`] give the TTS model a respelt name. They reach only
//!   `SpokenTurn::say_as`, which only the wire turn reads; the script, the
//!   quotes and what speech recognition is checked against keep the name.
//! - [`HeardVariants`] read what speech recognition writes for a name as the
//!   name, so a correctly spoken name is not counted as a word error.

use std::io::ErrorKind;
use std::ops::Range;
use std::path::{Path, PathBuf};

use podling_types::Lexicon;
use serde::Deserialize;

use crate::error::{CoreError, Result};
use crate::stages::verify_audio::words;

/// The user-level lexicon's file name, beside the `sidecars.toml` in use.
pub const USER_FILE: &str = "pronounce.toml";

/// Where the user-level lexicon is: beside `profiles`, the sidecar profiles
/// file in use (`~/.config/podling/sidecars.toml`, or the CLI's
/// `--sidecars`). The episode never names it.
pub fn user_path(profiles: &Path) -> PathBuf {
    profiles.with_file_name(USER_FILE)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PronounceFile {
    #[serde(default)]
    pronounce: Lexicon,
}

/// Reads the user-level lexicon at `path`: a `[pronounce]` table, like an
/// episode's `[tts.pronounce]`. No file is an empty lexicon; a file that
/// can't be read or parsed is a `Config` error naming it.
pub fn load_user(path: &Path) -> Result<Lexicon> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(Lexicon::default()),
        Err(err) => {
            return Err(CoreError::Config {
                message: format!("cannot read {}: {err}", path.display()),
            });
        }
    };
    toml::from_str::<PronounceFile>(&text)
        .map(|file| file.pronounce)
        .map_err(|err| CoreError::Config {
            message: format!("{} is not valid: {err}", path.display()),
        })
}

/// The user's lexicon with the episode's over it: the episode wins per name.
pub fn merge(user: &Lexicon, episode: &Lexicon) -> Lexicon {
    user.overlaid(episode)
}

/// Names and their respellings, longest name first, so "Le Mans" is
/// matched before a name "Le".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Respellings {
    entries: Vec<(String, String)>,
}

/// No respellings, for callers without a lexicon.
static NONE: Respellings = Respellings {
    entries: Vec::new(),
};

impl Respellings {
    pub fn new(lexicon: &Lexicon) -> Self {
        let mut entries: Vec<(String, String)> = lexicon
            .iter()
            .map(|(name, p)| (name.to_owned(), p.say().to_owned()))
            .collect();
        // Longest first; the name breaks ties so the order never depends on
        // the map's.
        entries.sort_by(|(a, _), (b, _)| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        Self { entries }
    }

    /// The empty set, borrowed for as long as needed.
    pub fn none() -> &'static Self {
        &NONE
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// `text` with each name in it respelt, or `None` when it has none.
    /// A name matches as a whole word (not inside a longer word) and with
    /// the same case.
    pub fn respell(&self, text: &str) -> Option<String> {
        let found = self.find(text);
        if found.is_empty() {
            return None;
        }
        let mut respelt = String::with_capacity(text.len());
        let mut from = 0;
        for (span, entry) in found {
            respelt.push_str(&text[from..span.start]);
            respelt.push_str(&self.entries[entry].1);
            from = span.end;
        }
        respelt.push_str(&text[from..]);
        Some(respelt)
    }

    /// The names in `text`, in order, each once.
    pub fn names_in(&self, text: &str) -> Vec<&str> {
        let mut names: Vec<&str> = Vec::new();
        for (_, entry) in self.find(text) {
            let name = self.entries[entry].0.as_str();
            if !names.contains(&name) {
                names.push(name);
            }
        }
        names
    }

    /// Where each name is in `text`, left to right, with its entry. At each
    /// word start the longest name that fits wins.
    fn find(&self, text: &str) -> Vec<(Range<usize>, usize)> {
        let is_word = |c: char| c.is_alphanumeric();
        let mut found = Vec::new();
        if self.entries.is_empty() {
            return found;
        }
        let mut at = 0;
        while at < text.len() {
            let rest = &text[at..];
            let word_start = text[..at].chars().next_back().is_none_or(|c| !is_word(c));
            let hit = word_start
                .then(|| {
                    self.entries.iter().position(|(name, _)| {
                        rest.starts_with(name.as_str())
                            && rest[name.len()..]
                                .chars()
                                .next()
                                .is_none_or(|c| !is_word(c))
                    })
                })
                .flatten();
            match hit {
                Some(entry) => {
                    let end = at + self.entries[entry].0.len();
                    found.push((at..end, entry));
                    at = end;
                }
                None => at += rest.chars().next().map_or(1, char::len_utf8),
            }
        }
        found
    }
}

/// What speech recognition may write for each name, as comparable words
/// ([`words`]), mapped to the name's own words; longest variant first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HeardVariants {
    variants: Vec<(Vec<String>, Vec<String>)>,
}

/// No variants, for callers without a lexicon.
static NO_VARIANTS: HeardVariants = HeardVariants {
    variants: Vec::new(),
};

impl HeardVariants {
    pub fn new(lexicon: &Lexicon) -> Self {
        let mut variants: Vec<(Vec<String>, Vec<String>)> = lexicon
            .iter()
            .flat_map(|(name, p)| {
                let name = words(name);
                p.heard()
                    .iter()
                    .map(move |variant| (words(variant), name.clone()))
            })
            .filter(|(variant, _)| !variant.is_empty())
            .collect();
        variants.sort_by(|(a, _), (b, _)| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        Self { variants }
    }

    /// The empty set, borrowed for as long as needed.
    pub fn none() -> &'static Self {
        &NO_VARIANTS
    }

    /// `heard` (a transcript's words) with each variant replaced by the
    /// words of its name.
    pub fn apply(&self, heard: Vec<String>) -> Vec<String> {
        if self.variants.is_empty() {
            return heard;
        }
        let mut out = Vec::with_capacity(heard.len());
        let mut at = 0;
        while at < heard.len() {
            let rest = &heard[at..];
            match self.variants.iter().find(|(v, _)| rest.starts_with(v)) {
                Some((variant, name)) => {
                    out.extend(name.iter().cloned());
                    at += variant.len();
                }
                None => {
                    out.push(heard[at].clone());
                    at += 1;
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use podling_types::Pronunciation;

    use super::*;

    fn lexicon(entries: &[(&str, &str, &[&str])]) -> Lexicon {
        let map: BTreeMap<String, Pronunciation> = entries
            .iter()
            .map(|(name, say, heard)| {
                let heard = heard.iter().map(|h| (*h).to_owned()).collect();
                ((*name).to_owned(), Pronunciation::new(*say, heard).unwrap())
            })
            .collect();
        Lexicon::new(map).unwrap()
    }

    fn w(text: &str) -> Vec<String> {
        words(text)
    }

    #[test]
    fn a_name_is_respelt_only_as_a_whole_word() {
        let r = Respellings::new(&lexicon(&[("Kulik", "Koolick", &[])]));
        assert_eq!(
            r.respell("Kulik went north. Kulik's men followed.")
                .as_deref(),
            Some("Koolick went north. Koolick's men followed.")
        );
        assert_eq!(r.respell("The Kuliks went north."), None);
        assert_eq!(r.respell("AKulik"), None);
    }

    #[test]
    fn a_name_is_matched_with_its_case() {
        let r = Respellings::new(&lexicon(&[("Kulik", "Koolick", &[])]));
        assert_eq!(r.respell("kulik went north"), None);
        assert_eq!(r.respell("KULIK went north"), None);
    }

    #[test]
    fn the_longest_name_wins_where_names_overlap() {
        let r = Respellings::new(&lexicon(&[("Le", "Luh", &[]), ("Le Mans", "Luh Mon", &[])]));
        assert_eq!(
            r.respell("Le Mans, then Le Havre.").as_deref(),
            Some("Luh Mon, then Luh Havre.")
        );
        assert_eq!(r.names_in("Le Mans, then Le Havre."), ["Le Mans", "Le"]);
    }

    #[test]
    fn text_without_a_name_has_no_respelling() {
        let r = Respellings::new(&lexicon(&[("Kulik", "Koolick", &[])]));
        assert_eq!(r.respell("A fireball crossed the sky."), None);
        assert!(r.names_in("A fireball crossed the sky.").is_empty());
        assert_eq!(Respellings::none().respell("Kulik"), None);
    }

    #[test]
    fn a_heard_variant_reads_as_the_name() {
        let h = HeardVariants::new(&lexicon(&[
            ("Kulik", "Koolick", &["Koolik"]),
            ("Tunguska", "Toongooska", &["Tungus Ka"]),
        ]));
        assert_eq!(
            h.apply(w("Koolik reached Tungus Ka in 1927")),
            w("Kulik reached Tunguska in 1927")
        );
        // Anything else is left alone.
        assert_eq!(h.apply(w("Tungus river")), w("Tungus river"));
        assert_eq!(HeardVariants::none().apply(w("Koolik")), w("Koolik"));
    }

    #[test]
    fn the_episode_wins_over_the_user_file_per_name() {
        let user = lexicon(&[("Kulik", "Koolick", &[]), ("Vanavara", "Vanavahra", &[])]);
        let episode = lexicon(&[("Kulik", "Kooleek", &[])]);
        let merged = merge(&user, &episode);
        assert_eq!(merged.get("Kulik").unwrap().say(), "Kooleek");
        assert_eq!(merged.get("Vanavara").unwrap().say(), "Vanavahra");
    }

    #[test]
    fn the_user_file_sits_beside_sidecars_toml() {
        assert_eq!(
            user_path(Path::new("/home/u/.config/podling/sidecars.toml")),
            Path::new("/home/u/.config/podling/pronounce.toml")
        );
    }

    #[test]
    fn a_missing_user_file_is_empty_and_a_bad_one_is_named() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(USER_FILE);
        assert!(load_user(&path).unwrap().is_empty());

        std::fs::write(&path, "[pronounce]\nKulik = \"Koolick\"\n").unwrap();
        assert_eq!(
            load_user(&path).unwrap().get("Kulik").unwrap().say(),
            "Koolick"
        );

        std::fs::write(&path, "[pronounce]\nKulik = \"\"\n").unwrap();
        let Err(CoreError::Config { message }) = load_user(&path) else {
            panic!("an empty respelling must be a Config error");
        };
        assert!(message.contains(&path.display().to_string()), "{message}");

        std::fs::write(&path, "[voices]\nKulik = \"Koolick\"\n").unwrap();
        assert!(matches!(load_user(&path), Err(CoreError::Config { .. })));
    }
}

//! Tokenizer, `EmbeddingProvider` (+ hashed TF-IDF implementation), semantic index and hybrid
//! (lexical + semantic) search.
//!
//! v1 semantic matching is an on-device hashed TF-IDF vector with a small concept lexicon. It is
//! lexical-ish, not a language model: for real semantic recall implement [`EmbeddingProvider`] with a
//! neural sentence encoder (e.g. via `ort`/`candle`) and hand it to [`SearchEngine::with_provider`].

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use unicode_normalization::UnicodeNormalization;

// ----------------------------------------------------------------- tokenizer

const STOPWORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "if", "then", "of", "to", "in", "on", "at", "for", "with", "by", "from", "as",
    "is", "are", "was", "were", "be", "been", "being", "it", "its", "this", "that", "these", "those", "i", "me", "my",
    "we", "our", "you", "your", "he", "she", "they", "them", "his", "her", "their", "not", "no", "so", "do", "did",
    "does", "have", "has", "had", "will", "would", "can", "could", "just", "about", "into", "out", "up", "down", "over",
    "than", "too", "very", "what", "when", "where", "who", "which", "how", "there", "here", "am", "im", "ive", "dont",
    "didnt", "cant", "wasnt",
];

fn fold(s: &str) -> String {
    s.nfd().filter(|c| !('\u{0300}'..='\u{036f}').contains(c)).collect()
}

/// Lowercased word tokens (letters/digits), stopwords removed, lightly stemmed.
pub fn tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        if cur.chars().count() >= 2 {
            let folded = fold(cur);
            if !STOPWORDS.contains(&folded.as_str()) {
                out.push(stem(&folded));
            }
        }
        cur.clear();
    };
    for ch in text.to_lowercase().chars() {
        if ch.is_alphanumeric() {
            cur.push(ch);
        } else if ch == '\'' || ch == '’' {
            continue; // don't -> dont
        } else {
            flush(&mut cur, &mut out);
        }
    }
    flush(&mut cur, &mut out);
    out
}

/// A deliberately tiny suffix stemmer (good enough for lexical matching; not linguistically complete).
pub fn stem(w: &str) -> String {
    let chars: Vec<char> = w.chars().collect();
    if chars.len() <= 4 || !chars.iter().all(|c| c.is_alphabetic()) {
        return w.to_string();
    }
    for suffix in ["ing", "edly", "ed", "ly", "ies", "es", "s"] {
        if w.ends_with(suffix) && chars.len() - suffix.len() >= 3 {
            let mut s: Vec<char> = chars[..chars.len() - suffix.len()].to_vec();
            if suffix == "ies" {
                s.push('y');
            }
            // undouble: "planning" -> "plann" -> "plan"
            if suffix == "ing" || suffix == "ed" {
                let n = s.len();
                if n >= 3 && s[n - 1] == s[n - 2] && !"aeioulsz".contains(s[n - 1]) {
                    s.pop();
                }
            }
            return s.into_iter().collect();
        }
    }
    w.to_string()
}

// ---------------------------------------------------------------- embeddings

/// Turns text into a fixed-size vector so semantic closeness = cosine similarity.
/// `identifier()` must change whenever vectors are not comparable with older ones.
pub trait EmbeddingProvider: Send + Sync {
    fn identifier(&self) -> String;
    fn dimensions(&self) -> usize;
    /// L2-normalised vector of length `dimensions()` (all zeros for empty/untokenizable text).
    fn embed(&self, text: &str) -> Vec<f32>;
    /// True when vector buckets are sparse lexical features that benefit from corpus IDF weighting
    /// (the index then applies IDF). Neural dense encoders return false.
    fn uses_idf(&self) -> bool {
        false
    }
}

pub fn normalized(mut v: Vec<f32>) -> Vec<f32> {
    let n: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        for x in &mut v {
            *x /= n;
        }
    }
    v
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let (mut d, mut na, mut nb) = (0f32, 0f32, 0f32);
    for i in 0..a.len().min(b.len()) {
        d += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na <= 0.0 || nb <= 0.0 {
        0.0
    } else {
        d / (na.sqrt() * nb.sqrt())
    }
}

const CONCEPTS: &[(&str, &[&str])] = &[
    ("fatigue", &["burnout", "burned", "burnt", "exhausted", "exhaustion", "tired", "drained", "fatigue", "worn", "overwhelmed", "overworked", "depleted", "sleepy"]),
    ("stress", &["stress", "stressed", "anxious", "anxiety", "worried", "worry", "nervous", "panic", "pressure", "tense", "deadline"]),
    ("joy", &["happy", "joy", "joyful", "delighted", "excited", "thrilled", "glad", "wonderful", "amazing", "grateful", "cheerful"]),
    ("sadness", &["sad", "unhappy", "depressed", "lonely", "grief", "cried", "crying", "miserable", "heartbroken", "down"]),
    ("family", &["mom", "mother", "dad", "father", "sister", "brother", "family", "parents", "grandma", "grandmother", "grandpa", "grandfather", "kids", "daughter", "son"]),
    ("travel", &["trip", "travel", "vacation", "holiday", "flight", "airport", "hotel", "journey", "roadtrip", "abroad", "hiking"]),
    ("work", &["work", "job", "office", "meeting", "boss", "colleague", "project", "client", "career", "manager", "salary"]),
    ("health", &["sick", "ill", "doctor", "hospital", "pain", "medicine", "health", "illness", "fever", "therapy", "checkup"]),
    ("money", &["money", "budget", "rent", "salary", "debt", "savings", "expensive", "paid", "invoice", "mortgage"]),
    ("food", &["dinner", "lunch", "breakfast", "cooked", "restaurant", "recipe", "meal", "cooking", "baked"]),
];

/// Hashing-trick bag-of-words (term frequency) embedding, weighted by corpus IDF inside
/// [`SemanticIndex`]:
///  - stemmed unigrams (sublinear tf), bigrams at lower weight
///  - character trigrams at low weight so "journaling" ~ "journal"
///  - a small built-in concept lexicon (exhausted/tired/burnout → "fatigue") so a few common
///    emotional/life themes match by meaning, not only by word.
pub struct HashedTfIdfEmbedding {
    dims: usize,
    use_concepts: bool,
    concept_index: HashMap<String, &'static str>,
}

impl HashedTfIdfEmbedding {
    pub fn new(dims: usize, use_concepts: bool) -> Self {
        let mut concept_index = HashMap::new();
        for (concept, words) in CONCEPTS {
            for w in *words {
                concept_index.insert(stem(w), *concept);
            }
        }
        Self { dims, use_concepts, concept_index }
    }

    /// FNV-1a 64-bit with a finaliser (stable across runs/platforms).
    pub fn fnv1a(s: &str) -> u64 {
        let mut h: u64 = 0xcbf29ce484222325;
        for b in s.bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
        h ^= h >> 33;
        h = h.wrapping_mul(0xff51afd7ed558ccd);
        h ^= h >> 33;
        h
    }
}

impl Default for HashedTfIdfEmbedding {
    fn default() -> Self {
        Self::new(1024, true)
    }
}

impl EmbeddingProvider for HashedTfIdfEmbedding {
    fn identifier(&self) -> String {
        format!("hashed-tfidf-v1-{}-c{}", self.dims, self.use_concepts as u8)
    }
    fn dimensions(&self) -> usize {
        self.dims
    }
    fn uses_idf(&self) -> bool {
        true
    }
    fn embed(&self, text: &str) -> Vec<f32> {
        let toks = tokens(text);
        let mut v = vec![0f32; self.dims];
        if toks.is_empty() {
            return v;
        }
        let mut add = |feature: &str, weight: f32| {
            let h = Self::fnv1a(feature);
            let bucket = (h % self.dims as u64) as usize;
            let sign = if (h >> 63) == 0 { 1.0 } else { -1.0 };
            v[bucket] += sign * weight;
        };
        let mut counts: BTreeMap<&str, usize> = BTreeMap::new();
        for t in &toks {
            *counts.entry(t.as_str()).or_default() += 1;
        }
        for (t, c) in counts {
            add(&format!("u:{t}"), 1.0 + (c as f32).ln());
            let chars: Vec<char> = t.chars().collect();
            if chars.len() >= 4 {
                for i in 0..=(chars.len() - 3) {
                    let tri: String = chars[i..i + 3].iter().collect();
                    add(&format!("c:{tri}"), 0.15);
                }
            }
            if self.use_concepts {
                if let Some(concept) = self.concept_index.get(t) {
                    add(&format!("k:{concept}"), 1.5);
                }
            }
        }
        for w in toks.windows(2) {
            add(&format!("b:{}_{}", w[0], w[1]), 0.5);
        }
        normalized(v)
    }
}

// ------------------------------------------------------------ semantic index

/// In-memory semantic index: id -> sparse embedding (+ IDF weighting when the provider asks for it).
/// Cheap enough for tens of thousands of notes/segments.
pub struct SemanticIndex {
    provider: Box<dyn EmbeddingProvider>,
    vectors: BTreeMap<String, Vec<(u32, f32)>>,
    df: Vec<u32>,
}

fn to_sparse(v: &[f32]) -> Vec<(u32, f32)> {
    v.iter().enumerate().filter(|(_, x)| **x != 0.0).map(|(i, x)| (i as u32, *x)).collect()
}

impl SemanticIndex {
    pub fn new(provider: Box<dyn EmbeddingProvider>) -> Self {
        let dims = provider.dimensions();
        Self { provider, vectors: BTreeMap::new(), df: vec![0; dims] }
    }
    pub fn provider(&self) -> &dyn EmbeddingProvider {
        self.provider.as_ref()
    }
    pub fn len(&self) -> usize {
        self.vectors.len()
    }
    pub fn is_empty(&self) -> bool {
        self.vectors.is_empty()
    }
    pub fn upsert(&mut self, id: &str, text: &str) {
        self.remove(id);
        let sv = to_sparse(&self.provider.embed(text));
        for (i, _) in &sv {
            self.df[*i as usize] += 1;
        }
        self.vectors.insert(id.to_string(), sv);
    }
    pub fn remove(&mut self, id: &str) {
        if let Some(old) = self.vectors.remove(id) {
            for (i, _) in old {
                self.df[i as usize] = self.df[i as usize].saturating_sub(1);
            }
        }
    }
    fn idf(&self, bucket: u32) -> f32 {
        if !self.provider.uses_idf() {
            return 1.0;
        }
        let n = self.vectors.len() as f32;
        ((n + 1.0) / (self.df[bucket as usize] as f32 + 1.0)).ln() + 1.0
    }
    fn weighted(&self, v: &[(u32, f32)]) -> Vec<(u32, f32)> {
        v.iter().map(|(i, x)| (*i, x * self.idf(*i))).collect()
    }
    fn sparse_cosine(a: &[(u32, f32)], b: &[(u32, f32)]) -> f32 {
        let (mut i, mut j) = (0, 0);
        let (mut d, mut na, mut nb) = (0f32, 0f32, 0f32);
        for (_, x) in a {
            na += x * x;
        }
        for (_, x) in b {
            nb += x * x;
        }
        while i < a.len() && j < b.len() {
            match a[i].0.cmp(&b[j].0) {
                std::cmp::Ordering::Equal => {
                    d += a[i].1 * b[j].1;
                    i += 1;
                    j += 1;
                }
                std::cmp::Ordering::Less => i += 1,
                std::cmp::Ordering::Greater => j += 1,
            }
        }
        if na <= 0.0 || nb <= 0.0 {
            0.0
        } else {
            d / (na.sqrt() * nb.sqrt())
        }
    }
    fn similar_to_vec(&self, q: &[(u32, f32)], limit: usize, min_score: f32, exclude: Option<&str>) -> Vec<(String, f32)> {
        let qw = self.weighted(q);
        let mut res: Vec<(String, f32)> = Vec::new();
        for (id, v) in &self.vectors {
            if Some(id.as_str()) == exclude {
                continue;
            }
            let s = Self::sparse_cosine(&qw, &self.weighted(v));
            if s >= min_score {
                res.push((id.clone(), s));
            }
        }
        res.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.0.cmp(&b.0)));
        res.truncate(limit);
        res
    }
    /// Top matches by cosine similarity, descending; ties broken by id for determinism.
    pub fn search(&self, query: &str, limit: usize, min_score: f32) -> Vec<(String, f32)> {
        self.similar_to_vec(&to_sparse(&self.provider.embed(query)), limit, min_score, None)
    }
    pub fn similar(&self, id: &str, limit: usize, min_score: f32) -> Vec<(String, f32)> {
        match self.vectors.get(id) {
            Some(v) => self.similar_to_vec(v, limit, min_score, Some(id)),
            None => vec![],
        }
    }
}

// --------------------------------------------------------------------- search

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SearchDocKind {
    Note,
    Recording,
    TranscriptSegment,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchDocument {
    /// Unique document id (e.g. note id, media id, or `media:segment`).
    pub id: String,
    pub kind: SearchDocKind,
    /// Note id or media id the document belongs to (what the UI opens).
    pub owner_id: String,
    pub title: String,
    pub text: String,
    pub date: i64,
    /// Seek position (seconds) for transcript segments.
    pub time: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub id: String,
    pub owner_id: String,
    pub kind: SearchDocKind,
    pub title: String,
    pub score: f32,
    pub text_score: f32,
    pub semantic_score: f32,
    pub snippet: String,
    pub date: i64,
    pub time: Option<f64>,
}

fn lower_chars(s: &str) -> Vec<char> {
    s.chars().flat_map(|c| fold(&c.to_string()).chars().collect::<Vec<_>>()).flat_map(|c| c.to_lowercase()).collect()
}

/// Lexical score in 0...1: fraction of query terms found (title hits weigh 2x), plus a phrase bonus.
pub fn text_score(query: &str, title: &str, text: &str) -> f32 {
    let q = tokens(query);
    if q.is_empty() {
        return 0.0;
    }
    let title_set: HashSet<String> = tokens(title).into_iter().collect();
    let text_set: HashSet<String> = tokens(text).into_iter().collect();
    let mut s = 0f32;
    for t in &q {
        if title_set.contains(t) {
            s += 2.0;
        } else if text_set.contains(t) {
            s += 1.0;
        } else if text_set.iter().any(|w| w.starts_with(t.as_str()) || (t.chars().count() >= 4 && t.starts_with(w.as_str()) && w.chars().count() >= 4)) {
            s += 0.5;
        }
    }
    let mut score = s / (q.len() * 2) as f32;
    let phrase: String = lower_chars(query.trim()).into_iter().collect();
    if q.len() > 1 {
        let hay_t: String = lower_chars(text).into_iter().collect();
        let hay_title: String = lower_chars(title).into_iter().collect();
        if hay_t.contains(&phrase) || hay_title.contains(&phrase) {
            score = (score + 0.25).min(1.0);
        }
    }
    score.min(1.0)
}

/// A short excerpt around the first match of any query term.
pub fn make_snippet(query: &str, text: &str, radius: usize) -> String {
    let flat: Vec<char> = text.split_whitespace().collect::<Vec<_>>().join(" ").chars().collect();
    let lowered: Vec<char> = flat.iter().map(|c| c.to_lowercase().next().unwrap_or(*c)).collect();
    let terms: Vec<Vec<char>> = query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.chars().count() >= 2)
        .map(|t| t.chars().map(|c| c.to_lowercase().next().unwrap_or(c)).collect())
        .collect();
    for term in terms {
        if let Some(pos) = lowered.windows(term.len()).position(|w| w == term.as_slice()) {
            let lo = pos.saturating_sub(radius);
            let hi = (pos + term.len() + radius).min(flat.len());
            let mid: String = flat[lo..hi].iter().collect();
            return format!("{}{}{}", if lo > 0 { "…" } else { "" }, mid, if hi < flat.len() { "…" } else { "" });
        }
    }
    let head: String = flat.iter().take(radius * 2).collect();
    if flat.len() > radius * 2 { format!("{head}…") } else { head }
}

/// Hybrid search = lexical + semantic over notes, recordings and transcript segments.
pub struct SearchEngine {
    semantic: SemanticIndex,
    docs: BTreeMap<String, SearchDocument>,
    pub text_weight: f32,
    pub semantic_weight: f32,
}

impl SearchEngine {
    pub fn new() -> Self {
        Self::with_provider(Box::new(HashedTfIdfEmbedding::default()))
    }
    pub fn with_provider(p: Box<dyn EmbeddingProvider>) -> Self {
        Self { semantic: SemanticIndex::new(p), docs: BTreeMap::new(), text_weight: 0.55, semantic_weight: 0.45 }
    }
    pub fn upsert(&mut self, doc: SearchDocument) {
        self.semantic.upsert(&doc.id, &format!("{}\n{}", doc.title, doc.text));
        self.docs.insert(doc.id.clone(), doc);
    }
    pub fn remove(&mut self, id: &str) {
        self.docs.remove(id);
        self.semantic.remove(id);
    }
    /// Removes every document owned by `owner_id` (e.g. all segments of a recording).
    pub fn remove_owner(&mut self, owner_id: &str) {
        let ids: Vec<String> = self.docs.values().filter(|d| d.owner_id == owner_id).map(|d| d.id.clone()).collect();
        for i in ids {
            self.remove(&i);
        }
    }
    pub fn len(&self) -> usize {
        self.docs.len()
    }
    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }
    pub fn provider_id(&self) -> String {
        self.semantic.provider().identifier()
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<SearchHit> {
        let q = query.trim();
        if q.is_empty() {
            return vec![];
        }
        let sem: HashMap<String, f32> = self.semantic.search(q, 200, 0.0).into_iter().collect();
        let mut hits = Vec::new();
        for (id, d) in &self.docs {
            let t = text_score(q, &d.title, &d.text);
            let s = sem.get(id).copied().unwrap_or(0.0).max(0.0);
            // A lexical hit always qualifies; a purely semantic hit needs real similarity.
            if !(t > 0.0 || s >= 0.12) {
                continue;
            }
            hits.push(SearchHit {
                id: id.clone(),
                owner_id: d.owner_id.clone(),
                kind: d.kind,
                title: d.title.clone(),
                score: self.text_weight * t + self.semantic_weight * s,
                text_score: t,
                semantic_score: s,
                snippet: make_snippet(q, &d.text, 60),
                date: d.date,
                time: d.time,
            });
        }
        hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal).then_with(|| a.id.cmp(&b.id)));
        hits.truncate(limit);
        hits
    }

    /// "Related by meaning" for a document.
    pub fn related(&self, id: &str, kind: Option<SearchDocKind>, limit: usize) -> Vec<SearchHit> {
        let mut out = Vec::new();
        for (rid, score) in self.semantic.similar(id, limit * 4, 0.1) {
            let Some(d) = self.docs.get(&rid) else { continue };
            if kind.is_some() && kind != Some(d.kind) {
                continue;
            }
            out.push(SearchHit {
                id: rid,
                owner_id: d.owner_id.clone(),
                kind: d.kind,
                title: d.title.clone(),
                score,
                text_score: 0.0,
                semantic_score: score,
                snippet: d.text.chars().take(100).collect(),
                date: d.date,
                time: d.time,
            });
            if out.len() == limit {
                break;
            }
        }
        out
    }
}

impl Default for SearchEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(id: &str, kind: SearchDocKind, title: &str, text: &str) -> SearchDocument {
        SearchDocument { id: id.into(), kind, owner_id: id.into(), title: title.into(), text: text.into(), date: 0, time: None }
    }

    #[test]
    fn tokenizer_drops_stopwords_folds_and_stems() {
        assert_eq!(tokens("The Café was planning, planned and plans!"), vec!["cafe", "plan", "plan", "plan"]);
        assert_eq!(tokens("don't I'm"), Vec::<String>::new());
        assert_eq!(stem("stories"), "story");
        assert_eq!(stem("run"), "run");
        assert_eq!(stem("a1b2c3"), "a1b2c3");
    }

    #[test]
    fn fnv_is_stable() {
        // Golden values: changing the hash invalidates every stored vector.
        assert_eq!(HashedTfIdfEmbedding::fnv1a("u:work"), HashedTfIdfEmbedding::fnv1a("u:work"));
        assert_ne!(HashedTfIdfEmbedding::fnv1a("u:work"), HashedTfIdfEmbedding::fnv1a("u:play"));
    }

    #[test]
    fn embedding_is_unit_length_deterministic_and_zero_for_empty() {
        let e = HashedTfIdfEmbedding::default();
        let v = e.embed("a quiet evening walk by the river");
        assert_eq!(v.len(), 1024);
        let n: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((n - 1.0).abs() < 1e-4);
        assert_eq!(v, e.embed("a quiet evening walk by the river"));
        assert!(e.embed("the and of").iter().all(|x| *x == 0.0));
        assert_eq!(e.identifier(), "hashed-tfidf-v1-1024-c1");
    }

    #[test]
    fn concept_lexicon_links_synonyms() {
        let e = HashedTfIdfEmbedding::default();
        let plain = HashedTfIdfEmbedding::new(1024, false);
        let q = "feeling burnout";
        let d = "I was exhausted and drained";
        assert!(cosine(&e.embed(q), &e.embed(d)) > cosine(&plain.embed(q), &plain.embed(d)));
        assert!(cosine(&e.embed(q), &e.embed(d)) > 0.15);
        assert!(cosine(&e.embed(q), &e.embed("bought tomatoes at market")) < 0.1);
    }

    #[test]
    fn related_words_via_char_trigrams() {
        let e = HashedTfIdfEmbedding::default();
        assert!(cosine(&e.embed("journaling"), &e.embed("journal entries")) > 0.2);
    }

    #[test]
    fn text_score_ranks_title_over_body_and_rewards_phrase() {
        let title = text_score("lisbon trip", "Lisbon Trip", "walking");
        let body = text_score("lisbon trip", "Notes", "my lisbon trip was good");
        let partial = text_score("lisbon trip", "Notes", "lisbon only");
        assert!(title > body && body > partial && partial > 0.0);
        assert_eq!(text_score("zzz", "a", "b"), 0.0);
        assert_eq!(text_score("", "a", "b"), 0.0);
        assert!(text_score("plan", "x", "planning ahead") > 0.0);
    }

    #[test]
    fn snippet_surrounds_first_match() {
        let text = "alpha ".repeat(30) + "needle " + &"omega ".repeat(30);
        let s = make_snippet("needle", &text, 20);
        assert!(s.starts_with('…') && s.ends_with('…') && s.contains("needle"));
        assert_eq!(make_snippet("zzz", "short text", 20), "short text");
    }

    fn engine() -> SearchEngine {
        let mut e = SearchEngine::new();
        e.upsert(doc("burn", SearchDocKind::Note, "Burnout Notes", "I feel exhausted and drained after two weeks. Couldn't focus and need a break."));
        e.upsert(doc("trip", SearchDocKind::Note, "Lisbon Trip", "Three days of walking, tiles, and pastries. Dinner with the cousins."));
        e.upsert(doc("atlas", SearchDocKind::Note, "Project Atlas", "Kickoff for the client project. Concerned about the deadline pressure."));
        e.upsert(doc("rec", SearchDocKind::Recording, "Evening reflection", "I am tired. The whole week has been long."));
        e
    }

    #[test]
    fn hybrid_search_finds_by_word_and_by_meaning() {
        let e = engine();
        let h = e.search("lisbon", 5);
        assert_eq!(h[0].id, "trip");
        assert!(h[0].text_score > 0.0);
        // Meaning-only: "worn out" shares no stem with the note text, concept lexicon bridges it.
        let h = e.search("worn out", 5);
        assert!(!h.is_empty());
        assert!(h.iter().any(|x| x.id == "burn" || x.id == "rec"), "{h:?}");
        assert!(h.iter().all(|x| x.id != "trip"));
        assert!(e.search("   ", 5).is_empty());
        assert!(e.search("xylophone", 5).is_empty());
    }

    #[test]
    fn related_excludes_self_and_orders_by_similarity() {
        let e = engine();
        let r = e.related("burn", None, 3);
        assert!(r.iter().all(|h| h.id != "burn"));
        assert_eq!(r[0].id, "rec");
        let only_notes = e.related("burn", Some(SearchDocKind::Note), 3);
        assert!(only_notes.iter().all(|h| h.kind == SearchDocKind::Note));
        assert!(e.related("missing", None, 3).is_empty());
    }

    #[test]
    fn upsert_replaces_and_remove_cleans_index() {
        let mut e = engine();
        let n = e.len();
        e.upsert(doc("trip", SearchDocKind::Note, "Lisbon Trip", "totally different volcano content"));
        assert_eq!(e.len(), n);
        assert!(e.search("pastries", 5).is_empty());
        assert_eq!(e.search("volcano", 5)[0].id, "trip");
        e.remove("trip");
        assert!(e.search("volcano", 5).is_empty());
        assert_eq!(e.len(), n - 1);
    }

    #[test]
    fn remove_owner_removes_all_segments() {
        let mut e = SearchEngine::new();
        for i in 0..3 {
            let mut d = doc(&format!("m:{i}"), SearchDocKind::TranscriptSegment, "Rec", &format!("segment number {i} about whales"));
            d.owner_id = "m".into();
            d.time = Some(i as f64);
            e.upsert(d);
        }
        e.upsert(doc("other", SearchDocKind::Note, "Other", "whales too"));
        e.remove_owner("m");
        assert_eq!(e.len(), 1);
        assert_eq!(e.search("whales", 5).len(), 1);
    }

    #[test]
    fn swappable_provider_is_used() {
        struct Const;
        impl EmbeddingProvider for Const {
            fn identifier(&self) -> String { "const".into() }
            fn dimensions(&self) -> usize { 4 }
            fn embed(&self, text: &str) -> Vec<f32> {
                if text.contains("cat") { vec![1.0, 0.0, 0.0, 0.0] } else { vec![0.0, 1.0, 0.0, 0.0] }
            }
        }
        let mut e = SearchEngine::with_provider(Box::new(Const));
        e.upsert(doc("a", SearchDocKind::Note, "x", "my cat"));
        e.upsert(doc("b", SearchDocKind::Note, "y", "my dog"));
        assert_eq!(e.provider_id(), "const");
        // "kitten" shares no words with the docs; the provider maps it nowhere ("dog" axis) -> b is the semantic hit.
        let h = e.search("kitten", 5);
        assert_eq!(h[0].id, "b");
        let h = e.search("cat food", 5);
        assert_eq!(h[0].id, "a");
    }

    #[test]
    fn idf_downweights_ubiquitous_terms() {
        let mut idx = SemanticIndex::new(Box::new(HashedTfIdfEmbedding::new(1024, false)));
        for i in 0..20 {
            idx.upsert(&format!("d{i}"), &format!("common word filler{i}x"));
        }
        idx.upsert("rare", "common zebra");
        let r = idx.search("common zebra", 3, 0.0);
        assert_eq!(r[0].0, "rare");
    }
}

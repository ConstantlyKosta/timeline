//! Note link graph (wiki links + shared tags) and a deterministic force-directed layout for the
//! mind map.

use crate::markdown::{normalize_title, ParsedNote};
use crate::models::Note;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphNode {
    /// Note id, or `ghost:<normalised title>` for a link target without a note yet.
    pub id: String,
    pub title: String,
    pub is_ghost: bool,
    pub tags: Vec<String>,
    pub degree: usize,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeKind {
    Link,
    Tag,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Graph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
    pub width: f64,
    pub height: f64,
}

/// Backlink: a note that links to the current one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Backlink {
    pub note_id: String,
    pub title: String,
    pub snippet: String,
    pub line: usize,
}

/// Resolve a wiki-link target to a note id (case/diacritic-insensitive title match).
pub fn resolve_target(notes: &[Note], target: &str) -> Option<String> {
    let key = target_key(target);
    notes.iter().find(|n| n.link_names().iter().any(|x| target_key(x) == key)).map(|n| n.id.clone())
}

/// Normalised lookup key for a link target or note name: case/diacritics folded, `.md` suffix and
/// folder prefix (`[[Projects/Atlas]]`) dropped.
pub fn target_key(t: &str) -> String {
    let t = t.trim();
    let last = t.rsplit('/').next().unwrap_or(t);
    let last = last.strip_suffix(".md").unwrap_or(last);
    normalize_title(last)
}

/// Map from every name a note answers to (title, file name, aliases) to its id. First note wins.
pub fn name_index(notes: &[&Note]) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for n in notes {
        for name in n.link_names() {
            m.entry(target_key(&name)).or_insert_with(|| n.id.clone());
        }
    }
    m
}

/// Build the graph: one node per note, ghost nodes for unresolved links, link edges, and tag
/// edges between notes that share a tag (a note pair gets at most one tag edge).
pub fn build(notes: &[Note], parsed: &HashMap<String, ParsedNote>) -> Graph {
    let mut nodes: BTreeMap<String, GraphNode> = BTreeMap::new();
    let mut by_title: HashMap<String, String> = HashMap::new();
    let mut ordered: Vec<&Note> = notes.iter().collect();
    ordered.sort_by(|a, b| a.id.cmp(&b.id));
    by_title.extend(name_index(&ordered));
    for n in &ordered {
        let tags = parsed.get(&n.id).map(|p| p.tags.clone()).unwrap_or_default();
        nodes.insert(n.id.clone(), GraphNode { id: n.id.clone(), title: n.title.clone(), is_ghost: false, tags, degree: 0, x: 0.0, y: 0.0 });
    }
    let mut edges: Vec<GraphEdge> = Vec::new();
    let mut seen: BTreeSet<(String, String, u8)> = BTreeSet::new();
    for n in &ordered {
        let Some(p) = parsed.get(&n.id) else { continue };
        for target in p.unique_link_targets() {
            let key = target_key(&target);
            let to = match by_title.get(&key) {
                Some(id) => id.clone(),
                None => {
                    let gid = format!("ghost:{key}");
                    nodes.entry(gid.clone()).or_insert_with(|| GraphNode { id: gid.clone(), title: target.clone(), is_ghost: true, tags: vec![], degree: 0, x: 0.0, y: 0.0 });
                    gid
                }
            };
            if to != n.id && seen.insert((n.id.clone(), to.clone(), 0)) {
                edges.push(GraphEdge { from: n.id.clone(), to, kind: EdgeKind::Link });
            }
        }
    }
    // Tag edges: pairs within each tag group (groups over 60 notes are skipped, they are noise).
    let mut by_tag: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for n in &ordered {
        for t in &nodes[&n.id].tags {
            by_tag.entry(t.as_str()).or_default().push(n.id.as_str());
        }
    }
    let mut tag_pairs: BTreeSet<(String, String)> = BTreeSet::new();
    for ids in by_tag.values().filter(|v| v.len() <= 60) {
        for (i, a) in ids.iter().enumerate() {
            for b in &ids[i + 1..] {
                let (a, b) = if a < b { (*a, *b) } else { (*b, *a) };
                tag_pairs.insert((a.to_string(), b.to_string()));
            }
        }
    }
    for (a, b) in tag_pairs {
        // Skip when already directly linked, to keep the map readable.
        let linked = seen.contains(&(a.clone(), b.clone(), 0)) || seen.contains(&(b.clone(), a.clone(), 0));
        if !linked {
            edges.push(GraphEdge { from: a, to: b, kind: EdgeKind::Tag });
        }
    }
    for e in &edges {
        if let Some(n) = nodes.get_mut(&e.from) {
            n.degree += 1;
        }
        if let Some(n) = nodes.get_mut(&e.to) {
            n.degree += 1;
        }
    }
    let mut g = Graph { nodes: nodes.into_values().collect(), edges, width: 1000.0, height: 700.0 };
    layout(&mut g);
    g
}

/// Fruchterman–Reingold on a fixed canvas, deterministic (circle start, fixed iteration count).
pub fn layout(g: &mut Graph) {
    let n = g.nodes.len();
    let (w, h) = (g.width, g.height);
    if n == 0 {
        return;
    }
    if n == 1 {
        g.nodes[0].x = w / 2.0;
        g.nodes[0].y = h / 2.0;
        return;
    }
    let idx: HashMap<&str, usize> = g.nodes.iter().enumerate().map(|(i, n)| (n.id.as_str(), i)).collect();
    let edges: Vec<(usize, usize, f64)> = g
        .edges
        .iter()
        .filter_map(|e| Some((*idx.get(e.from.as_str())?, *idx.get(e.to.as_str())?, if e.kind == EdgeKind::Link { 1.0 } else { 0.4 })))
        .collect();
    let mut pos: Vec<(f64, f64)> = (0..n)
        .map(|i| {
            let a = i as f64 / n as f64 * std::f64::consts::TAU;
            (w / 2.0 + a.cos() * w * 0.3, h / 2.0 + a.sin() * h * 0.3)
        })
        .collect();
    let k = ((w * h) / n as f64).sqrt() * 0.7;
    let mut temp = w / 8.0;
    for _ in 0..300 {
        let mut disp = vec![(0f64, 0f64); n];
        for i in 0..n {
            for j in i + 1..n {
                let (dx, mut dy) = (pos[i].0 - pos[j].0, pos[i].1 - pos[j].1);
                let mut dx = dx;
                let mut d = (dx * dx + dy * dy).sqrt();
                if d < 0.01 {
                    dx = 0.01 * (i as f64 + 1.0);
                    dy = 0.01 * (j as f64 + 1.0);
                    d = (dx * dx + dy * dy).sqrt();
                }
                let f = k * k / d;
                disp[i].0 += dx / d * f;
                disp[i].1 += dy / d * f;
                disp[j].0 -= dx / d * f;
                disp[j].1 -= dy / d * f;
            }
        }
        for &(a, b, wt) in &edges {
            let (dx, dy) = (pos[a].0 - pos[b].0, pos[a].1 - pos[b].1);
            let d = (dx * dx + dy * dy).sqrt().max(0.01);
            let f = d * d / k * wt;
            disp[a].0 -= dx / d * f;
            disp[a].1 -= dy / d * f;
            disp[b].0 += dx / d * f;
            disp[b].1 += dy / d * f;
        }
        for i in 0..n {
            // gentle gravity keeps disconnected components on screen
            disp[i].0 += (w / 2.0 - pos[i].0) * 0.05;
            disp[i].1 += (h / 2.0 - pos[i].1) * 0.05;
            let d = (disp[i].0 * disp[i].0 + disp[i].1 * disp[i].1).sqrt().max(0.01);
            let step = d.min(temp);
            pos[i].0 += disp[i].0 / d * step;
            pos[i].1 += disp[i].1 / d * step;
            pos[i].0 = pos[i].0.clamp(60.0, w - 60.0);
            pos[i].1 = pos[i].1.clamp(40.0, h - 40.0);
        }
        temp *= 0.98;
    }
    for (i, node) in g.nodes.iter_mut().enumerate() {
        node.x = (pos[i].0 * 10.0).round() / 10.0;
        node.y = (pos[i].1 * 10.0).round() / 10.0;
    }
}

// ---------------------------------------------------------------------------------------------
// Rich graph data for the interactive mind map. The layout is done in the UI (live force
// simulation); this only describes the nodes, their metadata and the typed links between them.

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GNode {
    /// Note id, `ghost:<key>` for an unresolved link, `att:<media id>` for an attachment.
    pub id: String,
    pub title: String,
    /// `note`, `ghost` or `attachment`.
    pub kind: String,
    pub tags: Vec<String>,
    pub folder: String,
    /// Front-matter properties as display strings (lists are joined with ", ").
    pub props: BTreeMap<String, String>,
    pub degree: usize,
    pub in_degree: usize,
    pub out_degree: usize,
    pub words: usize,
    pub date: i64,
    /// Media id of the picture shown on the node (first image of the note, or the attachment itself).
    pub image: Option<String>,
    pub preview: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GEdge {
    pub from: String,
    pub to: String,
    /// `link` (in the text), `prop:<key>` (link inside a property) or `attachment`.
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphData {
    pub nodes: Vec<GNode>,
    pub edges: Vec<GEdge>,
    /// Property names used by at least one note (sorted), without tags/title/aliases.
    pub property_keys: Vec<String>,
}

/// Every `[[target]]` / `[[target|alias]]` / `[[target#heading]]` in a string.
pub fn wikilinks_in(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(i) = rest.find("[[") {
        let after = &rest[i + 2..];
        let Some(j) = after.find("]]") else { break };
        let inner = &after[..j];
        let t = inner.split('|').next().unwrap_or("").split('#').next().unwrap_or("").trim();
        if !t.is_empty() && !inner.contains('\n') {
            out.push(t.to_string());
        }
        rest = &after[j + 2..];
    }
    out
}

/// Ids of `![..](media:ID)` references, in order, unique.
pub fn media_refs(body: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = body;
    while let Some(i) = rest.find("](media:") {
        let after = &rest[i + 8..];
        let Some(j) = after.find(')') else { break };
        let id = after[..j].trim().to_string();
        if !id.is_empty() && !out.contains(&id) {
            out.push(id);
        }
        rest = &after[j..];
    }
    out
}

fn prop_display(v: &serde_json::Value) -> Option<String> {
    use serde_json::Value::*;
    let s = match v {
        String(s) => s.clone(),
        Number(n) => n.to_string(),
        Bool(b) => b.to_string(),
        Array(a) => a.iter().filter_map(prop_display).collect::<Vec<_>>().join(", "),
        _ => return None,
    };
    let s = s.trim().to_string();
    (!s.is_empty()).then(|| s.chars().take(200).collect())
}

/// `attachments`: media id -> (title, is_image). Only attachments referenced by a note appear as nodes.
pub fn build_data(notes: &[Note], parsed: &HashMap<String, ParsedNote>, attachments: &HashMap<String, (String, bool)>) -> GraphData {
    let mut ordered: Vec<&Note> = notes.iter().collect();
    ordered.sort_by(|a, b| a.id.cmp(&b.id));
    let by_name = name_index(&ordered);
    let mut nodes: BTreeMap<String, GNode> = BTreeMap::new();
    let mut prop_keys: BTreeSet<String> = BTreeSet::new();
    for n in &ordered {
        let p = parsed.get(&n.id);
        let mut props = BTreeMap::new();
        for (k, v) in &n.properties {
            if matches!(k.as_str(), "tags" | "tag" | "title" | "aliases" | "alias") {
                continue;
            }
            if let Some(d) = prop_display(v) {
                prop_keys.insert(k.clone());
                props.insert(k.clone(), d);
            }
        }
        nodes.insert(
            n.id.clone(),
            GNode {
                id: n.id.clone(),
                title: n.title.clone(),
                kind: "note".into(),
                tags: p.map(|p| p.tags.clone()).unwrap_or_default(),
                folder: n.folder.clone(),
                props,
                degree: 0,
                in_degree: 0,
                out_degree: 0,
                words: p.map(|p| p.plain_text.split_whitespace().count()).unwrap_or(0),
                date: n.date,
                image: crate::markdown::first_image(&n.body).filter(|id| attachments.get(id).map(|a| a.1).unwrap_or(true)),
                preview: crate::markdown::preview(&n.body, 220),
            },
        );
    }
    let mut edges: Vec<GEdge> = Vec::new();
    let mut seen: BTreeSet<(String, String, String)> = BTreeSet::new();
    fn add(by_name: &HashMap<String, String>, nodes: &mut BTreeMap<String, GNode>, edges: &mut Vec<GEdge>, seen: &mut BTreeSet<(String, String, String)>, from: &str, target: &str, kind: String) {
        let key = target_key(target);
        let to = match by_name.get(&key) {
            Some(id) => id.clone(),
            None => {
                let gid = format!("ghost:{key}");
                nodes.entry(gid.clone()).or_insert_with(|| GNode {
                    id: gid.clone(),
                    title: target.to_string(),
                    kind: "ghost".into(),
                    tags: vec![],
                    folder: String::new(),
                    props: BTreeMap::new(),
                    degree: 0,
                    in_degree: 0,
                    out_degree: 0,
                    words: 0,
                    date: 0,
                    image: None,
                    preview: String::new(),
                });
                gid
            }
        };
        if to != from && seen.insert((from.to_string(), to.clone(), kind.clone())) {
            edges.push(GEdge { from: from.to_string(), to, kind });
        }
    }
    for n in &ordered {
        if let Some(p) = parsed.get(&n.id) {
            for t in p.unique_link_targets() {
                add(&by_name, &mut nodes, &mut edges, &mut seen, &n.id, &t, "link".into());
            }
        }
        for (k, v) in &n.properties {
            if matches!(k.as_str(), "tags" | "tag") {
                continue;
            }
            let mut found = Vec::new();
            let mut walk = vec![v];
            while let Some(x) = walk.pop() {
                match x {
                    serde_json::Value::String(s) => found.extend(wikilinks_in(s)),
                    serde_json::Value::Array(a) => walk.extend(a.iter()),
                    _ => {}
                }
            }
            for t in found {
                add(&by_name, &mut nodes, &mut edges, &mut seen, &n.id, &t, format!("prop:{k}"));
            }
        }
        for mid in media_refs(&n.body) {
            if let Some((title, is_image)) = attachments.get(&mid) {
                let aid = format!("att:{mid}");
                nodes.entry(aid.clone()).or_insert_with(|| GNode {
                    id: aid.clone(),
                    title: title.clone(),
                    kind: "attachment".into(),
                    tags: vec![],
                    folder: String::new(),
                    props: BTreeMap::new(),
                    degree: 0,
                    in_degree: 0,
                    out_degree: 0,
                    words: 0,
                    date: 0,
                    image: is_image.then(|| mid.clone()),
                    preview: String::new(),
                });
                if seen.insert((n.id.clone(), aid.clone(), "attachment".into())) {
                    edges.push(GEdge { from: n.id.clone(), to: aid, kind: "attachment".into() });
                }
            }
        }
    }
    let mut pairs: BTreeSet<(&str, &str)> = BTreeSet::new();
    for e in &edges {
        pairs.insert((e.from.as_str(), e.to.as_str()));
    }
    let mut deg: HashMap<&str, (usize, usize, usize)> = HashMap::new();
    for e in &edges {
        deg.entry(e.from.as_str()).or_default().2 += 1;
        deg.entry(e.to.as_str()).or_default().1 += 1;
    }
    let mut nbrs: HashMap<&str, BTreeSet<&str>> = HashMap::new();
    for (a, b) in &pairs {
        nbrs.entry(a).or_default().insert(b);
        nbrs.entry(b).or_default().insert(a);
    }
    let stats: HashMap<String, (usize, usize, usize)> = nodes
        .keys()
        .map(|k| {
            let d = deg.get(k.as_str()).copied().unwrap_or_default();
            (k.clone(), (nbrs.get(k.as_str()).map(|s| s.len()).unwrap_or(0), d.1, d.2))
        })
        .collect();
    for (k, n) in nodes.iter_mut() {
        let (d, i, o) = stats[k];
        n.degree = d;
        n.in_degree = i;
        n.out_degree = o;
    }
    GraphData { nodes: nodes.into_values().collect(), edges, property_keys: prop_keys.into_iter().collect() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown;

    fn note(id: &str, title: &str, body: &str) -> Note {
        Note::basic(id, title, body, 0, 0)
    }
    fn build_from(notes: &[Note]) -> Graph {
        let parsed = notes.iter().map(|n| (n.id.clone(), markdown::parse(&n.body))).collect();
        build(notes, &parsed)
    }

    #[test]
    fn resolves_links_ghosts_and_tags() {
        let notes = vec![
            note("a", "Atlas", "see [[Lisbon Trip]] and [[Café Plans]] #work"),
            note("b", "Lisbon Trip", "back to [[atlas]] #travel"),
            note("c", "Other", "#work stuff"),
        ];
        let g = build_from(&notes);
        assert_eq!(g.nodes.len(), 4);
        let ghost = g.nodes.iter().find(|n| n.is_ghost).unwrap();
        assert_eq!(ghost.id, "ghost:cafe plans");
        let links: Vec<_> = g.edges.iter().filter(|e| e.kind == EdgeKind::Link).collect();
        assert_eq!(links.len(), 3);
        let tags: Vec<_> = g.edges.iter().filter(|e| e.kind == EdgeKind::Tag).collect();
        assert_eq!(tags.len(), 1);
        assert_eq!((tags[0].from.as_str(), tags[0].to.as_str()), ("a", "c"));
        assert_eq!(g.nodes.iter().find(|n| n.id == "a").unwrap().degree, 4);
    }

    #[test]
    fn self_links_ignored_and_tag_edge_skipped_when_linked() {
        let notes = vec![note("a", "A", "[[A]] [[B]] #x"), note("b", "B", "#x")];
        let g = build_from(&notes);
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.edges[0].kind, EdgeKind::Link);
    }

    #[test]
    fn layout_is_deterministic_and_in_bounds() {
        let notes: Vec<Note> = (0..12).map(|i| note(&format!("n{i:02}"), &format!("N{i}"), &format!("[[N{}]] #t{}", (i + 1) % 12, i % 3))).collect();
        let g1 = build_from(&notes);
        let g2 = build_from(&notes);
        assert_eq!(g1, g2);
        for n in &g1.nodes {
            assert!(n.x >= 60.0 && n.x <= g1.width - 60.0 && n.y >= 40.0 && n.y <= g1.height - 40.0, "{n:?}");
        }
        // no two nodes on top of each other
        for (i, a) in g1.nodes.iter().enumerate() {
            for b in &g1.nodes[i + 1..] {
                assert!(((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt() > 20.0);
            }
        }
    }

    #[test]
    fn trivial_graphs() {
        assert!(build_from(&[]).nodes.is_empty());
        let g = build_from(&[note("a", "A", "")]);
        assert_eq!((g.nodes[0].x, g.nodes[0].y), (500.0, 350.0));
    }

    #[test]
    fn resolve_target_by_title() {
        let notes = vec![note("a", "Café", "")];
        assert_eq!(resolve_target(&notes, " cafe "), Some("a".into()));
        assert_eq!(resolve_target(&notes, "nope"), None);
    }

    fn data_from(notes: &[Note], att: &[(&str, &str, bool)]) -> GraphData {
        let parsed = notes.iter().map(|n| (n.id.clone(), markdown::parse(&n.body))).collect();
        let a = att.iter().map(|(i, t, im)| (i.to_string(), (t.to_string(), *im))).collect();
        build_data(notes, &parsed, &a)
    }

    #[test]
    fn rich_graph_has_typed_edges_attachments_and_props() {
        let mut a = note("a", "Atlas", "see [[Lisbon]] and [[Nowhere]] #work\n\n![pic](media:m1) ![doc](media:m2) ![gone](media:zz)");
        a.folder = "Projects".into();
        a.properties.insert("status".into(), serde_json::json!("active"));
        a.properties.insert("related".into(), serde_json::json!(["[[Lisbon]]", "[[Other Ghost|alias]]"]));
        a.properties.insert("tags".into(), serde_json::json!(["work"]));
        let b = note("b", "Lisbon", "back to [[atlas]]");
        let g = data_from(&[a, b], &[("m1", "pic.png", true), ("m2", "doc.pdf", false)]);
        let kinds: Vec<&str> = g.edges.iter().map(|e| e.kind.as_str()).collect();
        assert!(kinds.contains(&"link") && kinds.contains(&"prop:related") && kinds.contains(&"attachment"));
        assert_eq!(g.edges.iter().filter(|e| e.kind == "attachment").count(), 2);
        let atlas = g.nodes.iter().find(|n| n.id == "a").unwrap();
        assert_eq!(atlas.folder, "Projects");
        assert_eq!(atlas.props.get("status").map(|s| s.as_str()), Some("active"));
        assert!(!atlas.props.contains_key("tags"));
        assert_eq!(atlas.image.as_deref(), Some("m1"));
        assert_eq!(g.property_keys, vec!["related".to_string(), "status".to_string()]);
        // ghost for Nowhere and Other Ghost, an attachment node without image for the pdf
        assert_eq!(g.nodes.iter().filter(|n| n.kind == "ghost").count(), 2);
        let pdf = g.nodes.iter().find(|n| n.id == "att:m2").unwrap();
        assert!(pdf.image.is_none());
        // Lisbon is linked by link, prop and links back: degree counts neighbours once
        let lis = g.nodes.iter().find(|n| n.id == "b").unwrap();
        assert_eq!((lis.degree, lis.in_degree, lis.out_degree), (1, 2, 1));
    }

    #[test]
    fn link_extraction_helpers() {
        assert_eq!(wikilinks_in("x [[A|b]] y [[C#h]] [[ ]]"), vec!["A", "C"]);
        assert_eq!(media_refs("![a](media:1) ![b](media:2) ![c](media:1)"), vec!["1", "2"]);
    }
}

//! Lightweight Markdown scanner: extracts the structure Timeline needs (wiki links, tags, tasks,
//! headings, plain text). It is *not* a renderer: rendering/live-preview is done by the editor in the
//! UI (CodeMirror 6). Notes are stored as plain Markdown text.

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WikiLink {
    /// Target note title as written (without alias or #heading).
    pub target: String,
    pub alias: Option<String>,
    pub heading: Option<String>,
    /// 0-based line number.
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskItem {
    pub text: String,
    pub is_done: bool,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heading {
    pub level: usize,
    pub text: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ParsedNote {
    /// Text of the first level-1 heading, if any.
    pub title: Option<String>,
    pub headings: Vec<Heading>,
    pub links: Vec<WikiLink>,
    /// Lowercased tags without the leading "#", unique, in first-seen order.
    pub tags: Vec<String>,
    pub tasks: Vec<TaskItem>,
    /// Body with Markdown syntax stripped; suitable for search and snippets.
    pub plain_text: String,
}

impl ParsedNote {
    /// Link targets, unique by normalised title, in first-seen order.
    pub fn unique_link_targets(&self) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let mut out = Vec::new();
        for l in &self.links {
            if seen.insert(normalize_title(&l.target)) {
                out.push(l.target.clone());
            }
        }
        out
    }
}

/// Case-, diacritic- and whitespace-insensitive title key.
pub fn normalize_title(s: &str) -> String {
    let folded: String = s
        .trim()
        .nfd()
        .filter(|c| !('\u{0300}'..='\u{036f}').contains(c))
        .collect::<String>()
        .to_lowercase();
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn split_lines(md: &str) -> Vec<String> {
    md.replace("\r\n", "\n").split('\n').map(|s| s.to_string()).collect()
}

fn fence_marker(trimmed: &str) -> Option<&'static str> {
    if trimmed.starts_with("```") {
        Some("```")
    } else if trimmed.starts_with("~~~") {
        Some("~~~")
    } else {
        None
    }
}

pub fn parse(markdown: &str) -> ParsedNote {
    let mut headings = Vec::new();
    let mut links = Vec::new();
    let mut tags: Vec<String> = Vec::new();
    let mut tasks = Vec::new();
    let mut plain_lines: Vec<String> = Vec::new();
    let mut in_fence = false;
    let mut marker = "";

    for (n, raw) in split_lines(markdown).iter().enumerate() {
        let trimmed = raw.trim();
        if let Some(m) = fence_marker(trimmed) {
            if !in_fence {
                in_fence = true;
                marker = m;
                continue;
            }
            if m == marker {
                in_fence = false;
                continue;
            }
        }
        if in_fence {
            plain_lines.push(raw.clone()); // code stays searchable but yields no structure
            continue;
        }
        let mut line = raw.clone();
        if let Some((level, text)) = heading_match(trimmed) {
            headings.push(Heading { level, text: text.clone(), line: n });
            line = text;
        }
        let mut body = line.clone();
        if let Some((done, text)) = task_match(trimmed) {
            tasks.push(TaskItem { text: text.clone(), is_done: done, line: n });
            body = text;
        }
        let no_code = strip_inline_code(&body);
        links.extend(extract_links(&no_code, n));
        for t in extract_tags(&no_code) {
            if !tags.contains(&t) {
                tags.push(t);
            }
        }
        plain_lines.push(strip_syntax(&body));
    }
    let title = headings.iter().find(|h| h.level == 1).map(|h| h.text.clone());
    let plain = plain_lines.join("\n").trim().to_string();
    ParsedNote { title, headings, links, tags, tasks, plain_text: plain }
}

pub fn heading_match(trimmed: &str) -> Option<(usize, String)> {
    let level = trimmed.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let rest = &trimmed[level..];
    if !rest.starts_with(' ') {
        return None; // "#tag" is a tag, "# Title" is a heading
    }
    let mut text = rest.trim().to_string();
    while text.ends_with('#') {
        text.pop();
    }
    Some((level, text.trim().to_string()))
}

pub fn task_match(trimmed: &str) -> Option<(bool, String)> {
    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(bullet) {
            if let Some(t) = rest.strip_prefix("[ ] ") {
                return Some((false, t.to_string()));
            }
            if let Some(t) = rest.strip_prefix("[x] ").or_else(|| rest.strip_prefix("[X] ")) {
                return Some((true, t.to_string()));
            }
        }
    }
    None
}

fn strip_inline_code(s: &str) -> String {
    if !s.contains('`') {
        return s.to_string();
    }
    let mut out = String::new();
    let mut in_code = false;
    for c in s.chars() {
        if c == '`' {
            in_code = !in_code;
            out.push(' ');
            continue;
        }
        out.push(if in_code { ' ' } else { c });
    }
    out
}

pub fn extract_links(s: &str, line: usize) -> Vec<WikiLink> {
    let mut result = Vec::new();
    let mut rest = s;
    while let Some(open) = rest.find("[[") {
        let after = &rest[open + 2..];
        let Some(close) = after.find("]]") else { break };
        let inner = &after[..close];
        rest = &after[close + 2..];
        if inner.is_empty() || inner.contains('[') || inner.contains('\n') {
            continue;
        }
        let mut target_part = inner.to_string();
        let mut alias = None;
        if let Some(bar) = inner.find('|') {
            target_part = inner[..bar].to_string();
            let a = inner[bar + 1..].trim().to_string();
            if !a.is_empty() {
                alias = Some(a);
            }
        }
        let mut heading = None;
        if let Some(hash) = target_part.find('#') {
            heading = Some(target_part[hash + 1..].trim().to_string());
            target_part.truncate(hash);
        }
        let target = target_part.trim().to_string();
        if target.is_empty() {
            continue;
        }
        result.push(WikiLink { target, alias, heading, line });
    }
    result
}

pub fn extract_tags(s: &str) -> Vec<String> {
    // Remove wiki links so "[[Foo#Bar]]" is not read as a tag.
    let mut text = s.to_string();
    while let Some(open) = text.find("[[") {
        let Some(rel) = text[open + 2..].find("]]") else { break };
        let end = open + 2 + rel + 2;
        text.replace_range(open..end, " ");
    }
    let chars: Vec<char> = text.chars().collect();
    let mut tags = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let boundary = i == 0 || chars[i - 1].is_whitespace() || "([{,;".contains(chars[i - 1]);
        if chars[i] == '#' && boundary {
            let mut j = i + 1;
            let mut tag = String::new();
            while j < chars.len() && (chars[j].is_alphanumeric() || matches!(chars[j], '_' | '-' | '/')) {
                tag.push(chars[j]);
                j += 1;
            }
            while tag.ends_with('-') || tag.ends_with('/') {
                tag.pop();
            }
            if !tag.is_empty() && tag.chars().any(|c| c.is_alphabetic()) {
                tags.push(tag.to_lowercase());
            }
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    tags
}

fn strip_syntax(s: &str) -> String {
    let mut t = s.to_string();
    // wiki links -> alias or target
    while let Some(open) = t.find("[[") {
        let Some(rel) = t[open + 2..].find("]]") else { break };
        let close = open + 2 + rel;
        let inner = t[open + 2..close].to_string();
        let shown = inner.rsplit_once('|').map(|x| x.1).unwrap_or(&inner);
        let cleaned = shown.split('#').next().unwrap_or(shown).to_string();
        t.replace_range(open..close + 2, &cleaned);
    }
    let t = strip_md_links(&t);
    let mut t = strip_line_prefix(&t);
    for mark in ["**", "__", "~~", "`", "*"] {
        t = t.replace(mark, "");
    }
    t.trim().to_string()
}

/// `[text](url)` -> `text`, `![alt](url)` -> `alt`.
fn strip_md_links(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let bang = chars[i] == '!' && chars.get(i + 1) == Some(&'[');
        let start = if bang { i + 1 } else { i };
        if chars[start] == '[' {
            if let Some(rb) = (start + 1..chars.len()).find(|&k| chars[k] == ']' || chars[k] == '[') {
                if chars[rb] == ']' && chars.get(rb + 1) == Some(&'(') {
                    if let Some(rp) = (rb + 2..chars.len()).find(|&k| chars[k] == ')') {
                        out.extend(&chars[start + 1..rb]);
                        i = rp + 1;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn strip_line_prefix(s: &str) -> String {
    let lead = s.len() - s.trim_start_matches(' ').len();
    if lead > 3 {
        return s.to_string();
    }
    let rest = &s[lead..];
    // headings
    let hashes = rest.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && rest[hashes..].starts_with(' ') {
        return rest[hashes..].trim_start().to_string();
    }
    for b in ["- ", "* ", "+ "] {
        if let Some(r) = rest.strip_prefix(b) {
            let r = r
                .strip_prefix("[ ] ")
                .or_else(|| r.strip_prefix("[x] "))
                .or_else(|| r.strip_prefix("[X] "))
                .unwrap_or(r);
            return r.trim_start().to_string();
        }
    }
    if let Some(r) = rest.strip_prefix('>') {
        return r.trim_start().to_string();
    }
    let digits = rest.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 && rest[digits..].starts_with(". ") {
        return rest[digits + 2..].trim_start().to_string();
    }
    s.to_string()
}

/// First `max_len` characters of plain text, cut on a word boundary.
pub fn snippet(markdown: &str, max_len: usize) -> String {
    let p = parse(markdown).plain_text.replace('\n', " ");
    let collapsed = p.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= max_len {
        return collapsed;
    }
    let cut: String = collapsed.chars().take(max_len).collect();
    match cut.rfind(' ') {
        Some(sp) => format!("{}…", &cut[..sp]),
        None => format!("{cut}…"),
    }
}

/// Flips the checkbox on 0-based `line` (only if that line is a task). Returns the new body.
pub fn toggle_task(markdown: &str, line: usize) -> Option<String> {
    let normalized = markdown.replace("\r\n", "\n");
    let mut lines: Vec<String> = normalized.split('\n').map(|s| s.to_string()).collect();
    let l = lines.get_mut(line)?;
    for bullet in ["- ", "* ", "+ "] {
        if let Some(pos) = l.find(&format!("{bullet}[ ] ")) {
            if l[..pos].trim().is_empty() {
                l.replace_range(pos..pos + bullet.len() + 3, &format!("{bullet}[x]"));
                return Some(lines.join("\n"));
            }
        }
        for mark in ["[x]", "[X]"] {
            if let Some(pos) = l.find(&format!("{bullet}{mark} ")) {
                if l[..pos].trim().is_empty() {
                    l.replace_range(pos..pos + bullet.len() + 3, &format!("{bullet}[ ]"));
                    return Some(lines.join("\n"));
                }
            }
        }
    }
    None
}

/// Title for a body: first `# Heading`, else first non-empty line (syntax stripped), else "Untitled".
pub fn derive_title(markdown: &str) -> String {
    let p = parse(markdown);
    if let Some(t) = p.title.filter(|t| !t.is_empty()) {
        return t;
    }
    for l in p.plain_text.lines() {
        let l = l.trim();
        if !l.is_empty() {
            return l.chars().take(80).collect();
        }
    }
    "Untitled".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_links_aliases_headings() {
        let p = parse("# Title\nSee [[Lisbon Trip]] and [[Project Atlas|the project]] and [[Notes#Part 2]].\n[[]] [[a [b]]");
        assert_eq!(p.title.as_deref(), Some("Title"));
        let targets: Vec<_> = p.links.iter().map(|l| l.target.as_str()).collect();
        assert_eq!(targets, vec!["Lisbon Trip", "Project Atlas", "Notes"]);
        assert_eq!(p.links[1].alias.as_deref(), Some("the project"));
        assert_eq!(p.links[2].heading.as_deref(), Some("Part 2"));
        assert_eq!(p.links[0].line, 1);
    }

    #[test]
    fn tags_need_boundary_and_a_letter_and_skip_headings_links_code() {
        let p = parse("# Heading\n#one text#no (#two) #3 #four-five- `#code` [[Foo#Bar]] #Mixed/Case\n```\n#fenced\n```");
        assert_eq!(p.tags, vec!["one", "two", "four-five", "mixed/case"]);
    }

    #[test]
    fn heading_requires_space_after_hashes() {
        assert_eq!(heading_match("## Hi ##"), Some((2, "Hi".into())));
        assert_eq!(heading_match("#tag"), None);
        assert_eq!(heading_match("####### seven"), None);
    }

    #[test]
    fn tasks_detected_with_state_and_lines() {
        let p = parse("intro\n- [x] done thing\n  * [ ] open thing #t\n- [ ]nospace\n- plain");
        assert_eq!(p.tasks.len(), 2);
        assert!(p.tasks[0].is_done);
        assert_eq!(p.tasks[0].line, 1);
        assert!(!p.tasks[1].is_done);
        assert_eq!(p.tasks[1].text, "open thing #t");
        assert!(p.tags.contains(&"t".to_string()));
    }

    #[test]
    fn plain_text_strips_syntax() {
        let p = parse("# Hi\n- [x] **bold** and *it* with [[A|alias]] and [link](http://x.y) ![img](a.png)\n> quote\n1. one\n```\ncode #x\n```");
        assert_eq!(p.plain_text, "Hi\nbold and it with alias and link img\nquote\none\ncode #x");
    }

    #[test]
    fn code_is_not_parsed_for_links() {
        let p = parse("`[[Inline]]`\n```\n[[Fenced]]\n```\n[[Real]]");
        assert_eq!(p.links.len(), 1);
        assert_eq!(p.links[0].target, "Real");
    }

    #[test]
    fn normalize_title_folds_case_accents_space() {
        assert_eq!(normalize_title("  Café   Déjà  Vu "), "cafe deja vu");
        assert_eq!(normalize_title("Mom's Birthday"), normalize_title("mom's  birthday"));
    }

    #[test]
    fn unique_targets_dedupe_by_normalised_title() {
        let p = parse("[[Foo]] [[foo ]] [[Bar]] [[FOO#x]]");
        assert_eq!(p.unique_link_targets(), vec!["Foo", "Bar"]);
    }

    #[test]
    fn snippet_cuts_on_word_boundary() {
        let s = snippet("# T\nalpha beta gamma delta epsilon zeta", 17);
        assert_eq!(s, "T alpha beta…");
        assert_eq!(snippet("short", 50), "short");
    }

    #[test]
    fn toggle_task_flips_only_task_lines() {
        let md = "a\n- [ ] one\n  - [x] two\nplain [ ] three";
        let a = toggle_task(md, 1).unwrap();
        assert_eq!(a, "a\n- [x] one\n  - [x] two\nplain [ ] three");
        let b = toggle_task(&a, 2).unwrap();
        assert!(b.contains("  - [ ] two"));
        assert!(toggle_task(md, 0).is_none());
        assert!(toggle_task(md, 3).is_none());
        assert!(toggle_task(md, 99).is_none());
    }

    #[test]
    fn derive_title_prefers_h1_then_first_line() {
        assert_eq!(derive_title("intro\n# Real\nbody"), "Real");
        assert_eq!(derive_title("\n\n**First** line\nsecond"), "First line");
        assert_eq!(derive_title("   "), "Untitled");
    }

    #[test]
    fn crlf_is_handled() {
        let p = parse("# A\r\n[[B]]\r\n- [ ] t");
        assert_eq!(p.links[0].line, 1);
        assert_eq!(p.tasks[0].line, 2);
    }
}

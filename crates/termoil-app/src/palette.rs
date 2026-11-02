//! The command palette's list: every catalogue action with its summary and the keys bound to it,
//! filtered by a fuzzy match on the name and summary (`docs/TDD.md` section 4.8). The palette
//! window that shows the list is drawn by the front end.

use termoil_protocol::CATALOGUE;

use crate::config::Binding;

/// One palette row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// The action's catalogue name.
    pub action: &'static str,
    /// What it does.
    pub summary: &'static str,
    /// The keys bound to it, comma separated, or empty.
    pub keys: String,
    /// The action has required parameters, so the palette asks for them before running it.
    pub needs_args: bool,
    /// Arguments the entry runs its action with, as a JSON object, for entries that are an action
    /// with a fixed argument ("New window in its own process").
    pub preset: Option<&'static str>,
}

/// Every action, in catalogue order, with the bindings that run it.
pub fn entries(bindings: &[Binding]) -> Vec<Entry> {
    CATALOGUE
        .iter()
        .map(|a| {
            let keys: Vec<&str> = bindings
                .iter()
                .filter(|b| b.action == a.name && b.table.is_none() && b.args.is_empty())
                .map(|b| b.keys.as_str())
                .collect();
            Entry {
                action: a.name,
                summary: a.summary,
                keys: keys.join(", "),
                needs_args: a.params.iter().any(|p| p.required),
                preset: None,
            }
        })
        .chain(std::iter::once(Entry {
            action: "new-window",
            summary: "New window in its own process.",
            keys: String::new(),
            needs_args: false,
            preset: Some(r#"{"separate":true}"#),
        }))
        .collect()
}

/// The entries matching `query`, best first. Every character of the query must appear in order in
/// the action name or summary; matches in the name, at word starts and in runs score higher.
pub fn filter<'a>(entries: &'a [Entry], query: &str) -> Vec<&'a Entry> {
    let q: Vec<char> = query.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();
    if q.is_empty() {
        return entries.iter().collect();
    }
    let mut scored: Vec<(i32, usize, &Entry)> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let name = score(&q, e.action).map(|s| s + 100);
            let summary = score(&q, &e.summary.to_lowercase());
            name.max(summary).map(|s| (s, i, e))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, e)| e).collect()
}

/// A subsequence match score, or None when `q` is not a subsequence of `text`.
fn score(q: &[char], text: &str) -> Option<i32> {
    let mut total = 0;
    let mut qi = 0;
    let mut prev: Option<char> = None;
    let mut run = 0;
    for c in text.chars() {
        if qi < q.len() && c == q[qi] {
            run += 1;
            total += 1 + run * 2;
            if prev.is_none_or(|p| p == '-' || p == ' ') {
                total += 8;
            }
            qi += 1;
        } else {
            run = 0;
        }
        prev = Some(c);
    }
    (qi == q.len()).then_some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filters_by_subsequence_and_ranks_names_first() {
        let all = entries(&[Binding::new("ctrl+shift+t", "new-tab")]);
        let tab = all.iter().find(|e| e.action == "new-tab").unwrap();
        assert_eq!(tab.keys, "ctrl+shift+t");
        let found = filter(&all, "ntab");
        assert_eq!(found[0].action, "new-tab");
        assert!(filter(&all, "zzzz").is_empty());
        assert_eq!(filter(&all, "").len(), CATALOGUE.len() + 1);
        assert!(filter(&all, "own process").iter().any(|e| e.preset.is_some()));
    }
}

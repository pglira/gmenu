use memchr::memmem;
use rayon::prelude::*;

pub struct Items {
    pub raw: Vec<String>,
    pub lower: Vec<String>,
}

impl Items {
    pub fn new(raw: Vec<String>, case_sensitive: bool) -> Self {
        let lower = if case_sensitive {
            Vec::new()
        } else {
            raw.par_iter().map(|s| s.to_lowercase()).collect()
        };
        Self { raw, lower }
    }

    pub fn len(&self) -> usize {
        self.raw.len()
    }
}

pub struct Filter {
    pub query: String,
    pub matches: Vec<u32>,
    case_sensitive: bool,
    cap: usize,
}

impl Filter {
    pub fn new(case_sensitive: bool, cap: usize) -> Self {
        Self { query: String::new(), matches: Vec::new(), case_sensitive, cap }
    }

    /// Recompute `matches`. Splits `new_query` on whitespace; an item matches
    /// iff every token appears (substring) in the item. Ranking uses the
    /// earliest token match position. Uses incremental refinement when the
    /// new query extends the previous one (only re-scan previous matches).
    pub fn update(&mut self, items: &Items, new_query: &str) {
        let normalized: String = if self.case_sensitive {
            new_query.to_string()
        } else {
            new_query.to_lowercase()
        };
        let tokens: Vec<&str> = normalized.split_whitespace().collect();

        if tokens.is_empty() {
            let n = items.len().min(self.cap);
            self.matches = (0..n as u32).collect();
            self.query.clear();
            return;
        }

        let finders: Vec<memmem::Finder> =
            tokens.iter().map(|t| memmem::Finder::new(t.as_bytes())).collect();

        let haystacks: &[String] = if self.case_sensitive { &items.raw } else { &items.lower };
        let cap = self.cap;

        // Extends iff the new (normalized) query is a textual extension of the
        // old. With token-AND semantics this guarantees every old required
        // token still appears in some new token, so new matches ⊆ old matches.
        let extends = !self.query.is_empty() && normalized.starts_with(&self.query);

        let score = |bytes: &[u8]| -> Option<u32> {
            let mut min_pos = u32::MAX;
            for f in &finders {
                let p = f.find(bytes)?;
                if (p as u32) < min_pos { min_pos = p as u32; }
            }
            Some(min_pos)
        };

        let scored: Vec<(u32, u32)> = if extends {
            self.matches
                .par_iter()
                .copied()
                .filter_map(|i| score(haystacks[i as usize].as_bytes()).map(|p| (p, i)))
                .take_any(cap)
                .collect()
        } else {
            haystacks
                .par_iter()
                .enumerate()
                .filter_map(|(i, s)| score(s.as_bytes()).map(|p| (p, i as u32)))
                .take_any(cap)
                .collect()
        };

        let mut scored = scored;
        scored.sort_unstable_by_key(|&(pos, idx)| (pos, idx));
        let new_matches: Vec<u32> = scored.into_iter().map(|(_, i)| i).collect();

        self.matches = new_matches;
        self.query = normalized;
    }
}

/// Find all byte-offset spans in `haystack` that match any whitespace-split
/// token in `query`. Returned spans are sorted and merged (overlaps and
/// touches collapse into a single span). Empty query returns no spans.
pub fn match_spans(haystack: &str, query: &str, case_sensitive: bool) -> Vec<(usize, usize)> {
    if query.split_whitespace().next().is_none() {
        return Vec::new();
    }
    // For case-insensitive matching we scan against a lowercased copy of the
    // haystack but emit byte offsets that index into the original. This only
    // works when lowercasing is length-preserving (always true for ASCII;
    // breaks for some non-ASCII chars — give up on highlighting in that case).
    let lower_haystack;
    let scan: &str = if case_sensitive {
        haystack
    } else {
        lower_haystack = haystack.to_lowercase();
        if lower_haystack.len() != haystack.len() {
            return Vec::new();
        }
        &lower_haystack
    };
    let lower_query;
    let query_norm: &str = if case_sensitive {
        query
    } else {
        lower_query = query.to_lowercase();
        &lower_query
    };

    let mut spans: Vec<(usize, usize)> = Vec::new();
    for tok in query_norm.split_whitespace() {
        let finder = memmem::Finder::new(tok.as_bytes());
        let mut start = 0;
        while let Some(p) = finder.find(&scan.as_bytes()[start..]) {
            let s = start + p;
            let e = s + tok.len();
            spans.push((s, e));
            start = e;
        }
    }
    spans.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
    for (s, e) in spans {
        if let Some(last) = merged.last_mut() {
            if s <= last.1 {
                last.1 = last.1.max(e);
                continue;
            }
        }
        merged.push((s, e));
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_returns_all_capped() {
        let items = Items::new(vec!["a".into(), "b".into(), "c".into()], false);
        let mut f = Filter::new(false, 100);
        f.update(&items, "");
        assert_eq!(f.matches, vec![0, 1, 2]);
    }

    #[test]
    fn substring_case_insensitive() {
        let items = Items::new(vec!["FooBar".into(), "baz".into(), "qfoo".into()], false);
        let mut f = Filter::new(false, 100);
        f.update(&items, "foo");
        assert_eq!(f.matches, vec![0, 2]);
    }

    #[test]
    fn incremental_refines() {
        let items = Items::new(
            vec!["alpha".into(), "alphabet".into(), "beta".into(), "alps".into()],
            false,
        );
        let mut f = Filter::new(false, 100);
        f.update(&items, "al");
        assert_eq!(f.matches, vec![0, 1, 3]);
        f.update(&items, "alp");
        assert_eq!(f.matches, vec![0, 1, 3]);
        f.update(&items, "alph");
        assert_eq!(f.matches, vec![0, 1]);
    }

    #[test]
    fn prefix_matches_rank_first() {
        let items = Items::new(
            vec![
                "do-youtube".into(),  // match at pos 3
                "youtube".into(),     // match at pos 0
                "you-med".into(),     // match at pos 0
                "see-you".into(),     // match at pos 4
            ],
            false,
        );
        let mut f = Filter::new(false, 100);
        f.update(&items, "you");
        // youtube (1) and you-med (2) both at pos 0; tie-break by input
        // order, so 1 then 2. Then see-you (3) at pos 4, then do-youtube
        // (0) at pos 3.
        assert_eq!(f.matches, vec![1, 2, 0, 3]);
    }

    #[test]
    fn spans_basic() {
        assert_eq!(match_spans("FooBarFoo", "foo", false), vec![(0, 3), (6, 9)]);
        assert_eq!(match_spans("abc", "", false), Vec::<(usize,usize)>::new());
    }

    #[test]
    fn multitoken_and_match() {
        let items = Items::new(
            vec![
                "git-sla".into(),
                "git-only".into(),
                "do-git-sla".into(),
                "sla-only".into(),
            ],
            false,
        );
        let mut f = Filter::new(false, 100);
        f.update(&items, "git sla");
        // git-sla matches at pos 0; do-git-sla matches at pos 3 (token "git").
        assert_eq!(f.matches, vec![0, 2]);
    }

    #[test]
    fn multitoken_order_independent() {
        let items = Items::new(vec!["git-sla".into(), "sla-git".into()], false);
        let mut f = Filter::new(false, 100);
        f.update(&items, "sla git");
        assert_eq!(f.matches, vec![0, 1]);
    }

    #[test]
    fn multitoken_spans_merged() {
        // "git sla" against "git-sla" -> two adjacent spans (0,3) and (4,7)
        // (the dash sits between them so they don't merge).
        let s = match_spans("git-sla", "git sla", false);
        assert_eq!(s, vec![(0, 3), (4, 7)]);
        // Overlapping example: tokens "ab" and "bc" against "abc" -> merged.
        let s = match_spans("abc", "ab bc", false);
        assert_eq!(s, vec![(0, 3)]);
    }
}

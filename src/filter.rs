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

    /// Recompute `matches`. Uses incremental refinement when the new query
    /// extends the previous one (only re-scan previous matches).
    pub fn update(&mut self, items: &Items, new_query: &str) {
        if new_query.is_empty() {
            let n = items.len().min(self.cap);
            self.matches = (0..n as u32).collect();
            self.query.clear();
            return;
        }

        let needle_owned: String = if self.case_sensitive {
            new_query.to_string()
        } else {
            new_query.to_lowercase()
        };
        let finder = memmem::Finder::new(needle_owned.as_bytes());

        let haystacks: &[String] = if self.case_sensitive { &items.raw } else { &items.lower };
        let cap = self.cap;

        let extends = !self.query.is_empty()
            && needle_owned.len() >= self.query.len()
            && needle_owned.starts_with(&self.query);

        // Collect (match_position, item_index). Lower match_position = better
        // rank; original input order breaks ties.
        let scored: Vec<(u32, u32)> = if extends {
            self.matches
                .par_iter()
                .copied()
                .filter_map(|i| {
                    finder
                        .find(haystacks[i as usize].as_bytes())
                        .map(|p| (p as u32, i))
                })
                .take_any(cap)
                .collect()
        } else {
            haystacks
                .par_iter()
                .enumerate()
                .filter_map(|(i, s)| finder.find(s.as_bytes()).map(|p| (p as u32, i as u32)))
                .take_any(cap)
                .collect()
        };

        let mut scored = scored;
        scored.sort_unstable_by_key(|&(pos, idx)| (pos, idx));
        let new_matches: Vec<u32> = scored.into_iter().map(|(_, i)| i).collect();

        self.matches = new_matches;
        self.query = if self.case_sensitive { new_query.to_string() } else { new_query.to_lowercase() };
    }
}

/// Find all byte-offset spans in `haystack` that match `needle`.
/// Returned spans are non-overlapping. Empty needle returns no spans.
pub fn match_spans(haystack: &str, needle: &str, case_sensitive: bool) -> Vec<(usize, usize)> {
    if needle.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    if case_sensitive {
        let finder = memmem::Finder::new(needle.as_bytes());
        let mut start = 0;
        while let Some(p) = finder.find(&haystack.as_bytes()[start..]) {
            let s = start + p;
            let e = s + needle.len();
            out.push((s, e));
            start = e;
        }
    } else {
        // Match by walking lowercased copies. Byte positions match because
        // ASCII case mapping preserves length; for non-ASCII we fall back
        // to scanning the lowercased string and mapping byte indices via
        // the original char_indices. To keep the highlight in the *original*
        // bytes we scan the raw bytes after locating the offset in lower.
        let lh = haystack.to_lowercase();
        let ln = needle.to_lowercase();
        // Length-preserving check; if not, give up on highlighting.
        if lh.len() != haystack.len() {
            return Vec::new();
        }
        let finder = memmem::Finder::new(ln.as_bytes());
        let mut start = 0;
        while let Some(p) = finder.find(&lh.as_bytes()[start..]) {
            let s = start + p;
            let e = s + ln.len();
            out.push((s, e));
            start = e;
        }
    }
    out
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
}

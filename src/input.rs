/// Editable text input with caret + selection.
///
/// Byte offsets are used throughout; helpers ensure they always sit on
/// UTF-8 char boundaries.
pub struct Input {
    pub text: String,
    pub caret: usize,
    pub anchor: Option<usize>,
}

impl Input {
    pub fn new() -> Self {
        Self { text: String::new(), caret: 0, anchor: None }
    }

    pub fn selection(&self) -> Option<(usize, usize)> {
        self.anchor.map(|a| if a < self.caret { (a, self.caret) } else { (self.caret, a) })
    }

    pub fn clear_selection(&mut self) { self.anchor = None; }

    fn anchor_if_none(&mut self) {
        if self.anchor.is_none() { self.anchor = Some(self.caret); }
    }

    pub fn delete_selection(&mut self) -> bool {
        if let Some((s, e)) = self.selection() {
            self.text.replace_range(s..e, "");
            self.caret = s;
            self.anchor = None;
            true
        } else {
            false
        }
    }

    pub fn insert_str(&mut self, s: &str) {
        self.delete_selection();
        self.text.insert_str(self.caret, s);
        self.caret += s.len();
    }

    pub fn insert_char(&mut self, c: char) {
        let mut buf = [0u8; 4];
        let s = c.encode_utf8(&mut buf);
        self.insert_str(s);
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.caret = 0;
        self.anchor = None;
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.caret = self.text.len();
    }

    pub fn move_left(&mut self, extend: bool) {
        if extend { self.anchor_if_none(); } else { self.clear_selection(); }
        self.caret = prev_char_boundary(&self.text, self.caret);
    }
    pub fn move_right(&mut self, extend: bool) {
        if extend { self.anchor_if_none(); } else { self.clear_selection(); }
        self.caret = next_char_boundary(&self.text, self.caret);
    }
    pub fn move_word_left(&mut self, extend: bool) {
        if extend { self.anchor_if_none(); } else { self.clear_selection(); }
        self.caret = word_boundary_left(&self.text, self.caret);
    }
    pub fn move_word_right(&mut self, extend: bool) {
        if extend { self.anchor_if_none(); } else { self.clear_selection(); }
        self.caret = word_boundary_right(&self.text, self.caret);
    }
    pub fn move_home(&mut self, extend: bool) {
        if extend { self.anchor_if_none(); } else { self.clear_selection(); }
        self.caret = 0;
    }
    pub fn move_end(&mut self, extend: bool) {
        if extend { self.anchor_if_none(); } else { self.clear_selection(); }
        self.caret = self.text.len();
    }

    pub fn delete_left(&mut self, word: bool) {
        if self.delete_selection() { return; }
        let new = if word { word_boundary_left(&self.text, self.caret) }
                  else { prev_char_boundary(&self.text, self.caret) };
        if new < self.caret {
            self.text.replace_range(new..self.caret, "");
            self.caret = new;
        }
    }
    pub fn delete_right(&mut self, word: bool) {
        if self.delete_selection() { return; }
        let end = if word { word_boundary_right(&self.text, self.caret) }
                  else { next_char_boundary(&self.text, self.caret) };
        if end > self.caret {
            self.text.replace_range(self.caret..end, "");
        }
    }
}

fn prev_char_boundary(s: &str, mut i: usize) -> usize {
    if i == 0 { return 0; }
    i -= 1;
    while !s.is_char_boundary(i) { i -= 1; }
    i
}

fn next_char_boundary(s: &str, mut i: usize) -> usize {
    let n = s.len();
    if i >= n { return n; }
    i += 1;
    while i < n && !s.is_char_boundary(i) { i += 1; }
    i
}

fn is_word_char(c: char) -> bool { c.is_alphanumeric() || c == '_' }

/// Move caret left across one "word". First skips any non-word chars
/// (whitespace/punctuation), then skips word chars.
fn word_boundary_left(s: &str, i: usize) -> usize {
    let mut j = i;
    while j > 0 {
        let p = prev_char_boundary(s, j);
        let c = s[p..j].chars().next().unwrap();
        if is_word_char(c) { break; }
        j = p;
    }
    while j > 0 {
        let p = prev_char_boundary(s, j);
        let c = s[p..j].chars().next().unwrap();
        if !is_word_char(c) { break; }
        j = p;
    }
    j
}

fn word_boundary_right(s: &str, i: usize) -> usize {
    let n = s.len();
    let mut j = i;
    while j < n {
        let p = next_char_boundary(s, j);
        let c = s[j..p].chars().next().unwrap();
        if is_word_char(c) { break; }
        j = p;
    }
    while j < n {
        let p = next_char_boundary(s, j);
        let c = s[j..p].chars().next().unwrap();
        if !is_word_char(c) { break; }
        j = p;
    }
    j
}

#[cfg(test)]
mod tests {
    use super::*;
    fn s(text: &str, caret: usize) -> Input {
        Input { text: text.into(), caret, anchor: None }
    }
    #[test]
    fn insert_and_delete() {
        let mut i = s("hello", 5);
        i.insert_str(" world");
        assert_eq!(i.text, "hello world");
        assert_eq!(i.caret, 11);
        i.delete_left(true);
        assert_eq!(i.text, "hello ");
    }
    #[test]
    fn selection_replace() {
        let mut i = s("hello world", 0);
        i.move_right(false);
        i.move_right(true); i.move_right(true); i.move_right(true); i.move_right(true);
        assert_eq!(i.selection(), Some((1, 5)));
        i.insert_str("EY");
        assert_eq!(i.text, "hEY world");
        assert_eq!(i.caret, 3);
        assert!(i.anchor.is_none());
    }
    #[test]
    fn word_jumps() {
        let i_ = "  foo bar  baz";
        let mut i = s(i_, 0);
        i.move_word_right(false); assert_eq!(i.caret, 5);  // end of "foo"
        i.move_word_right(false); assert_eq!(i.caret, 9);  // end of "bar"
        i.move_word_left(false);  assert_eq!(i.caret, 6);  // start of "bar"
    }
    #[test]
    fn unicode_safe() {
        let mut i = s("éxçx", 0);
        i.move_right(false); // past 'é' (2 bytes)
        assert_eq!(i.caret, 2);
        i.delete_right(false);
        assert_eq!(i.text, "éçx");
    }
}

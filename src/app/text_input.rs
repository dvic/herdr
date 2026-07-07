use unicode_segmentation::UnicodeSegmentation;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct TextInputState {
    text: String,
    cursor: usize,
    replace_on_type: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WordClass {
    Word,
    Separator,
}

impl TextInputState {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn with_text(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            cursor: text.len(),
            text,
            replace_on_type: false,
        }
    }

    pub(crate) fn with_replace_on_type(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            cursor: text.len(),
            text,
            replace_on_type: true,
        }
    }

    pub(crate) fn text(&self) -> &str {
        &self.text
    }

    pub(crate) fn cursor(&self) -> usize {
        self.cursor
    }

    pub(crate) fn set_cursor(&mut self, cursor: usize) -> bool {
        self.disarm();
        let cursor = nearest_grapheme_boundary(&self.text, cursor);
        if self.cursor == cursor {
            return false;
        }
        self.cursor = cursor;
        true
    }

    #[cfg(test)]
    pub(crate) fn replace_on_type(&self) -> bool {
        self.replace_on_type
    }

    #[cfg(test)]
    pub(crate) fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor = self.text.len();
        self.resnap_cursor();
    }

    #[cfg(test)]
    pub(crate) fn set_replace_on_type(&mut self, replace_on_type: bool) {
        self.replace_on_type = replace_on_type;
    }

    pub(crate) fn clear(&mut self) -> bool {
        self.disarm();
        if self.text.is_empty() && self.cursor == 0 {
            return false;
        }
        self.text.clear();
        self.cursor = 0;
        true
    }

    pub(crate) fn insert_str(&mut self, text: &str) -> bool {
        let replace_on_type = self.replace_on_type;
        self.disarm();
        if replace_on_type {
            self.text.clear();
            self.cursor = 0;
        }

        let sanitized: String = text.chars().filter(|ch| !ch.is_control()).collect();
        if sanitized.is_empty() {
            self.resnap_cursor();
            return false;
        }

        self.text.insert_str(self.cursor, &sanitized);
        self.cursor += sanitized.len();
        self.resnap_cursor();
        true
    }

    pub(crate) fn backspace(&mut self) -> bool {
        if self.replace_on_type {
            return self.clear();
        }
        self.disarm();
        let Some(start) = self.previous_boundary(self.cursor) else {
            self.resnap_cursor();
            return false;
        };

        self.text.drain(start..self.cursor);
        self.cursor = start;
        self.resnap_cursor();
        true
    }

    pub(crate) fn delete_forward(&mut self) -> bool {
        if self.replace_on_type {
            return self.clear();
        }
        self.disarm();
        let Some(end) = self.next_boundary(self.cursor) else {
            self.resnap_cursor();
            return false;
        };

        self.text.drain(self.cursor..end);
        self.resnap_cursor();
        true
    }

    pub(crate) fn move_left(&mut self) -> bool {
        self.disarm();
        let Some(cursor) = self.previous_boundary(self.cursor) else {
            self.resnap_cursor();
            return false;
        };
        self.cursor = cursor;
        true
    }

    pub(crate) fn move_right(&mut self) -> bool {
        self.disarm();
        let Some(cursor) = self.next_boundary(self.cursor) else {
            self.resnap_cursor();
            return false;
        };
        self.cursor = cursor;
        true
    }

    pub(crate) fn move_start(&mut self) -> bool {
        self.disarm();
        if self.cursor == 0 {
            return false;
        }
        self.cursor = 0;
        true
    }

    pub(crate) fn move_end(&mut self) -> bool {
        self.disarm();
        if self.cursor == self.text.len() {
            return false;
        }
        self.cursor = self.text.len();
        true
    }

    pub(crate) fn move_word_left(&mut self) -> bool {
        self.disarm();
        let start = self.word_left_boundary();
        if start == self.cursor {
            return false;
        }
        self.cursor = start;
        true
    }

    pub(crate) fn move_word_right(&mut self) -> bool {
        self.disarm();
        let end = self.word_right_boundary();
        if end == self.cursor {
            return false;
        }
        self.cursor = end;
        true
    }

    pub(crate) fn delete_word_back(&mut self) -> bool {
        if self.replace_on_type {
            return self.clear();
        }
        self.disarm();
        let start = self.word_left_boundary();
        if start == self.cursor {
            return false;
        }
        self.text.drain(start..self.cursor);
        self.cursor = start;
        self.resnap_cursor();
        true
    }

    pub(crate) fn kill_to_start(&mut self) -> bool {
        self.disarm();
        if self.cursor == 0 {
            return false;
        }
        self.text.drain(..self.cursor);
        self.cursor = 0;
        self.resnap_cursor();
        true
    }

    pub(crate) fn kill_to_end(&mut self) -> bool {
        self.disarm();
        if self.cursor == self.text.len() {
            return false;
        }
        self.text.drain(self.cursor..);
        self.resnap_cursor();
        true
    }

    fn disarm(&mut self) {
        self.replace_on_type = false;
    }

    fn resnap_cursor(&mut self) {
        self.cursor = nearest_grapheme_boundary(&self.text, self.cursor);
    }

    fn previous_boundary(&self, cursor: usize) -> Option<usize> {
        self.boundaries()
            .into_iter()
            .rev()
            .find(|&idx| idx < cursor)
    }

    fn next_boundary(&self, cursor: usize) -> Option<usize> {
        self.boundaries().into_iter().find(|&idx| idx > cursor)
    }

    fn word_left_boundary(&self) -> usize {
        let mut cursor = self.cursor;
        while let Some((start, grapheme)) = self.grapheme_before(cursor) {
            if !grapheme.chars().all(char::is_whitespace) {
                break;
            }
            cursor = start;
        }

        let Some((_, grapheme)) = self.grapheme_before(cursor) else {
            return cursor;
        };
        let class = word_class(grapheme);
        while let Some((start, grapheme)) = self.grapheme_before(cursor) {
            if grapheme.chars().any(char::is_whitespace) || word_class(grapheme) != class {
                break;
            }
            cursor = start;
        }
        cursor
    }

    fn word_right_boundary(&self) -> usize {
        let mut cursor = self.cursor;
        while let Some((end, grapheme)) = self.grapheme_at(cursor) {
            if !grapheme.chars().all(char::is_whitespace) {
                break;
            }
            cursor = end;
        }

        let Some((_, grapheme)) = self.grapheme_at(cursor) else {
            return cursor;
        };
        let class = word_class(grapheme);
        while let Some((end, grapheme)) = self.grapheme_at(cursor) {
            if grapheme.chars().any(char::is_whitespace) || word_class(grapheme) != class {
                break;
            }
            cursor = end;
        }
        cursor
    }

    fn grapheme_before(&self, cursor: usize) -> Option<(usize, &str)> {
        self.text[..cursor].grapheme_indices(true).next_back()
    }

    fn grapheme_at(&self, cursor: usize) -> Option<(usize, &str)> {
        self.text[cursor..]
            .grapheme_indices(true)
            .next()
            .map(|(relative, grapheme)| (cursor + relative + grapheme.len(), grapheme))
    }

    fn boundaries(&self) -> Vec<usize> {
        grapheme_boundaries(&self.text)
    }
}

impl From<&str> for TextInputState {
    fn from(text: &str) -> Self {
        Self::with_text(text)
    }
}

impl From<String> for TextInputState {
    fn from(text: String) -> Self {
        Self::with_text(text)
    }
}

impl std::fmt::Display for TextInputState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.text.fmt(f)
    }
}

impl std::ops::Deref for TextInputState {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.text()
    }
}

impl PartialEq<&str> for TextInputState {
    fn eq(&self, other: &&str) -> bool {
        self.text() == *other
    }
}

fn word_class(grapheme: &str) -> WordClass {
    let first = grapheme.chars().next();
    if first.is_some_and(|ch| ch.is_alphanumeric() || ch == '_') {
        WordClass::Word
    } else {
        WordClass::Separator
    }
}

fn grapheme_boundaries(text: &str) -> Vec<usize> {
    let mut boundaries: Vec<usize> = text.grapheme_indices(true).map(|(idx, _)| idx).collect();
    boundaries.push(text.len());
    boundaries
}

fn nearest_grapheme_boundary(text: &str, cursor: usize) -> usize {
    let cursor = cursor.min(text.len());
    if text.is_char_boundary(cursor) && text.is_grapheme_boundary(cursor) {
        return cursor;
    }

    let mut previous = 0;
    let mut next = text.len();
    for boundary in grapheme_boundaries(text) {
        if boundary <= cursor {
            previous = boundary;
        } else {
            next = boundary;
            break;
        }
    }

    if cursor - previous <= next - cursor {
        previous
    } else {
        next
    }
}

trait GraphemeBoundary {
    fn is_grapheme_boundary(&self, idx: usize) -> bool;
}

impl GraphemeBoundary for str {
    fn is_grapheme_boundary(&self, idx: usize) -> bool {
        grapheme_boundaries(self)
            .into_iter()
            .any(|boundary| boundary == idx)
    }
}

#[cfg(test)]
mod tests {
    use super::TextInputState;

    #[test]
    fn insert_replaces_when_armed_and_sanitizes_controls() {
        let mut input = TextInputState::with_replace_on_type("generated");

        assert!(input.insert_str("feature\n\tbranch\u{7}/x"));

        assert_eq!(input.text(), "featurebranch/x");
        assert_eq!(input.cursor(), "featurebranch/x".len());
        assert!(!input.replace_on_type());
    }

    #[test]
    fn backspace_and_delete_forward_are_grapheme_aware() {
        let mut input = TextInputState::with_text("a👩‍💻界e\u{301}z");

        assert!(input.move_left());
        assert!(input.backspace());
        assert_eq!(input.text(), "a👩‍💻界z");
        assert_eq!(input.cursor(), "a👩‍💻界".len());

        input.move_start();
        input.move_right();
        assert!(input.delete_forward());
        assert_eq!(input.text(), "a界z");
        assert_eq!(input.cursor(), "a".len());
    }

    #[test]
    fn word_movement_and_delete_use_readline_style_classes() {
        let mut input = TextInputState::with_text("alpha-beta  gamma_delta");

        assert!(input.delete_word_back());
        assert_eq!(input.text(), "alpha-beta  ");
        assert_eq!(input.cursor(), "alpha-beta  ".len());

        input.set_text("alpha-beta  gamma_delta");

        assert!(input.move_word_left());
        assert_eq!(input.cursor(), "alpha-beta  ".len());

        assert!(input.delete_word_back());
        assert_eq!(input.text(), "alpha-gamma_delta");
        assert_eq!(input.cursor(), "alpha-".len());

        input.set_text("alpha-beta  gamma_delta");
        assert!(input.move_word_left());
        assert_eq!(input.cursor(), "alpha-beta  ".len());
        assert!(input.move_word_left());
        assert_eq!(input.cursor(), "alpha-".len());

        assert!(input.move_word_right());
        assert_eq!(input.cursor(), "alpha-beta".len());
    }

    #[test]
    fn kill_to_start_and_end_follow_cursor() {
        let mut input = TextInputState::with_text("prefix middle suffix");
        input.move_word_left();

        assert!(input.kill_to_start());
        assert_eq!(input.text(), "suffix");
        assert_eq!(input.cursor(), 0);

        assert!(input.move_end());
        input.move_word_left();
        assert!(input.kill_to_end());
        assert_eq!(input.text(), "");
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn movement_disarms_replace_on_type_without_editing() {
        let mut input = TextInputState::with_replace_on_type("prefill");

        assert!(input.move_left());

        assert_eq!(input.text(), "prefill");
        assert_eq!(input.cursor(), "prefil".len());
        assert!(!input.replace_on_type());
    }

    #[test]
    fn insertion_resnaps_cursor_when_text_merges_graphemes() {
        let mut input = TextInputState::with_text("👩💻");
        input.move_left();

        assert!(input.insert_str("\u{200d}"));

        assert_eq!(input.text(), "👩‍💻");
        assert_eq!(input.cursor(), "👩‍💻".len());

        input.move_start();
        assert!(input.insert_str("e"));
        assert!(input.insert_str("\u{301}"));
        assert_eq!(input.text(), "e\u{301}👩‍💻");
        assert_eq!(input.cursor(), "e\u{301}".len());
    }
}

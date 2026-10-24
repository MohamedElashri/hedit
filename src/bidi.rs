//! Right-to-left text: Arabic letter joining and bidirectional reordering.
//!
//! The text itself always stays in logical (typing) order; these functions
//! only decide how each line is drawn. Joining picks each Arabic letter's
//! contextual form from the Unicode presentation forms, so fonts without
//! OpenType shaping still connect letters, and reordering follows the Unicode
//! Bidirectional Algorithm with each line as its own paragraph.
use unicode_bidi::{Level, ParagraphBidiInfo};

/// Whether `c` belongs to a right-to-left script (Hebrew, Arabic, Syriac,
/// Thaana, N'Ko, and their presentation forms and extensions).
pub fn is_rtl(c: char) -> bool {
    matches!(c as u32,
        0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF)
}

/// Whether `text` needs bidirectional handling at all.
pub fn has_rtl(text: &str) -> bool {
    // Every right-to-left character is at least two bytes; skip ASCII quickly.
    !text.is_ascii() && text.chars().any(is_rtl)
}

/// How a letter connects to its neighbours.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Joining {
    /// Joins on both sides: isolated, final, initial, and medial forms.
    Dual,
    /// Joins only to the letter before it: isolated and final forms.
    Right,
    /// Joins on both sides without changing shape (tatweel, zero-width joiner).
    Causing,
    /// Does not affect joining (combining marks such as harakat).
    Transparent,
    None,
}

/// Arabic letters U+0621–U+064A in order, with how many presentation forms
/// each has; their forms follow one another from U+FE80.
const BASIC: [u8; 42] = [
    1, 2, 2, 2, 2, 4, 2, 4, 2, 4, 4, 4, 4, 4, 2, 2, 2, 2, 4, 4, 4, 4, 4, 4, 4,
    4, // 0621–063A
    0, 0, 0, 0, 0, 0, // 063B–0640: not in the table (0640 is tatweel)
    4, 4, 4, 4, 4, 4, 4, 2, 2, 4, // 0641–064A
];

/// Persian, Urdu, and other extended letters with their first presentation
/// form (isolated) in Arabic Presentation Forms-A, and their form count.
const EXTENDED: &[(char, u32, u8)] = &[
    ('\u{0671}', 0xFB50, 2), // alef wasla
    ('\u{0679}', 0xFB66, 4), // tteh
    ('\u{067E}', 0xFB56, 4), // peh
    ('\u{0686}', 0xFB7A, 4), // tcheh
    ('\u{0688}', 0xFB88, 2), // ddal
    ('\u{0691}', 0xFB8C, 2), // rreh
    ('\u{0698}', 0xFB8A, 2), // jeh
    ('\u{06A4}', 0xFB6A, 4), // veh
    ('\u{06A9}', 0xFB8E, 4), // keheh
    ('\u{06AF}', 0xFB92, 4), // gaf
    ('\u{06BA}', 0xFB9E, 2), // noon ghunna
    ('\u{06BE}', 0xFBAA, 4), // heh doachashmee
    ('\u{06C1}', 0xFBA6, 4), // heh goal
    ('\u{06CC}', 0xFBFC, 4), // farsi yeh
    ('\u{06D2}', 0xFBAE, 2), // yeh barree
];

/// The first presentation form of `c` and how many forms it has.
fn forms(c: char) -> Option<(u32, u8)> {
    let code = c as u32;
    if (0x0621..=0x064A).contains(&code) {
        let index = (code - 0x0621) as usize;
        let count = BASIC[index];
        if count == 0 {
            return None;
        }
        let first = 0xFE80 + BASIC[..index].iter().map(|&n| n as u32).sum::<u32>();
        return Some((first, count));
    }
    EXTENDED
        .iter()
        .find(|(base, _, _)| *base == c)
        .map(|&(_, first, count)| (first, count))
}

fn joining(c: char) -> Joining {
    match c as u32 {
        0x0640 | 0x200D => Joining::Causing,
        0x0610..=0x061A
        | 0x064B..=0x065F
        | 0x0670
        | 0x06D6..=0x06DC
        | 0x06DF..=0x06E4
        | 0x06E7
        | 0x06E8
        | 0x06EA..=0x06ED => Joining::Transparent,
        _ => match forms(c) {
            Some((_, 4)) => Joining::Dual,
            Some((_, 2)) => Joining::Right,
            _ => Joining::None,
        },
    }
}

/// How one character is drawn after joining.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shaped {
    /// The character whose glyph is drawn.
    pub glyph: char,
    /// Drawn as part of the previous character's ligature (the alef of lam-alef).
    pub hidden: bool,
}

/// The lam-alef ligature for an alef variant, isolated form first.
fn lam_alef(alef: char) -> Option<u32> {
    match alef {
        '\u{0622}' => Some(0xFEF5),
        '\u{0623}' => Some(0xFEF7),
        '\u{0625}' => Some(0xFEF9),
        '\u{0627}' => Some(0xFEFB),
        _ => None,
    }
}

/// Pick each character's contextual form; one entry per character.
pub fn shape(chars: &[char]) -> Vec<Shaped> {
    let mut shaped: Vec<Shaped> = chars
        .iter()
        .map(|&glyph| Shaped {
            glyph,
            hidden: false,
        })
        .collect();
    if !chars.iter().any(|&c| forms(c).is_some()) {
        return shaped;
    }
    let kinds: Vec<Joining> = chars.iter().map(|&c| joining(c)).collect();
    // The nearest non-transparent neighbours of each character.
    let neighbour = |i: usize, step: isize| -> Option<usize> {
        let mut j = i as isize + step;
        while j >= 0 && (j as usize) < chars.len() {
            if kinds[j as usize] != Joining::Transparent {
                return Some(j as usize);
            }
            j += step;
        }
        None
    };
    let mut i = 0;
    while i < chars.len() {
        let kind = kinds[i];
        if !matches!(kind, Joining::Dual | Joining::Right) {
            i += 1;
            continue;
        }
        let joins_before = neighbour(i, -1)
            .is_some_and(|p| matches!(kinds[p], Joining::Dual | Joining::Causing))
            // A lam-alef ligature ends a word's joining like a right-joining letter.
            && !neighbour(i, -1).is_some_and(|p| shaped[p].hidden);
        let next = neighbour(i, 1);
        if chars[i] == '\u{0644}' {
            if let Some((n, ligature)) = next.and_then(|n| lam_alef(chars[n]).map(|l| (n, l))) {
                let form = ligature + joins_before as u32;
                shaped[i].glyph = char::from_u32(form).unwrap_or(chars[i]);
                shaped[n].hidden = true;
                i = n + 1;
                continue;
            }
        }
        let joins_after = kind == Joining::Dual
            && next.is_some_and(|n| {
                matches!(kinds[n], Joining::Dual | Joining::Right | Joining::Causing)
            });
        let (first, count) = forms(chars[i]).unwrap();
        let offset = match (joins_before, joins_after, count) {
            (true, true, 4) => 3,
            (false, true, 4) => 2,
            (true, _, _) => 1,
            _ => 0,
        };
        shaped[i].glyph = char::from_u32(first + offset).unwrap_or(chars[i]);
        i += 1;
    }
    shaped
}

/// The mirror image of a paired bracket, drawn inside right-to-left runs.
pub fn mirror(c: char) -> Option<char> {
    Some(match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        '‹' => '›',
        '›' => '‹',
        _ => return None,
    })
}

/// Bidirectional levels for one line of text.
pub struct Line {
    text: String,
    /// A forced base level, or `None` to take it from the text.
    base: Option<Level>,
    /// The byte offset of each character, plus the line length.
    bytes: Vec<usize>,
    base_rtl: bool,
}

impl Line {
    /// Resolve the levels of `text`, one line with no newline, taking its base
    /// direction from its first strong character.
    pub fn new(text: &str) -> Self {
        Self::with_base(text, None)
    }
    /// Like [`Line::new`], with the base direction forced when `rtl` is given.
    pub fn with_base(text: &str, rtl: Option<bool>) -> Self {
        let mut bytes: Vec<usize> = text.char_indices().map(|(b, _)| b).collect();
        bytes.push(text.len());
        let base = rtl.map(|rtl| if rtl { Level::rtl() } else { Level::ltr() });
        let base_rtl = ParagraphBidiInfo::new(text, base).paragraph_level.is_rtl();
        Self {
            text: text.to_owned(),
            base,
            bytes,
            base_rtl,
        }
    }
    fn chars(&self) -> usize {
        self.bytes.len() - 1
    }
    /// Levels of every character, with rule L1 applied to the row `start..end`.
    fn levels(&self, start: usize, end: usize) -> Vec<Level> {
        ParagraphBidiInfo::new(&self.text, self.base)
            .reordered_levels_per_char(self.bytes[start]..self.bytes[end])
    }
    /// Whether the line reads right to left overall.
    pub fn base_rtl(&self) -> bool {
        self.base_rtl
    }
    /// Whether the character at `index` is drawn right to left.
    pub fn rtl(&self, index: usize) -> bool {
        index < self.chars() && self.levels(0, self.chars())[index].is_rtl()
    }
    /// The characters `start..end` (one wrapped row) in drawing order, left to
    /// right, as indices into the line.
    pub fn visual(&self, start: usize, end: usize) -> Vec<usize> {
        let (start, end) = (start.min(self.chars()), end.min(self.chars()));
        ParagraphBidiInfo::reorder_visual(&self.levels(start, end)[start..end])
            .into_iter()
            .map(|i| i + start)
            .collect()
    }
    /// Directions of the characters `start..end`, one wrapped row.
    pub fn row_rtl(&self, start: usize, end: usize) -> Vec<bool> {
        let (start, end) = (start.min(self.chars()), end.min(self.chars()));
        self.levels(start, end)[start..end]
            .iter()
            .map(Level::is_rtl)
            .collect()
    }
}

/// `text` as it should appear in a label of the left-to-right interface:
/// letters joined and each line in visual order, read with a left-to-right
/// base like a file name. For display only, never editing.
pub fn display(text: &str) -> std::borrow::Cow<'_, str> {
    if !has_rtl(text) {
        return text.into();
    }
    let lines: Vec<String> = text
        .split('\n')
        .map(|line| {
            let chars: Vec<char> = line.chars().collect();
            if !chars.iter().copied().any(is_rtl) {
                return line.to_owned();
            }
            let shaped = shape(&chars);
            let bidi = Line::with_base(line, Some(false));
            let rtl = bidi.row_rtl(0, chars.len());
            bidi.visual(0, chars.len())
                .into_iter()
                .filter(|&i| !shaped[i].hidden)
                .map(|i| {
                    let glyph = shaped[i].glyph;
                    if rtl[i] {
                        mirror(glyph).unwrap_or(glyph)
                    } else {
                        glyph
                    }
                })
                .collect()
        })
        .collect();
    lines.join("\n").into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glyphs(text: &str) -> String {
        let chars: Vec<char> = text.chars().collect();
        shape(&chars)
            .into_iter()
            .filter(|s| !s.hidden)
            .map(|s| s.glyph)
            .collect()
    }

    #[test]
    fn letters_take_their_contextual_forms() {
        // بيت: beh initial, yeh medial, teh final.
        assert_eq!(glyphs("بيت"), "\u{FE91}\u{FEF4}\u{FE96}");
        // A lone letter stays isolated; dal never joins the next letter.
        assert_eq!(glyphs("ب"), "\u{FE8F}");
        assert_eq!(glyphs("دب"), "\u{FEA9}\u{FE8F}");
        // Harakat do not break joining.
        assert_eq!(glyphs("بَت"), "\u{FE91}\u{064E}\u{FE96}");
        // Persian peh and gaf.
        assert_eq!(glyphs("پگ"), "\u{FB58}\u{FB93}");
        // Non-Arabic text is untouched.
        assert_eq!(glyphs("abc"), "abc");
    }

    #[test]
    fn lam_alef_becomes_one_ligature() {
        let chars: Vec<char> = "سلام".chars().collect();
        let shaped = shape(&chars);
        assert_eq!(shaped.len(), 4);
        assert_eq!(shaped[1].glyph, '\u{FEFC}'); // final lam-alef after seen
        assert!(shaped[2].hidden);
        assert_eq!(glyphs("لا"), "\u{FEFB}");
        assert_eq!(glyphs("لأ"), "\u{FEF7}");
        // The letter after a lam-alef starts a new joining group.
        assert_eq!(glyphs("لام"), "\u{FEFB}\u{FEE1}");
    }

    #[test]
    fn labels_show_joined_letters_in_visual_order() {
        assert_eq!(display("notes.txt"), "notes.txt");
        // بيت.txt: the word reads right to left, joined, before the extension.
        assert_eq!(display("بيت.txt"), "\u{FE96}\u{FEF4}\u{FE91}.txt");
        assert_eq!(display("(لا)"), "(\u{FEFB})");
    }

    #[test]
    fn mixed_lines_reorder_runs() {
        // An English line with an Arabic word: the word reads right to left.
        let line = Line::new("say مرحبا now");
        assert!(!line.base_rtl());
        let order = line.visual(0, 13);
        assert_eq!(&order[..4], &[0, 1, 2, 3]);
        assert_eq!(&order[4..9], &[8, 7, 6, 5, 4]);
        assert_eq!(&order[9..], &[9, 10, 11, 12]);
        // An Arabic line with an English word and a number keeps them left to right.
        let line = Line::new("عدد 42 hello");
        assert!(line.base_rtl());
        let order = line.visual(0, 12);
        assert_eq!(&order[..5], &[7, 8, 9, 10, 11]);
        assert_eq!(&order[5..], &[6, 4, 5, 3, 2, 1, 0]);
        assert!(line.rtl(0) && !line.rtl(4) && !line.rtl(7));
    }
}

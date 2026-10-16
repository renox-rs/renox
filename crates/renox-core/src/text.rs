//! Text helpers for Arabic and for matching names across scripts. No
//! dependencies, no I/O.
//!
//! - [`normalize_arabic`] makes spelling variants of the same Arabic text
//!   equal: it strips the marks (harakat, shadda, tatweel), unifies the alef,
//!   ya and ta marbuta forms, folds the Persian ک and ی, and turns Arabic-Indic
//!   and Persian digits into ASCII digits.
//! - [`transliterate`] writes Arabic in Latin letters as a consonant
//!   skeleton: for matching, not for display.
//! - [`match_key`] gives the same key to a name however it is spelled, in
//!   Arabic or in Latin letters ("محمد", "Mohammed", "Muhammad").
//!
//! Full-text search uses the same normalisation in the database:
//! `#[model(search = "name", normalize = "arabic")]`, see
//! [`renox::db::search`](crate::db::search) and docs/search.md.
//!
//! ```
//! use renox::text::{match_key, normalize_arabic, transliterate};
//!
//! assert_eq!(normalize_arabic("أَحْمَد"), "احمد");
//! assert_eq!(transliterate("محمد"), "mhmd");
//! assert_eq!(match_key("Mohammed"), match_key("محمد"));
//! ```

/// How `normalize_arabic` changes one character: replaced by another, or
/// deleted. The one table behind the Rust function and the SQL that
/// `db::search` builds, so the two cannot drift apart.
#[derive(Clone, Copy)]
pub(crate) enum Rule {
    /// The character becomes this one.
    Map(char, char),
    /// The character is removed.
    Delete(char),
}

/// Every normalisation rule, in one place.
pub(crate) const RULES: &[Rule] = &[
    // Marks: fathatan, dammatan, kasratan, fatha, damma, kasra, shadda, sukun.
    Rule::Delete('\u{064B}'),
    Rule::Delete('\u{064C}'),
    Rule::Delete('\u{064D}'),
    Rule::Delete('\u{064E}'),
    Rule::Delete('\u{064F}'),
    Rule::Delete('\u{0650}'),
    Rule::Delete('\u{0651}'),
    Rule::Delete('\u{0652}'),
    // Superscript alef, tatweel (kashida).
    Rule::Delete('\u{0670}'),
    Rule::Delete('\u{0640}'),
    // Alef with hamza above or below, alef with madda, alef wasla: ا
    Rule::Map('\u{0623}', '\u{0627}'),
    Rule::Map('\u{0625}', '\u{0627}'),
    Rule::Map('\u{0622}', '\u{0627}'),
    Rule::Map('\u{0671}', '\u{0627}'),
    // Alef maqsura: ي. Ta marbuta: ه.
    Rule::Map('\u{0649}', '\u{064A}'),
    Rule::Map('\u{0629}', '\u{0647}'),
    // Persian kaf and ya: ك and ي.
    Rule::Map('\u{06A9}', '\u{0643}'),
    Rule::Map('\u{06CC}', '\u{064A}'),
    // Arabic-Indic digits ٠-٩ and Persian digits ۰-۹: 0-9.
    Rule::Map('\u{0660}', '0'),
    Rule::Map('\u{0661}', '1'),
    Rule::Map('\u{0662}', '2'),
    Rule::Map('\u{0663}', '3'),
    Rule::Map('\u{0664}', '4'),
    Rule::Map('\u{0665}', '5'),
    Rule::Map('\u{0666}', '6'),
    Rule::Map('\u{0667}', '7'),
    Rule::Map('\u{0668}', '8'),
    Rule::Map('\u{0669}', '9'),
    Rule::Map('\u{06F0}', '0'),
    Rule::Map('\u{06F1}', '1'),
    Rule::Map('\u{06F2}', '2'),
    Rule::Map('\u{06F3}', '3'),
    Rule::Map('\u{06F4}', '4'),
    Rule::Map('\u{06F5}', '5'),
    Rule::Map('\u{06F6}', '6'),
    Rule::Map('\u{06F7}', '7'),
    Rule::Map('\u{06F8}', '8'),
    Rule::Map('\u{06F9}', '9'),
];

/// Makes spelling variants of Arabic text equal, so that a search or a
/// comparison ignores them:
///
/// - the marks are removed: harakat (fatha, damma, kasra, their tanwin,
///   shadda, sukun), the superscript alef and the tatweel;
/// - أ إ آ ٱ become ا;
/// - ى becomes ي, and ة becomes ه;
/// - the Persian ک and ی become ك and ي;
/// - Arabic-Indic (٠-٩) and Persian (۰-۹) digits become 0-9.
///
/// Everything else, Latin text included, is left as it is (case too).
///
/// ```
/// use renox::text::normalize_arabic;
///
/// assert_eq!(normalize_arabic("أَحْمَد"), "احمد");
/// assert_eq!(normalize_arabic("مدرسة"), "مدرسه");
/// assert_eq!(normalize_arabic("هاتف ٠١٢٣"), "هاتف 0123");
/// assert_eq!(normalize_arabic("Café"), "Café");
/// ```
pub fn normalize_arabic(text: &str) -> String {
    text.chars()
        .filter_map(|c| {
            for rule in RULES {
                match *rule {
                    Rule::Delete(d) if d == c => return None,
                    Rule::Map(from, to) if from == c => return Some(to),
                    _ => {}
                }
            }
            Some(c)
        })
        .collect()
}

/// The Latin letters for an Arabic letter in `transliterate`: a consonant
/// skeleton, so the letters that are vowels or glottal (ا و ي ع ء) give
/// nothing. `None` for a character that is not an Arabic letter.
fn arabic_letter(c: char) -> Option<&'static str> {
    Some(match c {
        'ا' | 'ء' | 'ؤ' | 'ئ' | 'ع' | 'و' | 'ي' => "",
        'ب' | 'پ' => "b",
        'ت' | 'ط' => "t",
        'ث' => "th",
        'ج' => "j",
        'ح' | 'ه' => "h",
        'خ' => "kh",
        'د' | 'ض' => "d",
        'ذ' | 'ظ' => "dh",
        'ر' => "r",
        'ز' => "z",
        'س' | 'ص' => "s",
        'ش' => "sh",
        'غ' => "gh",
        'ف' => "f",
        'ق' => "q",
        'ك' => "k",
        'ل' => "l",
        'م' => "m",
        'ن' => "n",
        'چ' => "ch",
        'گ' => "g",
        'ژ' => "zh",
        'ڤ' => "v",
        _ => return None,
    })
}

/// Writes Arabic in Latin letters as a consonant skeleton in lower case:
/// "محمد" becomes "mhmd". The vowel letters (ا و ي) and ع, ء give nothing,
/// since Arabic leaves short vowels unwritten. It is meant for matching
/// (see [`match_key`]), not for display. The text is [normalised](normalize_arabic)
/// first; characters that are not Arabic letters pass through, lower-cased.
///
/// ```
/// use renox::text::transliterate;
///
/// assert_eq!(transliterate("محمد"), "mhmd");
/// assert_eq!(transliterate("خالد بن سعيد"), "khld bn sd");
/// assert_eq!(transliterate("Ali"), "ali");
/// ```
pub fn transliterate(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in normalize_arabic(text).chars() {
        match arabic_letter(c) {
            Some(latin) => out.push_str(latin),
            None => out.extend(c.to_lowercase()),
        }
    }
    out
}

/// The base letter of a common accented Latin letter.
fn unaccent(c: char) -> char {
    match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => 'a',
        'è' | 'é' | 'ê' | 'ë' => 'e',
        'ì' | 'í' | 'î' | 'ï' => 'i',
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' => 'o',
        'ù' | 'ú' | 'û' | 'ü' => 'u',
        'ç' => 'c',
        'ñ' => 'n',
        other => other,
    }
}

/// The key of one word of lower-case Latin letters and digits: digraphs
/// folded (sh, ch, kh, gh, th, dh, ph, zh), vowels and y/w dropped, letters
/// that spellings swap (k q g c, b p, f v) united, doubled letters made
/// single, and a final h dropped (Fatima, Fatimah).
fn word_key(word: &str) -> String {
    let letters: Vec<char> = word.chars().map(unaccent).collect();
    let mut key = String::new();
    let mut push = |c: char| {
        if !key.ends_with(c) {
            key.push(c);
        }
    };
    let mut i = 0;
    while i < letters.len() {
        let c = letters[i];
        let next = letters.get(i + 1).copied();
        if next == Some('h') {
            let digraph = match c {
                's' | 'c' => Some('x'),
                'k' | 'g' => Some('k'),
                't' => Some('t'),
                'd' => Some('z'),
                'p' => Some('f'),
                'z' => Some('j'),
                _ => None,
            };
            if let Some(d) = digraph {
                push(d);
                i += 2;
                continue;
            }
        }
        match c {
            'a' | 'e' | 'i' | 'o' | 'u' | 'y' | 'w' => {}
            'c' | 'q' | 'g' => push('k'),
            'p' => push('b'),
            'v' => push('f'),
            other => push(other),
        }
        i += 1;
    }
    if key.chars().count() > 1 && key.ends_with('h') {
        key.pop();
    }
    key
}

/// A key that is the same for the spellings of a name, whether written in
/// Arabic or in Latin letters: "محمد", "Mohammed", "Muhammad" and "Mohamed" all
/// give "mhmd". The text is [transliterated](transliterate), then each word
/// loses its vowels, common digraphs (sh, kh, th, …) and look-alike
/// consonants (k/q/g, b/p, f/v) are united, and doubled letters become
/// single. Spaces and punctuation are left out, so "Abd Allah", "Abd al-Rahman"
/// and "Abdullah" or "Abdulrahman" agree however they are split.
///
/// The key is for finding candidates ("possible duplicate" checks, a
/// search-as-you-type on names), never for display, and different names can
/// share one. Store it in an indexed column and compare:
///
/// ```
/// use renox::text::match_key;
///
/// for spelling in ["محمد", "Mohammed", "Muhammad", "Mohamed"] {
///     assert_eq!(match_key(spelling), "mhmd");
/// }
/// assert_eq!(match_key("Abd al-Rahman"), match_key("عبد الرحمن"));
/// assert_eq!(match_key("Fatima"), match_key("فاطمة"));
/// ```
pub fn match_key(text: &str) -> String {
    let skeleton = transliterate(text);
    let mut key = String::new();
    for word in skeleton.split(|c: char| !c.is_alphanumeric()) {
        for c in word_key(word).chars() {
            if !key.ends_with(c) {
                key.push(c);
            }
        }
    }
    key
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_normalisation_rule() {
        // Harakat, tanwin, shadda, sukun, superscript alef.
        assert_eq!(normalize_arabic("مُحَمَّدٌ"), "محمد");
        assert_eq!(normalize_arabic("كِتَابًا"), "كتابا");
        assert_eq!(normalize_arabic("كِتَابٍ"), "كتاب");
        assert_eq!(normalize_arabic("مَنْ"), "من");
        assert_eq!(normalize_arabic("هٰذا"), "هذا");
        // Tatweel.
        assert_eq!(normalize_arabic("محـــمد"), "محمد");
        // Alef forms.
        assert_eq!(
            normalize_arabic("أحمد إبراهيم آمنة ٱلله"),
            "احمد ابراهيم امنه الله"
        );
        // Alef maqsura and ta marbuta.
        assert_eq!(normalize_arabic("مصطفى"), "مصطفي");
        assert_eq!(normalize_arabic("فاطمة"), "فاطمه");
        // Persian letters.
        assert_eq!(normalize_arabic("کتاب فارسی"), "كتاب فارسي");
        // Digits.
        assert_eq!(normalize_arabic("٠١٢٣٤٥٦٧٨٩"), "0123456789");
        assert_eq!(normalize_arabic("۰۱۲۳۴۵۶۷۸۹"), "0123456789");
        // Latin text, other scripts and an empty text are left alone.
        assert_eq!(normalize_arabic("Café 東京 42"), "Café 東京 42");
        assert_eq!(normalize_arabic(""), "");
        // Idempotent.
        let once = normalize_arabic("أَحْمَدُ ٣");
        assert_eq!(normalize_arabic(&once), once);
    }

    #[test]
    fn transliteration_is_a_consonant_skeleton() {
        assert_eq!(transliterate("محمد"), "mhmd");
        assert_eq!(transliterate("أَحْمَد"), "hmd");
        assert_eq!(transliterate("خالد"), "khld");
        assert_eq!(transliterate("شريف"), "shrf");
        assert_eq!(transliterate("Ali 12"), "ali 12");
        assert_eq!(transliterate("رقم ٧"), "rqm 7");
    }

    #[test]
    fn names_get_one_key_in_both_scripts() {
        let names: &[(&str, &[&str])] = &[
            ("محمد", &["Mohammed", "Muhammad", "Mohamed", "Mohammad"]),
            ("أحمد", &["Ahmad", "Ahmed"]),
            ("علي", &["Ali", "Aly"]),
            ("عمر", &["Omar", "Umar"]),
            ("فاطمة", &["Fatima", "Fatimah"]),
            ("حسن", &["Hassan", "Hasan"]),
            ("خالد", &["Khalid", "Khaled"]),
            ("يوسف", &["Yusuf", "Youssef", "Yousef"]),
            ("إبراهيم", &["Ibrahim", "Ebrahim"]),
            ("زينب", &["Zainab", "Zaynab"]),
            ("سارة", &["Sara", "Sarah"]),
            ("شريف", &["Sharif", "Shareef"]),
            ("نور", &["Nour", "Noor"]),
            ("مصطفى", &["Mustafa", "Moustafa"]),
            ("عبد الله", &["Abdullah", "Abd Allah", "Abdallah"]),
            ("عبدالله", &["Abdullah", "Abd Allah"]),
            ("عبد الرحمن", &["Abd al-Rahman", "Abd Al Rahman"]),
            ("ياسمين", &["Yasmin", "Yasmine"]),
        ];
        for (arabic, latin) in names {
            let key = match_key(arabic);
            assert!(!key.is_empty(), "{arabic}");
            for spelling in *latin {
                assert_eq!(match_key(spelling), key, "{spelling} vs {arabic}");
            }
        }
        assert_eq!(match_key("محمد"), "mhmd");
        assert_eq!(match_key("Mohammed"), "mhmd");
    }

    #[test]
    fn different_names_get_different_keys() {
        let keys: Vec<String> = ["محمد", "أحمد", "علي", "عمر", "خالد", "Sara", "Karim"]
            .iter()
            .map(|n| match_key(n))
            .collect();
        for (i, a) in keys.iter().enumerate() {
            for b in &keys[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn match_key_edge_cases() {
        assert_eq!(match_key(""), "");
        assert_eq!(match_key("  ... "), "");
        assert_eq!(match_key("aeiou"), "");
        assert_eq!(match_key("MOHAMMED   ALI"), "mhmdl");
        assert_eq!(match_key("Zoë"), "z");
        assert_eq!(match_key("Unit 42"), "nt42");
        assert_eq!(match_key("هاتف ٠١٢"), "htf012");
    }
}

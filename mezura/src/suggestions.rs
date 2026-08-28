// Edit distance alone fails on the case that motivates this: 'list-themes' and 'show-themes' are
// four edits apart, and a threshold that finds them matches noise. The names are hyphen-compounded,
// so a shared token is the stronger signal and is tried first.

pub fn suggest<'a>(input: &str, candidates: &[&'a str]) -> Vec<&'a str> {
    let input = input.trim().to_lowercase();
    if input.is_empty() {
        return Vec::new();
    }

    let input_tokens = input.split(['-', '_', ' ']).filter(|x| !x.is_empty()).collect::<Vec<_>>();
    let by_token = candidates.iter().filter(|candidate| {
            candidate.to_lowercase().split(['-', '_', ' ']).any(|token|
                    input_tokens.iter().any(|x| tokens_match(x, token)))
        }).copied().collect::<Vec<_>>();
    if !by_token.is_empty() {
        return by_token;
    }

    let by_prefix = candidates.iter().filter(|candidate| candidate.to_lowercase().starts_with(&input))
            .copied().collect::<Vec<_>>();
    if !by_prefix.is_empty() {
        return by_prefix;
    }

    let tolerance = calculate_max_edit_distance(&input);
    candidates.iter().filter(|candidate| calculate_edit_distance(&input, &candidate.to_lowercase()) <= tolerance)
            .copied().collect()
}

pub fn format_suggestion(input: &str, candidates: &[&str]) -> Option<String> {
    let matches = suggest(input, candidates);
    if matches.is_empty() {
        return None;
    }

    Some(format!("Did you mean:\n  {}", matches.join("\n  ")))
}

// The tolerance grows with the name, because two edits over three letters reaches most of the
// alphabet: 'rst' offers 'Rust' alone, where a flat two also offers 'CSS', 'JS', 'R' and 'TS'.
fn calculate_max_edit_distance(input: &str) -> usize {
    if input.chars().count() < 5 {1} else {2}
}

// 'palette' and 'palettes' are the same word for this purpose, and so are 'lang' and 'languages',
// but a single letter shared with a long token is not a signal
fn tokens_match(a: &str, b: &str) -> bool {
    const MIN_PREFIX : usize = 3;
    if a == b {
        return true;
    }
    let shorter = a.len().min(b.len());
    shorter >= MIN_PREFIX && (a.starts_with(b) || b.starts_with(a))
}

fn calculate_edit_distance(a: &str, b: &str) -> usize {
    let (a, b) = (a.chars().collect::<Vec<_>>(), b.chars().collect::<Vec<_>>());
    if a.is_empty() || b.is_empty() {
        return a.len().max(b.len());
    }

    let mut previous = (0..=b.len()).collect::<Vec<_>>();
    let mut current = vec![0; b.len() + 1];
    for (i, a_char) in a.iter().enumerate() {
        current[0] = i + 1;
        for (j, b_char) in b.iter().enumerate() {
            let substitution = previous[j] + usize::from(a_char != b_char);
            current[j + 1] = substitution.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }

    previous[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMANDS : [&str; 8] = ["show-themes", "theme-editor", "show-configs", "show-languages",
            "theme", "save-theme", "threads", "exclude"];

    #[test]
    fn a_shared_token_beats_the_edit_distance_that_would_miss_it() {
        assert!(calculate_edit_distance("list-themes", "show-themes") > calculate_max_edit_distance("list-themes"));
        assert_eq!(vec!["show-themes", "theme-editor", "theme", "save-theme"], suggest("list-themes", &COMMANDS));
    }

    #[test]
    fn a_prefix_offers_the_whole_family() {
        assert_eq!(vec!["show-themes", "show-configs", "show-languages"], suggest("show", &COMMANDS));
    }

    #[test]
    fn a_plain_typo_falls_through_to_the_edit_distance() {
        assert_eq!(vec!["threads"], suggest("treads", &COMMANDS));
        assert_eq!(vec!["exclude"], suggest("exclud", &COMMANDS));
    }

    #[test]
    fn a_short_name_gets_a_tighter_tolerance() {
        let languages = ["Rust", "CSS", "JS", "R", "TS", "TSX"];
        assert_eq!(vec!["Rust"], suggest("rst", &languages));
    }

    #[test]
    fn nothing_close_suggests_nothing() {
        assert!(suggest("zzzzzzzz", &COMMANDS).is_empty());
        assert!(suggest("", &COMMANDS).is_empty());
    }

    #[test]
    fn a_token_has_to_be_long_enough_to_mean_something() {
        assert!(tokens_match("theme", "themes"));
        assert!(tokens_match("lang", "languages"));
        assert!(!tokens_match("t", "themes"));
        assert!(!tokens_match("th", "themes"));
    }

    #[test]
    fn one_edit_is_one_insertion_one_deletion_or_one_substitution() {
        assert_eq!(0, calculate_edit_distance("same", "same"));
        assert_eq!(1, calculate_edit_distance("save", "safe"));
        assert_eq!(1, calculate_edit_distance("theme", "themes"));
        assert_eq!(4, calculate_edit_distance("", "four"));
        assert_eq!(3, calculate_edit_distance("kitten", "sitting"));
    }
}

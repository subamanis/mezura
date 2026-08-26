//! The arithmetic behind showing a result: shares, percentages, and how a number reads to a person.
//! Nothing here decides a color, a width or a word, and nothing reads a global.

const TINY_THRESHOLD : f64 = 0.01;

/// How many cells of a bar of `total_cells` each share is worth, by largest remainder.
///
/// The shares are percentages of the same whole, so a list of them adding up to less than 100 gets
/// a bar that is not full: one language holding a tenth of a project draws four cells of forty,
/// and the thirty six left empty are everything the caller did not name. A caller wanting the
/// largest few drawn against each other instead asks
/// [`calculate_percentages_of_their_own_sum`] for its shares.
///
/// Anything visible keeps at least one cell, so the total comes to `total_cells` unless more shares
/// are visible than there are cells to give them, where it is the number of visible shares.
// The at-least-one rule can push the total over the target, since 97/1/1/1 wants fifty-one cells in
// a bar of fifty: the excess comes off whoever holds the most and never empties anyone, because a
// cell missing from a share of 96 is invisible while the same cell taken from a share of 3
// understates it by a third.
pub fn apportion(shares: &[f64], total_cells: usize) -> Vec<usize> {
    let exact = shares.iter().map(|x| x * total_cells as f64 / 100.0).collect::<Vec<_>>();
    let mut cells = shares.iter().zip(exact.iter())
            .map(|(share, exact)| if *share < TINY_THRESHOLD {0} else {(*exact as usize).max(1)})
            .collect::<Vec<_>>();

    let mut sum = cells.iter().sum::<usize>();

    while sum < total_cells {
        let distance_below = |i: &usize| exact[*i] - cells[*i] as f64;
        // Still below its own claim, or a partial list would go on being handed cells after every
        // share already has all it is worth, and the ones left over flatten the differences: over
        // the fifty cells the tests below use, 40/8/2 filled that way is 28/12/10, where the
        // smallest share is a twentieth of the largest.
        let furthest_below = (0..cells.len()).filter(|i| shares[*i] >= TINY_THRESHOLD)
                .filter(|i| distance_below(i) > 0.0)
                .max_by(|a, b| distance_below(a).total_cmp(&distance_below(b)));
        match furthest_below {
            Some(i) => cells[i] += 1,
            None => break
        }
        sum += 1;
    }

    while sum > total_cells {
        let largest = (0..cells.len()).filter(|i| cells[*i] > 1)
                .max_by(|a, b| cells[*a].cmp(&cells[*b]).then(exact[*a].total_cmp(&exact[*b])));
        match largest {
            Some(i) => cells[i] -= 1,
            None => break
        }
        sum -= 1;
    }

    cells
}

/// What share of the whole each number holds, the whole being the sum of the numbers themselves.
///
/// Only right when the list is everything: asking this of the top few gives shares of the few,
/// which look like shares of the whole and are not. A caller that cut its list wants
/// [`calculate_percentages_of_a_given_total`].
///
/// Rounded to two decimals and summing to 100, with the last entry absorbing what the rounding of
/// the others left over. So the order matters, and a list ending in a leftovers row wants that row
/// last.
pub fn calculate_percentages_of_their_own_sum(numbers: &[usize]) -> Vec<f64> {
    calculate_percentages_of_a_given_total(numbers, numbers.iter().sum())
}

/// The same against a total the caller names: what each number is worth out of everything there
/// was, whether or not everything there was is in the list.
///
/// A share that rounds to zero comes back as the true small number rather than as zero, so whoever
/// formats it can tell "none" from "too little to show". [`NumberFormat::percent`] writes `<0.01`
/// for anything positive that rounds away and [`apportion`] gives it no cell, both from the honest
/// figure.
pub fn calculate_percentages_of_a_given_total(numbers: &[usize], total: usize) -> Vec<f64> {
    if total == 0 {
        return vec![0.0; numbers.len()];
    }

    // The last entry mops up the rounding of the others only when the list really is the whole of
    // the total, which is the case that owes the reader exactly 100. A list that is a part of
    // something larger owes 100 nothing.
    let accounts_for_everything = numbers.iter().sum::<usize>() == total;
    let exact = |number: usize| number as f64 / total as f64 * 100f64;
    let mut shares = Vec::with_capacity(numbers.len());
    let mut sum = 0.0;
    for (position, number) in numbers.iter().enumerate() {
        if accounts_for_everything && position == numbers.len() - 1 {
            let remainder = if sum > 99.99 {0.0} else {((100f64 - sum) * 100f64).round() / 100f64};
            shares.push(if remainder == 0.0 && *number > 0 {exact(*number)} else {remainder});
        } else {
            let rounded = (exact(*number) * 100f64).round() / 100f64;
            // The running sum takes the rounded value, so that a share too small to print leaves
            // the arithmetic of the last entry untouched
            sum += rounded;
            shares.push(if rounded == 0.0 && *number > 0 {exact(*number)} else {rounded});
        }
    }

    shares
}

/// How much bigger or smaller the newer figure is, as a percentage of the older one.
///
/// Signed, so a shrinking count comes back negative, and zero when there was nothing to grow from:
/// a jump out of nothing is not a percentage, and calling it one prints `inf`.
pub fn calculate_relative_change(older: usize, newer: usize) -> f64 {
    if older == 0 {
        return 0.0;
    }
    (newer as f64 - older as f64) / older as f64 * 100.0
}

/// What a person expects a number to look like, which differs by country and settles nothing about
/// what was counted.
///
/// Held as a value and passed, never read from a global: two callers in one process are allowed to
/// want different things.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumberFormat {
    thousands: Option<char>,
    decimal: char
}

impl NumberFormat {
    /// The character between every three digits, if any, and the one before the decimals.
    pub fn new(thousands: Option<char>, decimal: char) -> Self {
        NumberFormat { thousands, decimal }
    }

    /// A whole number, grouped.
    pub fn integer(&self, number: usize) -> String {
        self.grouped(&number.to_string())
    }

    /// Groups digits already written out. Applied to text that is already rounded, so that every
    /// rule about rounding stays written with a dot and only this last step decides what the reader
    /// sees.
    pub fn grouped(&self, digits: &str) -> String {
        let Some(separator) = self.thousands else {
            return self.with_decimal_mark(digits);
        };

        let (whole, rest) = match digits.split_once('.') {
            Some((whole, fraction)) => (whole, Some(fraction)),
            None => (digits, None)
        };
        let mut grouped = String::with_capacity(digits.len() + digits.len() / 3);
        for (position, character) in whole.chars().rev().enumerate() {
            if position != 0 && position % 3 == 0 {
                grouped.insert(0, separator);
            }
            grouped.insert(0, character);
        }
        match rest {
            Some(fraction) => grouped + &self.decimal.to_string() + fraction,
            None => grouped
        }
    }

    /// A count of bytes in the largest unit that leaves a figure worth reading, so that 2417403 is
    /// 2.4 and not 2417.4, and the unit beside it.
    ///
    /// Divided by 1000 and not 1024, which is what `KB` means; the 1024 ladder is spelled `KiB`.
    ///
    /// Only a divided value gets a decimal: a count of bytes is a whole number, and `430.0 B`
    /// would claim a precision the figure does not have.
    pub fn size_with_unit(&self, bytes: usize) -> (String, &'static str) {
        if bytes >= 1_000_000_000 {(self.with_decimal_mark(&format!("{:.1}", bytes as f64 / 1_000_000_000f64)), "GB")}
        else if bytes >= 1_000_000 {(self.with_decimal_mark(&format!("{:.1}", bytes as f64 / 1_000_000f64)), "MB")}
        else if bytes >= 1_000 {(self.with_decimal_mark(&format!("{:.1}", bytes as f64 / 1_000f64)), "KB")}
        else {(self.integer(bytes), "B")}
    }

    /// A percentage to two decimals, and `<0.01` for a share that is present but would round to
    /// `0.00` and read as absent.
    // The comparison is on the formatted text rather than on the number, which keeps the rule
    // independent of how the formatter rounds a halfway value.
    pub fn percent(&self, value: f64) -> String {
        let text = format!("{value:.2}");
        self.with_decimal_mark(&if value > 0.0 && text == "0.00" {"<0.01".to_owned()} else {text})
    }

    /// The same, carrying the direction: what a comparison against an earlier run prints.
    pub fn signed_percent(&self, value: f64) -> String {
        let magnitude = value.abs();
        let sign = if value > 0.0 {"+"} else if value < 0.0 {"-"} else {""};
        let (marker, magnitude) = if magnitude > 0.0 && magnitude < TINY_THRESHOLD {(" <", TINY_THRESHOLD)}
                else {("", magnitude)};

        format!("{sign}{marker}{}", self.with_decimal_mark(&round_2(magnitude).to_string()))
    }

    /// Swaps the dot for this format's decimal mark, for text a caller has already shaped itself.
    pub fn with_decimal_mark(&self, text: &str) -> String {
        match self.decimal {
            '.' => text.to_owned(),
            mark => text.replace('.', &mark.to_string())
        }
    }
}

// What the program itself prints. Grouped, since '{}' already hands out plain digits; ungrouped is
// asked for by name, with 'NumberFormat::new(None, '.')'.
impl Default for NumberFormat {
    fn default() -> Self {
        NumberFormat { thousands: Some(','), decimal: '.' }
    }
}

fn round_2(number: f64) -> f64 {
    (number * 100.0).round() / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const BAR : usize = 50;

    #[test]
    fn apportionment_is_exact_and_scales_to_any_number_of_cells() {
        assert_eq!(vec![25,25], apportion(&[49.6,50.4], BAR));
        assert_eq!(vec![0,50], apportion(&[0.0,100.0], BAR));
        assert_eq!(vec![16,17,17], apportion(&[33.33,33.33,33.34], BAR));
        assert_eq!(vec![1,32,17], apportion(&[0.3,65.67,34.3], BAR));
        assert_eq!(vec![0,0,50], apportion(&[0.0,0.0,100.0], BAR));
        assert_eq!(vec![1,24,25], apportion(&[0.2,49.9,49.9], BAR));
        assert_eq!(vec![6,25,13,6], apportion(&[12.5,50.0,25.0,12.5], BAR));
        assert_eq!(vec![1,1,24,24], apportion(&[0.1,0.1,49.9,49.9], BAR));

        assert_eq!(25, apportion(&[50.0,50.0], 50)[0]);
        assert_eq!(50, apportion(&[50.0,50.0], 100)[0]);
        assert_eq!(10, apportion(&[50.0,50.0], 20)[0]);
    }

    // The minimum-one rule wants 48+1+1+1 here, one cell over the target.
    #[test]
    fn the_cell_that_a_protected_minimum_costs_comes_off_the_largest_share() {
        assert_eq!(vec![47,1,1,1], apportion(&[97.0,1.0,1.0,1.0], BAR));
        assert_eq!(vec![47,1,1,1], apportion(&[99.4,0.2,0.2,0.2], BAR));

        let cells = apportion(&[99.7,0.1,0.1,0.1], BAR);
        assert_eq!(BAR, cells.iter().sum::<usize>());
        assert!(cells.iter().all(|x| *x >= 1), "a share that is present must never lose its last cell");

        // The second entry deserves 3 cells of the hundred and must keep them: losing one
        // understates it by a third, while the first barely notices
        assert_eq!(vec![93,3,1,1,1,1], apportion(&[96.96, 3.0, 0.01, 0.01, 0.01, 0.01], 100));
        assert_eq!(vec![45,1,1,1,1,1], apportion(&[96.96, 3.0, 0.01, 0.01, 0.01, 0.01], BAR));
    }

    #[test]
    fn the_cells_always_sum_to_the_total_and_keep_visible_shares_visible() {
        let cases: Vec<Vec<f64>> = vec![
            vec![100.0], vec![50.0,50.0], vec![0.01,99.99], vec![0.0,0.0,0.0,100.0],
            vec![25.0,25.0,25.0,25.0], vec![70.0,10.0,10.0,10.0], vec![1.0,1.0,1.0,97.0],
            vec![0.04,0.04,0.04,99.88], vec![33.34,33.33,33.33], vec![60.5,39.5],
            vec![98.0,2.0], vec![2.0,98.0], vec![0.0,100.0,0.0]
        ];

        for shares in cases {
            let cells = apportion(&shares, BAR);
            assert_eq!(BAR, cells.iter().sum::<usize>(), "wrong total for {shares:?}");
            for (i, share) in shares.iter().enumerate() {
                if *share > 0.0 {
                    assert!(cells[i] >= 1, "{shares:?} made a present share disappear");
                } else {
                    assert_eq!(0, cells[i], "{shares:?} gave a cell to a share of nothing");
                }
            }
        }
    }

    // What a caller drawing only the largest few gets: the bar stops where their share of the whole
    // stops, and the empty part of it is everything they did not name. Filled instead, the leftover
    // cells go to whoever is furthest below their claim and then, once nobody is, to whoever the
    // tiebreak reaches, which flattens the differences the bar exists to show.
    #[test]
    fn a_list_that_covers_less_than_the_whole_draws_a_bar_that_stops_short() {
        assert_eq!(vec![5], apportion(&[10.0], BAR));
        assert_eq!(vec![13, 13], apportion(&[25.0, 25.0], BAR));
        assert_eq!(vec![20, 4, 1], apportion(&[40.0, 8.0, 2.0], BAR));

        // and the same three shares as shares of each other fill it, in their real proportions
        let of_each_other = calculate_percentages_of_their_own_sum(&[40, 8, 2]);
        let cells = apportion(&of_each_other, BAR);
        assert_eq!(BAR, cells.iter().sum::<usize>());
        assert_eq!(vec![40, 8, 2], cells);
    }

    #[test]
    fn percentages_sum_to_a_hundred_with_the_last_entry_absorbing_the_rounding() {
        assert_eq!(vec![0f64,50f64,50f64], calculate_percentages_of_their_own_sum(&[0,100,100]));
        assert_eq!(vec![100f64,0f64,0f64], calculate_percentages_of_their_own_sum(&[1,0,0]));
        assert_eq!(vec![33.33,33.33,33.34], calculate_percentages_of_their_own_sum(&[20,20,20]));
        assert_eq!(vec![0f64,50f64,50f64,0f64], calculate_percentages_of_their_own_sum(&[0,100,100,0]));
        assert_eq!(vec![33.33,33.33,33.33,0.01], calculate_percentages_of_their_own_sum(&[100,100,100,0]));
        assert_eq!(vec![33.28,33.28,33.44,0.0], calculate_percentages_of_their_own_sum(&[200,200,201,0]));
    }

    // 3 files out of 800,000 is 0.000375%. Checked in the middle and in the last position, which
    // are computed by different branches.
    #[test]
    fn a_share_that_rounds_away_is_named_rather_than_shown_as_absent() {
        let format = NumberFormat::default();
        for numbers in [vec![500_000, 3, 299_997], vec![500_000, 299_997, 3]] {
            let shares = calculate_percentages_of_their_own_sum(&numbers);
            let tiny = numbers.iter().position(|x| *x == 3).unwrap();
            assert_eq!("<0.01", format.percent(shares[tiny]), "for {numbers:?}");
            let cells = apportion(&shares, BAR);
            assert_eq!(0, cells[tiny], "a share too small to be printed must not claim a cell either");
            // The neighbouring test cannot carry this one: its rule is that every share above zero
            // keeps a cell, which is exactly what the share here does not do
            assert_eq!(BAR, cells.iter().sum::<usize>(), "the bar came up short around the tiny share");
        }

        // One that really is absent stays a flat zero and keeps no cell
        let shares = calculate_percentages_of_their_own_sum(&[500_000, 299_997, 0]);
        assert_eq!("0.00", format.percent(shares[2]));
        assert_eq!(0, apportion(&shares, BAR)[2]);

        // The figure a caller reads is the true share and not a marker standing in for it
        let shares = calculate_percentages_of_their_own_sum(&[500_000, 3, 299_997]);
        assert!((shares[1] - 0.000375).abs() < 1e-9, "the share was replaced by a marker: {}", shares[1]);
        assert!((shares.iter().sum::<f64>() - 100.0).abs() < 0.01, "the shares no longer sum to 100: {shares:?}");
    }

    // The largest two of three, as shares of each other and then as shares of the whole: two
    // different questions, and the plain call answers the first.
    #[test]
    fn a_share_is_of_its_own_sum_unless_a_total_is_given() {
        assert_eq!(vec![62.5, 37.5], calculate_percentages_of_their_own_sum(&[500_000, 300_000]));
        assert_eq!(vec![50.0, 30.0], calculate_percentages_of_a_given_total(&[500_000, 300_000], 1_000_000));
        assert_eq!(vec![0.0, 0.0], calculate_percentages_of_a_given_total(&[0, 0], 0));
    }

    // A report pads every percentage into a five column field, so '<0.01' has to fit in it. 100.00
    // is the one value that does not, which is why such padding saturates.
    #[test]
    fn every_percentage_but_a_full_hundred_fits_a_five_column_field() {
        let format = NumberFormat::default();
        assert_eq!("0.00", format.percent(0.0));
        assert_eq!("0.01", format.percent(0.01));
        assert_eq!("12.35", format.percent(12.345));
        assert_eq!("100.00", format.percent(100.0));

        for value in [0.0, 0.000375, 0.01, 9.9, 99.99] {
            assert!(format.percent(value).len() <= 5, "'{}' does not fit the column", format.percent(value));
        }
        assert_eq!(6, format.percent(100.0).len());
    }

    // The boundary belongs to the larger unit: a thousand bytes is one KB.
    #[test]
    fn a_size_takes_the_largest_unit_that_leaves_a_figure_worth_reading() {
        let format = NumberFormat::default();
        assert_eq!(("999".to_owned(), "B"), format.size_with_unit(999));
        assert_eq!(("1.0".to_owned(), "KB"), format.size_with_unit(1_000));
        assert_eq!(("1.0".to_owned(), "MB"), format.size_with_unit(1_000_000));
        assert_eq!(("1.0".to_owned(), "GB"), format.size_with_unit(1_000_000_000));
        assert_eq!(("2.4".to_owned(), "MB"), format.size_with_unit(2_417_403));
        assert_eq!(("0".to_owned(), "B"), format.size_with_unit(0));
    }

    #[test]
    fn a_change_out_of_nothing_is_not_a_percentage() {
        assert_eq!(0.0, calculate_relative_change(0, 500));
        assert_eq!(0.0, calculate_relative_change(0, 0));
        assert_eq!(100.0, calculate_relative_change(100, 200));
        assert_eq!(-10.0, calculate_relative_change(100, 90));
        assert_eq!(0.0, calculate_relative_change(100, 100));
    }

    #[test]
    fn a_number_is_grouped_and_marked_the_way_the_caller_asked() {
        let plain = NumberFormat::new(None, '.');
        assert_eq!("1234567", plain.integer(1234567));
        assert_eq!("1.5", plain.grouped("1.5"));

        let english = NumberFormat::new(Some(','), '.');
        assert_eq!("123", english.integer(123));
        assert_eq!("1,234", english.integer(1234));
        assert_eq!("12,345", english.integer(12345));
        assert_eq!("1,234,567", english.integer(1234567));

        // The grouping counts the digits before the mark and never the ones after it
        let european = NumberFormat::new(Some('.'), ',');
        assert_eq!("1.234.567", european.integer(1234567));
        assert_eq!("1.234,5", european.grouped("1234.5"));
        assert_eq!("12,35", european.percent(12.345));

        // The unit boundaries are the test above; here only that the mark reaches a size too
        assert_eq!(("2,4".to_owned(), "MB"), european.size_with_unit(2_417_403));

        // and saying nothing is the same as asking for the one above, which is what the program prints
        assert_eq!(english.integer(1234567), NumberFormat::default().integer(1234567));
        assert_eq!("1,234,567", NumberFormat::default().integer(1234567));
    }

    #[test]
    fn a_signed_percentage_carries_its_direction_and_names_the_tiny_ones() {
        let format = NumberFormat::default();
        assert_eq!("0", format.signed_percent(calculate_relative_change(100, 100)));
        assert_eq!("-10", format.signed_percent(calculate_relative_change(100, 90)));
        assert_eq!("+100", format.signed_percent(calculate_relative_change(100, 200)));
        assert_eq!("+ <0.01", format.signed_percent(calculate_relative_change(22819, 22820)));
        assert_eq!("+0.01", format.signed_percent(0.01));
    }
}

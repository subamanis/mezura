// The arithmetic behind showing a result, and nothing that decides a colour, a width or a word.
// It lives here rather than in whatever draws the report because none of it is about a terminal: a
// bar of fifty cells, a bar of two hundred, an HTML page and a spreadsheet all divide the same
// shares the same way, and getting the division right is the only hard part of any of them.
//
// Nothing here reads a global. What a person prefers to see, the digit grouping and the decimal
// mark, is a 'NumberFormat' the caller holds and passes.

const TINY_THRESHOLD : f64 = 0.01;


// How many cells of a bar of 'total_cells' each share is worth, by largest remainder: every share
// takes the whole part of its exact claim, anything visible keeps at least one cell so that it cannot
// vanish, and the cells still unspent go one at a time to whoever sits furthest below their exact
// claim. The result always sums to 'total_cells' exactly.
//
// Exact in both directions, which is the part that is easy to get wrong: the minimum-one rule can
// push the total over the target, since 97/1/1/1 wants fifty-one cells in a bar of fifty. The
// excess comes off whoever holds the most and never empties anybody, because what a bar owes the
// reader is relative fidelity: one cell missing from a share of 96 is invisible, while the same
// cell taken from a share of 3 understates it by a third.
//
// 'shares' are percentages, so they are expected to sum to about 100; a list that does not simply
// gets a bar that is not full.
pub fn apportion(shares: &[f64], total_cells: usize) -> Vec<usize> {
    let exact = shares.iter().map(|x| x * total_cells as f64 / 100.0).collect::<Vec<_>>();
    let mut cells = shares.iter().zip(exact.iter())
            .map(|(share, exact)| if *share < TINY_THRESHOLD {0} else {(*exact as usize).max(1)})
            .collect::<Vec<_>>();

    let mut sum = cells.iter().sum::<usize>();

    while sum < total_cells {
        let distance_below = |i: &usize| exact[*i] - cells[*i] as f64;
        let furthest_below = (0..cells.len()).filter(|i| shares[*i] >= TINY_THRESHOLD)
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

// The share of the whole each number holds, where the whole is the sum of the numbers themselves.
// That is the right answer only when the list is everything, so a caller that has cut its list must
// say what it cut it from and use 'percentages_of' below: taking the top few and asking this gives
// shares of the few, which look like shares of the whole and are not.
//
// Rounded to two decimals and summing to 100. The last entry absorbs whatever the rounding of the
// others left over, which is why the order matters and why a list that ends in a leftovers row
// wants that row last.
pub fn percentages(numbers: &[usize]) -> Vec<f64> {
    percentages_of(numbers, numbers.iter().sum())
}

// The same, against a total the caller names: what each number is worth out of everything there
// was, whether or not everything there was is in the list. A report that shows the largest few and
// folds the rest into a leftovers row can use either, since the row makes the list whole again.
//
// A share that rounds to zero comes back as the true small number and not as zero, so that whoever
// formats it can tell "none" from "too little to show": 'NumberFormat::percent' writes '<0.01' for
// anything positive that rounds away, and 'apportion' gives no cell to anything under a hundredth,
// so both answers are reached from the honest figure rather than from a marker standing in for it.
pub fn percentages_of(numbers: &[usize], total: usize) -> Vec<f64> {
    if total == 0 {
        return vec![0.0; numbers.len()];
    }

    // The last entry mops up the rounding of the others only when the list really is the whole of
    // the total, which is the case that owes the reader exactly 100. A list that is a part of
    // something larger owes 100 nothing, and handing its last entry the remainder is the very
    // renormalisation this function exists to let a caller avoid.
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

// How much bigger or smaller the newer figure is, as a percentage of the older one. Signed, so a
// shrinking count comes back negative, and zero when there was nothing to grow from: a jump out of
// nothing is not a percentage, and calling it one prints 'inf'.
pub fn relative_change(older: usize, newer: usize) -> f64 {
    if older == 0 {
        return 0.0;
    }
    (newer as f64 - older as f64) / older as f64 * 100.0
}


// What a person expects a number to look like, which differs by country and settles nothing about
// what was counted. Held as a value and passed, never read from a global: two callers in one
// process are allowed to want different things.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumberFormat {
    thousands: Option<char>,
    decimal: char
}

impl NumberFormat {
    pub fn new(thousands: Option<char>, decimal: char) -> Self {
        NumberFormat { thousands, decimal }
    }

    pub fn integer(&self, number: usize) -> String {
        self.grouped(&number.to_string())
    }

    // Applied to text that is already rounded, so that every rule about rounding stays written with
    // a dot and only this last step decides what the reader sees.
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

    // A count of bytes in the largest unit that leaves a figure worth reading, so that 2417403 is
    // 2.4 and not 2417.4. Every row of a report goes through it, which is what stops one line from
    // calling a value MBs while the line under it calls the same value KBs.
    //
    // A count of bytes is a whole number, so '430.0 Bytes' would claim a precision the figure does
    // not have: only a divided value has a decimal to show.
    pub fn size_with_unit(&self, bytes: usize) -> (String, &'static str) {
        if bytes >= 1_000_000_000 {(self.with_decimal_mark(&format!("{:.1}", bytes as f64 / 1_000_000_000f64)), "GBs")}
        else if bytes >= 1_000_000 {(self.with_decimal_mark(&format!("{:.1}", bytes as f64 / 1_000_000f64)), "MBs")}
        else if bytes >= 1_000 {(self.with_decimal_mark(&format!("{:.1}", bytes as f64 / 1_000f64)), "KBs")}
        else {(self.integer(bytes), "Bytes")}
    }

    // A share that is present but rounds to 0.00 would read as absent, so it is named instead. The
    // comparison is on the formatted text rather than on the number, which keeps the rule
    // independent of how the formatter rounds a halfway value.
    pub fn percent(&self, value: f64) -> String {
        let text = format!("{value:.2}");
        self.with_decimal_mark(&if value > 0.0 && text == "0.00" {"<0.01".to_owned()} else {text})
    }

    // The same, carrying the direction: what a comparison against an earlier run prints.
    pub fn signed_percent(&self, value: f64) -> String {
        let magnitude = value.abs();
        let sign = if value > 0.0 {"+"} else if value < 0.0 {"-"} else {""};
        let (marker, magnitude) = if magnitude > 0.0 && magnitude < TINY_THRESHOLD {(" <", TINY_THRESHOLD)}
                else {("", magnitude)};

        format!("{sign}{marker}{}", self.with_decimal_mark(&round_2(magnitude).to_string()))
    }

    // For text a caller has already shaped itself and only wants the mark put on, which is every
    // figure whose rounding rule is written elsewhere: those stay written with a dot, and this is
    // the single step that decides what the reader sees.
    pub fn with_decimal_mark(&self, text: &str) -> String {
        match self.decimal {
            '.' => text.to_owned(),
            mark => text.replace('.', &mark.to_string())
        }
    }
}

// Plain digits and a dot, which is what a caller that never says otherwise gets
impl Default for NumberFormat {
    fn default() -> Self {
        NumberFormat { thousands: None, decimal: '.' }
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

        // The count is a parameter, so the same shares scale to any bar
        assert_eq!(25, apportion(&[50.0,50.0], 50)[0]);
        assert_eq!(50, apportion(&[50.0,50.0], 100)[0]);
        assert_eq!(10, apportion(&[50.0,50.0], 20)[0]);
    }

    // The minimum-one rule wants 48+1+1+1 here, which is one cell over the target. The excess has
    // to come off the largest share rather than emptying one of the small ones.
    #[test]
    fn the_cell_that_a_protected_minimum_costs_comes_off_the_largest_share() {
        assert_eq!(vec![47,1,1,1], apportion(&[97.0,1.0,1.0,1.0], BAR));
        assert_eq!(vec![47,1,1,1], apportion(&[99.4,0.2,0.2,0.2], BAR));

        let cells = apportion(&[99.7,0.1,0.1,0.1], BAR);
        assert_eq!(BAR, cells.iter().sum::<usize>());
        assert!(cells.iter().all(|x| *x >= 1), "a share that is present must never lose its last cell");

        // Six entries at a hundred cells: the second deserves 3 and must keep them, because losing
        // one understates it by a third while the first barely notices
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

    #[test]
    fn percentages_sum_to_a_hundred_with_the_last_entry_absorbing_the_rounding() {
        assert_eq!(vec![0f64,50f64,50f64], percentages(&[0,100,100]));
        assert_eq!(vec![100f64,0f64,0f64], percentages(&[1,0,0]));
        assert_eq!(vec![33.33,33.33,33.34], percentages(&[20,20,20]));
        assert_eq!(vec![0f64,50f64,50f64,0f64], percentages(&[0,100,100,0]));
        assert_eq!(vec![33.33,33.33,33.33,0.01], percentages(&[100,100,100,0]));
        assert_eq!(vec![33.28,33.28,33.44,0.0], percentages(&[200,200,201,0]));
    }

    // 3 files out of 800000 is 0.000375%, which used to print as a flat 0.00. Checked in the middle
    // and in the last position, which are computed by different branches.
    #[test]
    fn a_share_that_rounds_away_is_named_rather_than_shown_as_absent() {
        let format = NumberFormat::default();
        for numbers in [vec![500_000, 3, 299_997], vec![500_000, 299_997, 3]] {
            let shares = percentages(&numbers);
            let tiny = numbers.iter().position(|x| *x == 3).unwrap();
            assert_eq!("<0.01", format.percent(shares[tiny]), "for {numbers:?}");
            let cells = apportion(&shares, BAR);
            assert_eq!(0, cells[tiny], "a share too small to be printed must not claim a cell either");
            assert_eq!(BAR, cells.iter().sum::<usize>());
        }

        // One that really is absent stays a flat zero and keeps no cell
        let shares = percentages(&[500_000, 299_997, 0]);
        assert_eq!("0.00", format.percent(shares[2]));
        assert_eq!(0, apportion(&shares, BAR)[2]);

        assert_eq!("0.00", format.percent(0.0));
        assert_eq!("0.01", format.percent(0.01));
        assert_eq!("12.35", format.percent(12.345));
        assert_eq!("100.00", format.percent(100.0));

        // The figure a caller reads is the true one and not a marker: it used to come back as a
        // flat 0.001 whatever the real share was, which is 2.7 times this one and sums past 100.
        let shares = percentages(&[500_000, 3, 299_997]);
        assert!((shares[1] - 0.000375).abs() < 1e-9, "the share was replaced by a marker: {}", shares[1]);
        assert!((shares.iter().sum::<f64>() - 100.0).abs() < 0.01, "the shares no longer sum to 100: {shares:?}");

        // And the denominator is stated when the list is not everything. Taking the largest two of
        // three and asking for their shares of the whole is not the same question as their shares
        // of each other, and the plain call answers the second.
        assert_eq!(vec![62.5, 37.5], percentages(&[500_000, 300_000]));
        assert_eq!(vec![50.0, 30.0], percentages_of(&[500_000, 300_000], 1_000_000));
        assert_eq!(vec![0.0, 0.0], percentages_of(&[0, 0], 0));

        // A report pads every percentage into a five column field, so '<0.01' has to fit in it.
        // 100.00 is the one value that does not, which is why such padding saturates.
        for value in [0.0, 0.000375, 0.01, 9.9, 99.99] {
            assert!(format.percent(value).len() <= 5, "'{}' does not fit the column", format.percent(value));
        }
        assert_eq!(6, format.percent(100.0).len());
    }

    // The boundary belongs to the larger unit: a thousand bytes is one KB, and the figure that
    // reads '1000 Bytes' next to a '1.0 KBs' one byte later is the one that is wrong.
    #[test]
    fn a_size_takes_the_largest_unit_that_leaves_a_figure_worth_reading() {
        let format = NumberFormat::default();
        assert_eq!(("999".to_owned(), "Bytes"), format.size_with_unit(999));
        assert_eq!(("1.0".to_owned(), "KBs"), format.size_with_unit(1_000));
        assert_eq!(("1.0".to_owned(), "MBs"), format.size_with_unit(1_000_000));
        assert_eq!(("1.0".to_owned(), "GBs"), format.size_with_unit(1_000_000_000));
        assert_eq!(("2.4".to_owned(), "MBs"), format.size_with_unit(2_417_403));
        assert_eq!(("0".to_owned(), "Bytes"), format.size_with_unit(0));
    }

    #[test]
    fn a_change_out_of_nothing_is_not_a_percentage() {
        assert_eq!(0.0, relative_change(0, 500));
        assert_eq!(0.0, relative_change(0, 0));
        assert_eq!(100.0, relative_change(100, 200));
        assert_eq!(-10.0, relative_change(100, 90));
        assert_eq!(0.0, relative_change(100, 100));
    }

    #[test]
    fn a_number_is_grouped_and_marked_the_way_the_caller_asked() {
        let plain = NumberFormat::default();
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

        assert_eq!(("2.4".to_owned(), "MBs"), english.size_with_unit(2_417_403));
        assert_eq!(("2,4".to_owned(), "MBs"), european.size_with_unit(2_417_403));
        // A whole number of bytes is not divided, so it shows no decimal at all
        assert_eq!(("430".to_owned(), "Bytes"), english.size_with_unit(430));
        assert_eq!(("999".to_owned(), "Bytes"), english.size_with_unit(999));
    }

    #[test]
    fn a_signed_percentage_carries_its_direction_and_names_the_tiny_ones() {
        let format = NumberFormat::default();
        assert_eq!("0", format.signed_percent(relative_change(100, 100)));
        assert_eq!("-10", format.signed_percent(relative_change(100, 90)));
        assert_eq!("+100", format.signed_percent(relative_change(100, 200)));
        assert_eq!("+ <0.01", format.signed_percent(relative_change(22819, 22820)));
        assert_eq!("+0.01", format.signed_percent(0.01));
    }
}

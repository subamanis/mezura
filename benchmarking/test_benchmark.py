#!/usr/bin/env python3
import unittest

from benchmark import drift_of, parse_exclusion_list, parse_porcelain, read_totals


class PorcelainShapes(unittest.TestCase):
    def test_a_clean_tree_gives_an_empty_list(self):
        self.assertEqual(parse_porcelain(''), [])

    def test_modified_staged_and_untracked_lines_all_count(self):
        out = ' M src/main.rs\nM  Cargo.toml\n?? notes.txt'
        self.assertEqual(parse_porcelain(out), ['src/main.rs', 'Cargo.toml', 'notes.txt'])

    def test_a_rename_reports_the_new_name(self):
        self.assertEqual(parse_porcelain('R  old.rs -> new.rs'), ['new.rs'])

    def test_a_quoted_name_loses_its_quotes(self):
        self.assertEqual(parse_porcelain('?? "with space.txt"'), ['with space.txt'])

    def test_a_quoted_rename_reports_the_new_name_unquoted(self):
        self.assertEqual(parse_porcelain('R  "old name.rs" -> "new name.rs"'), ['new name.rs'])


class ToolJsonReaders(unittest.TestCase):
    def test_mezura_region_document(self):
        data = {'scope': {'counting': 'region'},
                'total': {'files': 4, 'lines': 100, 'code': 70, 'comments': 20, 'blanks': 10}}
        self.assertEqual(read_totals('mezura', data),
                         {'model': 'region', 'files': 4, 'lines': 100, 'code': 70,
                          'comments': 20, 'third': 'blanks', 'value': 10})

    def test_mezura_content_document_names_its_third_bucket_extra(self):
        data = {'scope': {'counting': 'content'},
                'total': {'files': 4, 'lines': 100, 'code': 70, 'comments': 20, 'extra': 10}}
        totals = read_totals('mezura', data)
        assert totals is not None
        self.assertEqual(totals['model'], 'content')
        self.assertEqual(totals['third'], 'extra')
        self.assertEqual(totals['value'], 10)

    def test_scc_sums_over_its_language_list(self):
        data = [{'Count': 2, 'Lines': 50, 'Code': 40, 'Comment': 5, 'Blank': 5},
                {'Count': 1, 'Lines': 10, 'Code': 8, 'Comment': 1, 'Blank': 1}]
        self.assertEqual(read_totals('scc', data),
                         {'model': 'region', 'files': 3, 'lines': 60, 'code': 48,
                          'comments': 6, 'third': 'blanks', 'value': 6})

    def test_tokei_reads_its_grand_total_once_and_never_sums_it_in(self):
        data = {'C': {'code': 10, 'comments': 2, 'blanks': 3,
                      'reports': [{'name': 'a.c'}, {'name': 'b.c'}]},
                'Rust': {'code': 5, 'comments': 1, 'blanks': 1,
                         'reports': [{'name': 'c.rs'}]},
                'Total': {'code': 15, 'comments': 3, 'blanks': 4,
                          'reports': [{'name': 'a.c'}, {'name': 'b.c'}, {'name': 'c.rs'}]}}
        totals = read_totals('tokei', data)
        assert totals is not None
        self.assertEqual(totals['lines'], 22)
        self.assertEqual(totals['files'], 3)
        self.assertEqual(totals['code'], 15)
        self.assertEqual(totals['comments'], 3)
        self.assertEqual(totals['value'], 4)

    def test_an_unknown_tool_gives_nothing(self):
        self.assertIsNone(read_totals('cloc', {}))


class DefenderPlaceholders(unittest.TestCase):
    def test_the_na_placeholder_means_unknown_not_empty(self):
        self.assertIsNone(parse_exclusion_list('N/A: Must be an administrator to view exclusions'))

    def test_lower_case_na_is_the_same_placeholder(self):
        self.assertIsNone(parse_exclusion_list('n/a'))

    def test_an_empty_answer_means_no_exclusions(self):
        self.assertEqual(parse_exclusion_list(''), [])

    def test_a_real_list_splits_on_pipes_and_trims(self):
        raw = 'C:\\tools\\mezura.exe | D:\\dev ||'
        self.assertEqual(parse_exclusion_list(raw), ['C:\\tools\\mezura.exe', 'D:\\dev'])


class DriftArithmetic(unittest.TestCase):
    def test_drift_is_the_ratio_of_the_two_control_means(self):
        measurements = [{'set': 'control-start', 'mean_s': 0.4},
                        {'set': 'control-end', 'mean_s': 0.5}]
        self.assertEqual(drift_of(measurements), 1.25)

    def test_a_faster_end_reports_the_same_drift_as_a_slower_one(self):
        measurements = [{'set': 'control-start', 'mean_s': 0.5},
                        {'set': 'control-end', 'mean_s': 0.4}]
        self.assertEqual(drift_of(measurements), 1.25)

    def test_a_missing_control_phase_gives_no_drift(self):
        measurements = [{'set': 'control-start', 'mean_s': 0.4}]
        self.assertEqual(drift_of(measurements), '')


if __name__ == '__main__':
    unittest.main()

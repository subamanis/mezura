-- mezura-real  10 lines 2 code 8 comment
-- mezura-count 10 lines 2 code 8 comment
-- tokei-real   10 lines 2 code 8 comment
-- tokei-count  10 lines 3 code 7 comment
-- trap: standard SQL escapes a quote by doubling it and the backslash is an ordinary character
-- tokei: the backslash cancels the closing quote, and its strings cross lines, so the comment
-- under the path is counted as string content
UPDATE t SET path = 'C:\';
-- a real comment
SELECT 2;

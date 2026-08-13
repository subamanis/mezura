-- mezura-real  10 lines 2 code 8 comment
-- mezura-count 10 lines 3 code 7 comment
-- tokei-real   10 lines 2 code 8 comment
-- tokei-count  10 lines 3 code 7 comment
-- trap: standard SQL escapes a quote by doubling it and the backslash is an ordinary character, so
-- mezura: the backslash is treated as escaping in every language, so the quote never closes
-- tokei: the same, and its strings cross lines too, so everything under the path is string content
UPDATE t SET path = 'C:\';
-- a real comment
SELECT 2;

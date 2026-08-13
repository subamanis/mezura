-- mezura-expect lines=8 code=3 comments=4 extra=1 tables=1 views=1
/* a block
   comment */
CREATE TABLE users (name text);
CREATE VIEW active AS SELECT '-- not a comment';

-- a comment
SELECT "quoted";

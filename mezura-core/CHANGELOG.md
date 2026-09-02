# Changelog

## 1.0.0, unreleased

The first release: the counting engine of mezura as a library. `run` counts a directory and
answers per language, per module and per file, `explain_file` reads one file line by line and says
why each line was counted the way it was, and `Languages` holds the over eighty shipped languages
and takes yours beside them. Every line is sorted into one of nine classes, and both counting
models, content and region, come out of one run.

The version moves with the library's API. The mezura program built on it has a version of its own.

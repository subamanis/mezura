// The machinery that turns a set of targets into a result. Nothing in here is a noun the user ever
// names: 'domain' and 'result' hold those, and this holds the verbs.
pub mod config;
pub mod modules;
pub mod targets;
pub mod extensions;
pub mod producer;
pub mod consumer;
pub mod file_parser;

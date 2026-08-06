// The machinery that turns a set of targets into a result. Nothing in here is a noun the user ever
// names: 'domain' and 'result' hold those, and this holds the verbs.
pub mod config;
pub mod targets;

pub(crate) mod consumer;
pub(crate) mod extensions;
pub(crate) mod file_parser;
pub(crate) mod modules;
pub(crate) mod producer;

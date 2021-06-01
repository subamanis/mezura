#![allow(warnings)]

use std::sync::Arc;
use std::time::Duration;

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use code_stats::cmd_arg_parser::ProgramArguments;
use code_stats::*;
use lazy_static::lazy_static;

lazy_static! {
    static ref extension_map_ref : ExtMapRef = Arc::new(extension_reader::parse_supported_extensions_to_map().unwrap().0);
}

fn parse_Unity_Projects() {
    code_stats::run(ProgramArguments::new(vec!["C:\\Users\\petro\\Documents\\Unity Projects".to_owned()]).unwrap(),
                    extension_reader::parse_supported_extensions_to_map().unwrap().0);
}

fn parse_Intellij_Projects() {
    code_stats::run(ProgramArguments::new(vec!["C:\\Users\\petro\\IdeaProjects".to_owned()]).unwrap(),
                    extension_reader::parse_supported_extensions_to_map().unwrap().0);
}


// -------------------------------- BENCHMARKS ---------------------------------------

fn unity_projects_benchmark(c: &mut Criterion) {
    c.bench_function("unity projects", |b| b.iter(|| parse_Unity_Projects()));
}

fn intellij_projects_benchmark(c: &mut Criterion) {
    c.bench_function("intellij projects", |b| b.iter(|| parse_Intellij_Projects()));
}


criterion_group!{
    name = benches;
    config = Criterion::default().warm_up_time(Duration::from_secs(8)).sample_size(40);
    targets = unity_projects_benchmark
}
criterion_main!(benches);
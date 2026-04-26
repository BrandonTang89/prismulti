//! Microbenchmarks for DTMC Construction and property checking.
//! Not really used much in favour of hyperfine benchmarks
//! (kept around incase it becomes useful)
use std::collections::HashMap;
use std::time::Duration;

use criterion::{BenchmarkId, Criterion, SamplingMode, black_box, criterion_group, criterion_main};
use prismulti::analyze::analyse_basic_model;
use prismulti::constr_symbolic::build_symbolic_dtmc;
use prismulti::parser::{parse_dtmc, parse_dtmc_props};
use prismulti::sym_check::{PropertyEvaluation, evaluate_property_at_initial_state};

fn read_fixture(path: &str) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| panic!("Failed to read '{path}': {e}"))
}

fn make_const_overrides(entries: &[(&str, &str)]) -> HashMap<String, String> {
    entries
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}

fn parse_analyze_construct(
    model_source: &str,
    const_overrides: &HashMap<String, String>,
) -> prismulti::symbolic_dtmc::SymbolicDTMC {
    let mut ast = parse_dtmc(model_source).expect("Failed to parse model");
    let info = analyse_basic_model(&mut ast, const_overrides).expect("Failed to analyze model");
    build_symbolic_dtmc(ast, info)
}

fn parse_analyze_construct_and_check(
    model_source: &str,
    prop_source: &str,
    const_overrides: &HashMap<String, String>,
    property_index: usize,
) -> f64 {
    let mut ast = parse_dtmc(model_source).expect("Failed to parse model");
    let (mut prop_constants, mut properties) =
        parse_dtmc_props(prop_source).expect("Failed to parse property file");
    ast.constants.append(&mut prop_constants);
    ast.properties.append(&mut properties);

    let info = analyse_basic_model(&mut ast, const_overrides).expect("Failed to analyze model");
    let mut dtmc = build_symbolic_dtmc(ast, info);
    let property = {
        let properties = dtmc.ast.properties.clone();
        properties[property_index].clone()
    };

    match evaluate_property_at_initial_state(&mut dtmc, &property)
        .expect("Property checking failed")
    {
        PropertyEvaluation::Probability(value) => value,
        PropertyEvaluation::Unsupported(reason) => {
            panic!("Unsupported property benchmarked: {reason}")
        }
    }
}

fn construction_benchmarks(c: &mut Criterion) {
    let brp_model = read_fixture("tests/dtmc/brp.prism");
    let leader3_2_model = read_fixture("tests/dtmc/leader3_2.prism");
    let leader4_4_model = read_fixture("tests/dtmc/leader4_4.prism");
    let leader5_6_model = read_fixture("tests/dtmc/leader5_6.prism");

    let mut brp_group = c.benchmark_group("construction/brp");
    brp_group.sample_size(10);
    brp_group.sampling_mode(SamplingMode::Flat);

    let brp_cases = vec![
        (
            "N=1,MAX=2",
            make_const_overrides(&[("N", "1"), ("MAX", "2")]),
        ),
        (
            "N=2,MAX=3",
            make_const_overrides(&[("N", "2"), ("MAX", "3")]),
        ),
    ];

    for (case_name, const_overrides) in brp_cases {
        let model_source = brp_model.as_str();
        brp_group.bench_with_input(
            BenchmarkId::new("model", case_name),
            &const_overrides,
            move |b, consts| {
                b.iter(|| {
                    let dtmc = parse_analyze_construct(model_source, consts);
                    black_box(dtmc);
                });
            },
        );
    }
    brp_group.finish();

    let mut leader_group = c.benchmark_group("construction/leader");
    leader_group.sample_size(10);
    leader_group.sampling_mode(SamplingMode::Flat);

    let no_consts = make_const_overrides(&[]);

    let leader_models = vec![
        ("leader3_2", leader3_2_model.as_str()),
        ("leader4_4", leader4_4_model.as_str()),
        ("leader5_6", leader5_6_model.as_str()),
    ];

    for (model_name, model_source) in leader_models {
        leader_group.bench_with_input(
            BenchmarkId::new("model", model_name),
            &model_source,
            |b, model_source| {
                b.iter(|| {
                    let dtmc = parse_analyze_construct(model_source, &no_consts);
                    black_box(dtmc);
                });
            },
        );
    }

    leader_group.finish();
}

fn checking_benchmarks(c: &mut Criterion) {
    let brp_model = read_fixture("tests/dtmc/brp.prism");
    let brp_props = read_fixture("tests/dtmc/brp.prop");
    let leader3_2_model = read_fixture("tests/dtmc/leader3_2.prism");
    let leader4_4_model = read_fixture("tests/dtmc/leader4_4.prism");
    let leader5_6_model = read_fixture("tests/dtmc/leader5_6.prism");
    let leader_props = read_fixture("tests/dtmc/leader.prop");

    let mut brp_group = c.benchmark_group("checking/brp");
    brp_group.sample_size(10);
    brp_group.sampling_mode(SamplingMode::Flat);

    let brp_cases = vec![(
        "N=1,MAX=2",
        make_const_overrides(&[("N", "1"), ("MAX", "2")]),
    )];

    let brp_property_indices: &[usize] = &[0, 2, 5];

    for (case_name, const_overrides) in brp_cases {
        for &property_index in brp_property_indices {
            let id = format!("{case_name}/prop{}", property_index + 1);
            let model_source = brp_model.as_str();
            let prop_source = brp_props.as_str();
            brp_group.bench_with_input(
                BenchmarkId::new("scenario", id),
                &const_overrides,
                move |b, consts| {
                    b.iter(|| {
                        let value = parse_analyze_construct_and_check(
                            model_source,
                            prop_source,
                            consts,
                            property_index,
                        );
                        black_box(value);
                    });
                },
            );
        }
    }

    brp_group.finish();

    let mut leader_group = c.benchmark_group("checking/leader");
    leader_group.sample_size(10);
    leader_group.sampling_mode(SamplingMode::Flat);

    let leader_cases = vec![
        (
            "leader3_2",
            leader3_2_model.as_str(),
            make_const_overrides(&[("L", "3")]),
        ),
        (
            "leader4_4",
            leader4_4_model.as_str(),
            make_const_overrides(&[("L", "3")]),
        ),
        (
            "leader5_6",
            leader5_6_model.as_str(),
            make_const_overrides(&[("L", "3")]),
        ),
    ];

    for (case_name, model_source, const_overrides) in leader_cases {
        for property_index in [0usize, 1usize] {
            let id = format!("{case_name}/prop{}", property_index + 1);
            let prop_source = leader_props.as_str();
            leader_group.bench_with_input(
                BenchmarkId::new("scenario", id),
                &const_overrides,
                move |b, consts| {
                    b.iter(|| {
                        let value = parse_analyze_construct_and_check(
                            model_source,
                            prop_source,
                            consts,
                            property_index,
                        );
                        black_box(value);
                    });
                },
            );
        }
    }

    leader_group.finish();
}

fn criterion_config() -> Criterion {
    let target_secs = std::env::var("PRISM_BENCH_TARGET_SECS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok());
    let warmup_secs = std::env::var("PRISM_BENCH_WARMUP_SECS")
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok());

    if let Some(target_secs) = target_secs {
        let warmup_secs = warmup_secs.unwrap_or(target_secs.max(1) / 2).max(1);
        return Criterion::default()
            .sample_size(30)
            .warm_up_time(Duration::from_secs(warmup_secs))
            .measurement_time(Duration::from_secs(target_secs.max(1)));
    }

    Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(2))
}

criterion_group! {
    name = dtmc_benches;
    config = criterion_config();
    targets = construction_benchmarks, checking_benchmarks
}
criterion_main!(dtmc_benches);

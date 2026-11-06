use std::{collections::BTreeSet, fs, path::Path};

const MATRIX: &str = include_str!("../docs/test-matrix.md");
const SPEC: &str = include_str!("../docs/plano-tecnico-api-assinaturas-rust.md");
const PHASES: &str = include_str!("../docs/fases-implementacao.md");

#[test]
fn matrix_tracks_every_normative_scenario_and_phase_exit() {
    for section in [7, 9] {
        let heading = format!("## {section}. ");
        let content = SPEC.split(&heading).nth(1).expect("normative section");
        for requirement in content
            .split("\n## ")
            .next()
            .unwrap()
            .lines()
            .filter_map(|line| line.strip_prefix("- "))
        {
            let escaped = requirement.replace('|', "&#124;");
            assert_eq!(
                MATRIX
                    .lines()
                    .filter(|line| line.starts_with('|') && line.contains(&escaped))
                    .count(),
                1,
                "scenario must have exactly one traceable row: {requirement}"
            );
        }
    }
    for phase in PHASES.split("## Fase ").skip(1) {
        let number = phase.split(' ').next().unwrap().parse::<u8>().unwrap();
        let exit = phase
            .split("**Critério de saída:** ")
            .nth(1)
            .unwrap()
            .split("\n\n")
            .next()
            .unwrap()
            .replace('\n', " ");
        assert!(
            MATRIX.contains(&format!("| EX-{number:02} |")) && MATRIX.contains(&exit),
            "missing phase {number} exit"
        );
    }
}

#[test]
fn matrix_passing_rows_reference_real_automated_tests_without_wildcards() {
    let mut sources = Vec::new();
    collect_rust_sources(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src").as_path(),
        &mut sources,
    );
    collect_rust_sources(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .as_path(),
        &mut sources,
    );
    let mut identifiers = BTreeSet::new();
    for row in MATRIX.lines().filter(|line| line.starts_with("| ")) {
        let fields: Vec<&str> = row.trim_matches('|').split(" | ").map(str::trim).collect();
        if fields[0] == "ID" {
            continue;
        }
        assert!(
            identifiers.insert(fields[0]),
            "duplicate scenario ID: {}",
            fields[0]
        );
        if fields.last() != Some(&"passing") {
            continue;
        }
        let names: Vec<&str> = fields[3]
            .split('`')
            .enumerate()
            .filter_map(|(i, part)| (i % 2 == 1).then_some(part))
            .collect();
        assert!(!names.is_empty(), "passing row without test: {}", fields[0]);
        for name in names {
            assert!(
                sources.iter().any(|source| declares_test(source, name)),
                "{} references missing or non-test function {name}",
                fields[0]
            );
        }
    }
}

fn declares_test(source: &str, name: &str) -> bool {
    let Some((before, _)) = source.split_once(&format!("fn {name}(")) else {
        return false;
    };
    let attributes = before.rsplit('}').next().unwrap();
    (attributes.contains("#[test]")
        || attributes.contains("#[tokio::test(")
        || attributes.contains("#[tokio::test]"))
        && !attributes.contains("#[ignore")
}

#[test]
fn phase_gate_checks_all_accumulated_scenarios() {
    for phase in 0..=10 {
        let blocked = MATRIX
            .lines()
            .filter(|line| line.starts_with("| "))
            .any(|line| {
                let fields: Vec<&str> = line.split('|').map(str::trim).collect();
                let responsible = fields[fields.len() - 3].parse::<u8>();
                responsible.is_ok_and(|value| value <= phase)
                    && fields[fields.len() - 2] != "passing"
            });
        let outcome = std::process::Command::new("bash")
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args([
                "scripts/check-phase-gate.sh",
                &phase.to_string(),
                "--check-only",
            ])
            .output()
            .expect("phase gate process");
        assert_eq!(
            outcome.status.code(),
            Some(i32::from(blocked)),
            "phase {phase}: {}",
            String::from_utf8_lossy(&outcome.stderr)
        );
    }
}

fn collect_rust_sources(directory: &Path, sources: &mut Vec<String>) {
    for entry in fs::read_dir(directory).expect("repository source directory") {
        let path = entry.expect("source entry").path();
        if path.is_dir() {
            collect_rust_sources(&path, sources);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            sources.push(fs::read_to_string(path).expect("Rust source"));
        }
    }
}

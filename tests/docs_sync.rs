//! Doc-sync checks: make documentation drift a `cargo test` failure.
//!
//! When these tests fail, update the DOC (AGENTS.md structure / TODO.md test
//! counts / README doc links) — not this file. AGENTS.md Step 9 (Update
//! Documentation) is the workflow that keeps them green.

use std::fs;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("cannot read {rel}: {e}"))
}

/// Modules declared in the crate root (`mod foo;`).
fn declared_modules() -> Vec<String> {
    read("src/main.rs")
        .lines()
        .filter_map(|l| {
            let t = l.trim();
            t.strip_prefix("mod ")?.strip_suffix(';').map(str::to_owned)
        })
        .collect()
}

fn count_lines_containing(dir: &Path, needle: &str) -> usize {
    let mut count = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "rs") {
                count += fs::read_to_string(&p)
                    .unwrap()
                    .lines()
                    .filter(|l| l.trim_start().starts_with(needle))
                    .count();
            }
        }
    }
    count
}

/// Extract the integer immediately before `marker` (e.g. "12 inline unit tests" → 12).
fn leading_int(text: &str, marker: &str) -> usize {
    let idx = text
        .find(marker)
        .unwrap_or_else(|| panic!("doc missing marker '{marker}'"));
    let digits: String = text[..idx]
        .chars()
        .rev()
        .skip_while(|c| c.is_whitespace())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits
        .chars()
        .rev()
        .collect::<String>()
        .parse()
        .unwrap_or_else(|_| panic!("no integer before '{marker}'"))
}

/// A module counts as "documented" in prose docs when its name appears in a
/// recognisable reference form: `name.rs`, `name::`, or `name` in backticks.
fn documented_in(module: &str, doc: &str) -> bool {
    doc.contains(&format!("{module}.rs"))
        || doc.contains(&format!("{module}::"))
        || doc.contains(&format!("`{module}`"))
}

#[test]
fn agents_md_structure_lists_every_module() {
    // AGENTS.md is gitignored (agent-local), so it may be absent in a fresh
    // clone. Skip rather than fail when it isn't there.
    let Ok(agents) = fs::read_to_string(root().join("AGENTS.md")) else {
        return;
    };
    // A module is either a file (`foo.rs`) or a directory of files
    // (`foo/mod.rs`, `foo/expr.rs`, ...). Either form counts as documented.
    let missing: Vec<String> = declared_modules()
        .into_iter()
        .filter(|m| {
            !(agents.contains(&format!("{m}.rs")) || agents.contains(&format!("{m}/")))
        })
        .collect();
    assert!(
        missing.is_empty(),
        "modules missing from the AGENTS.md project structure: {missing:?}"
    );
}

#[test]
fn architecture_md_mentions_every_module() {
    let arch = read("ARCHITECTURE.md");
    let missing: Vec<String> = declared_modules()
        .into_iter()
        .filter(|m| !(documented_in(m, &arch) || arch.contains(&format!("{m}/"))))
        .collect();
    assert!(
        missing.is_empty(),
        "modules missing from ARCHITECTURE.md: {missing:?}"
    );
}

#[test]
fn todo_md_inline_unit_test_count_matches_code() {
    let actual = count_lines_containing(&root().join("src"), "#[test]");
    let claimed = leading_int(&read("TODO.md"), "inline unit tests");
    assert_eq!(
        claimed, actual,
        "TODO.md unit-test count is stale (run `cargo test`, then update the Done section)"
    );
}

#[test]
fn todo_md_integration_test_count_matches_code() {
    // Counts every test attribute in tests/, including this file — the claim in
    // TODO.md must match what `cargo test` actually reports.
    let actual = count_lines_containing(&root().join("tests"), "#[test]");
    let claimed = leading_int(&read("TODO.md"), "integration tests");
    assert_eq!(
        claimed, actual,
        "TODO.md integration-test count is stale (run `cargo test`, then update the Done section)"
    );
}

#[test]
fn readme_links_every_root_doc() {
    let readme = read("README.md");
    let missing: Vec<&str> = [
        "AGENTS.md",
        "ARCHITECTURE.md",
        "MEMORY.md",
        "SPEC.md",
        "TODO.md",
    ]
    .into_iter()
    .filter(|d| !readme.contains(d))
    .collect();
    assert!(
        missing.is_empty(),
        "README.md does not link the root docs: {missing:?}"
    );
}

/// Every cell of the README's benchmark table must match the committed report.
///
/// The README summarises the generated report in a table. Each row comes
/// from one workload's section of `bench/RESULTS.md`, so the values can be
/// compared — allowing for the README rounding to fewer digits.
///
/// This is the check that was missing: doc-sync verified that the docs
/// *exist* and that the counts match, but nothing verified that a number in
/// the prose was true. The README once quoted a benchmark table from a run
/// that predated a commit which changed the generated code.
#[test]
fn readme_benchmark_table_matches_the_report() {
    let readme = read("README.md");
    let results = read("bench/RESULTS.md");

    // The report's sections, each a list of rows of cells.
    let mut sections: Vec<(String, Vec<Vec<String>>)> = Vec::new();
    let mut current: Option<(String, Vec<Vec<String>>)> = None;
    for line in results.lines() {
        if let Some(name) = line.strip_prefix("## ") {
            if let Some(sec) = current.take() {
                sections.push(sec);
            }
            current = Some((name.trim().to_string(), Vec::new()));
            continue;
        }
        if let Some((_, rows)) = current.as_mut()
            && line.starts_with("| ")
            && !line.contains("---")
        {
            let cells: Vec<String> = line
                .trim_matches('|')
                .split('|')
                .map(|c| c.trim().to_string())
                .collect();
            if cells.first().map(|c| c.as_str()) != Some("Toolchain")
                && cells.first().map(|c| c.as_str()) != Some("Variant")
            {
                rows.push(cells);
            }
        }
    }
    if let Some(sec) = current.take() {
        sections.push(sec);
    }

    // Which report section and column each README row comes from. The label
    // is the cell's text, without the bold markers.
    let rows: Vec<(&str, &str, usize)> = vec![
        ("Artifact size", "fib", 3),
        ("Startup", "startup", 4),
        ("Compile time", "startup", 2),
        ("fib(35) runtime", "fib", 4),
        ("fib(35) unrewritten", "fib_recursive", 4),
        ("loop(1e8) runtime", "loop", 4),
    ];
    let columns = [
        "compiler (current)",
        "scriptc",
        "compiler v0.1.0 (published)",
        "deno compile",
        "bun build --compile",
        "tsrust + node",
        "tsc + node",
    ];

    // The README's benchmark table, as (label, cells) pairs.
    let table: Vec<(String, Vec<String>)> = readme
        .lines()
        .filter(|l| l.starts_with("| "))
        .map(|l| {
            let cells: Vec<String> = l
                .trim_matches('|')
                .split('|')
                .map(|c| c.trim().to_string())
                .collect();
            (
                cells
                    .first()
                    .map(|c| c.trim_matches('*').to_string())
                    .unwrap_or_default(),
                cells,
            )
        })
        .filter(|(label, _)| rows.iter().any(|(l, _, _)| l == label))
        .collect();

    let mut problems: Vec<String> = Vec::new();
    for (label, section, index) in rows {
        let Some((_, cells)) = table.iter().find(|(l, _)| l == label) else {
            problems.push(format!("README.md has no {label} row"));
            continue;
        };
        let cells: Vec<String> = cells
            .iter()
            .map(|c| c.trim_matches('*').replace(" JS", ""))
            .collect();

        for (i, toolchain) in columns.iter().enumerate() {
            let quoted = cells.get(i + 1).cloned().unwrap_or_default();
            let reported = sections
                .iter()
                .find(|(name, _)| name == section)
                .and_then(|(_, rows)| {
                    rows.iter()
                        .find(|cells| cells.first().is_some_and(|c| c.starts_with(toolchain)))
                })
                .and_then(|cells| cells.get(index))
                .cloned();
            let Some(reported) = reported else {
                problems.push(format!("{label} / {toolchain}: not found in the {section} section"));
                continue;
            };
            match compare_measurements(&quoted, &reported) {
                Ok(()) => {}
                Err(why) => problems.push(format!("{label} / {toolchain}: {why}")),
            }
        }
    }

    assert!(
        problems.is_empty(),
        "README.md's benchmark table does not match bench/RESULTS.md:\n  {}\n\
         — regenerate the report with `python3 scripts/bench.py` and update the README",
        problems.join("\n  ")
    );
}

/// Compare a figure the README quotes with the one in the report.
///
/// The README rounds to fewer digits, so the comparison allows half a unit of
/// the README's last digit — the same tolerance as correct rounding.
fn compare_measurements(quoted: &str, reported: &str) -> Result<(), String> {
    // Split "108.9 KiB" into (108.9, "KiB") and remember how many digits
    // the number was written with.
    let parse = |s: &str| -> Option<(f64, String, usize)> {
        let s = s.trim();
        let mut end = 0;
        for (i, c) in s.char_indices() {
            if c.is_ascii_digit() || c == '.' {
                end = i + c.len_utf8();
            } else {
                break;
            }
        }
        if end == 0 {
            return None;
        }
        let number = s[..end].parse::<f64>().ok()?;
        let unit = s[end..].trim().to_string();
        let decimals = s[..end]
            .split_once('.')
            .map(|(_, frac)| frac.len())
            .unwrap_or(0);
        Some((number, unit, decimals))
    };
    let (q_num, q_unit, q_decimals) =
        parse(quoted).ok_or_else(|| format!("{quoted:?} is not a figure"))?;
    let (r_num, r_unit, _) =
        parse(reported).ok_or_else(|| format!("{reported:?} is not a figure"))?;
    if q_unit != r_unit {
        return Err(format!("{quoted:?} and {reported:?} have different units"));
    }
    let tolerance = 0.5 * 10f64.powi(-(q_decimals as i32)) + 1e-9;
    if (q_num - r_num).abs() > tolerance {
        return Err(format!("{quoted:?} vs {reported:?}"));
    }
    Ok(())
}

/// The README's benchmark figures must come from the committed report.
///
/// The README summarises `bench/RESULTS.md` in prose and quotes the run it
/// came from — the host's load average and the process-spawn floor. Those two
/// numbers identify the run, so if the report is regenerated and the README
/// is not updated, the two no longer agree and this fails.
#[test]
fn readme_benchmark_figures_come_from_the_committed_report() {
    let readme = read("README.md");
    let results = read("bench/RESULTS.md");

    // The report records the run's load average and spawn floor.
    let load = results
        .lines()
        .find_map(|l| l.strip_prefix("- Machine load average when the run started: "))
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_else(|| panic!("bench/RESULTS.md has no load average line"));
    let floor = results
        .lines()
        .find_map(|l| l.strip_prefix("- Process spawn overhead (included in every runtime figure): "))
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_else(|| panic!("bench/RESULTS.md has no spawn-overhead line"));

    // The README says which run its figures came from. The two numbers are
    // in one sentence, which may be wrapped across lines — flatten the
    // whitespace first.
    let flattened = readme.split_whitespace().collect::<Vec<_>>().join(" ");
    let sentence = flattened
        .split("average of ")
        .nth(1)
        .and_then(|rest| rest.split(". ").next())
        .unwrap_or_else(|| panic!("README.md does not say which run its figures came from"));
    let quoted_load = sentence
        .split_whitespace()
        .next()
        .unwrap_or_else(|| panic!("README.md quotes no load average"));
    let quoted_floor = sentence
        .split(" and a ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap_or_else(|| panic!("README.md quotes no spawn floor"));

    assert_eq!(
        quoted_load, load,
        "README.md quotes a load average of {quoted_load} but bench/RESULTS.md says {load} \
         — the report was regenerated; update the README's benchmark section"
    );
    // The report writes the floor with the unit attached ("17.72ms"); the
    // README writes it as a number with a space ("17.7 ms"). Compare the
    // numbers, not the formatting.
    let floor_num: f64 = floor.trim_end_matches("ms").parse().unwrap_or_else(|_| {
        panic!("bench/RESULTS.md has an unparseable spawn floor: {floor}")
    });
    let quoted_floor_num: f64 = quoted_floor
        .trim_end_matches("ms")
        .parse()
        .unwrap_or_else(|_| panic!("README.md has an unparseable spawn floor: {quoted_floor}"));
    assert!(
        (floor_num - quoted_floor_num).abs() < 0.05,
        "README.md quotes a {quoted_floor} ms spawn floor but bench/RESULTS.md says {floor} \
         — the report was regenerated; update the README's benchmark section"
    );
}

/// Every `**Nx**` factor the README quotes must appear in the generated
/// benchmark report.
///
/// The README's benchmark prose is the one place where a number can be
/// checked against something measured rather than remembered:
/// `bench/RESULTS.md` is generated by `scripts/bench.py`, so it cannot drift
/// from reality the way a hand-written figure can.
#[test]
fn readme_benchmark_factors_come_from_the_report() {
    let readme = read("README.md");
    let results = read("bench/RESULTS.md");

    // The factors the README states in bold: `**1.52x**`, `**1.01x slower**`.
    let mut quoted: Vec<String> = Vec::new();
    let mut rest = readme.as_str();
    while let Some(start) = rest.find("**") {
        rest = &rest[start + 2..];
        let Some(end) = rest.find("**") else { break };
        let inner = &rest[..end];
        rest = &rest[end + 2..];
        // Only the speedup factors: `**1.52x**` and `**1.01x slower**`.
        if let Some(num) = inner.strip_suffix("x") {
            quoted.push(num.to_string());
        } else if let Some(num) = inner.strip_suffix("x slower") {
            quoted.push(num.to_string());
        } else if let Some(num) = inner.strip_suffix("x faster") {
            quoted.push(num.to_string());
        }
    }

    assert!(
        !quoted.is_empty(),
        "no speedup factors found in README.md — the parser needs updating"
    );
    let missing: Vec<&str> = quoted
        .iter()
        .map(String::as_str)
        .filter(|f| !results.contains(&format!("{f}x")))
        .collect();
    assert!(
        missing.is_empty(),
        "README.md quotes speedup factors that are not in bench/RESULTS.md: {missing:?} \
         — regenerate the report with `python3 scripts/bench.py` and update the README"
    );
}

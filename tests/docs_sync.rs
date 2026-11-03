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

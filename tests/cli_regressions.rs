use serde_json::Value;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "surc-cli-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn write(&self, relative: &str, content: &str) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_surc"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }

    fn ok(&self, args: &[&str]) -> String {
        let output = self.run(args);
        assert!(output.status.success(), "{args:?}: {output:?}");
        String::from_utf8(output.stdout).unwrap()
    }

    fn rejects(&self, args: &[&str], diagnostic: &str) {
        let output = self.run(args);
        assert!(!output.status.success(), "{args:?}: {output:?}");
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(combined.contains(diagnostic), "{combined}");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn manifest_checks_package_assignments_and_resolves_cross_file_references() {
    let f = Fixture::new();
    f.write(
        "surv.toml",
        r#"
[project]
name = "test"
[paths]
ir_root = "ir"
[packages.common]
root = "ir/common"
namespace = "app.common"
[packages.app]
root = "ir/app"
depends = ["common"]
"#,
    );
    f.write(
        "ir/common/shared.toml",
        "[schema.Id]\nkind = \"node\"\n[mod.shared]\npurpose = \"Common\"\n",
    );
    let valid = r#"
import = ["common as util"]
require = ["app.common.mod.shared"]
[func.load]
input = ["util.schema.Id", "app.common.schema.Id", "common.schema.Id", "schema.Id"]

[mod.app]
funcs = ["func.load"]
pipeline = ["func.load"]
"#;
    f.write("ir/app/main.toml", valid);
    f.ok(&["check", "surv.toml"]);
    f.write(
        "ir/common/duplicate.toml",
        "[schema.Id]\nkind = \"value\"\ntype = \"DuplicateId\"\n",
    );
    let duplicate = f.run(&["check", "surv.toml"]);
    assert!(!duplicate.status.success(), "{duplicate:?}");
    let duplicate = String::from_utf8_lossy(&duplicate.stdout);
    assert!(duplicate.contains("W_AMBIGUOUS_NAME"), "{duplicate}");
    assert!(duplicate.contains("E_AMBIGUOUS_NAME"), "{duplicate}");
    fs::remove_file(f.0.join("ir/common/duplicate.toml")).unwrap();
    f.write(
        "ir/app/main.toml",
        &valid.replace("util.schema.Id", "util.schema.Missing"),
    );
    f.rejects(&["check", "surv.toml"], "E_UNDEFINED_SCHEMA");
    f.write(
        "ir/app/main.toml",
        &format!("package = \"unknown\"\n{valid}"),
    );
    f.rejects(&["project-check", "surv.toml"], "E_PACKAGE_UNKNOWN");
    f.write(
        "ir/app/main.toml",
        &valid.replace("app.common.mod.shared", "mod.app"),
    );
    f.rejects(&["check", "surv.toml"], "RequireCycle");
    f.write("ir/app/main.toml", valid);
    let manifest = fs::read_to_string(f.0.join("surv.toml")).unwrap();
    f.write(
        "surv.toml",
        &manifest.replace("depends = [\"common\"]", "depends = []"),
    );
    f.rejects(&["check", "surv.toml"], "E_UNDECLARED_DEPENDENCY");
}

#[test]
fn check_rejects_unterminated_values_even_in_legacy_syntax() {
    let f = Fixture::new();
    for invalid in [
        "purpose = \"\"\"unfinished",
        "purpose = \"unfinished",
        "funcs = [\"func.run\"",
    ] {
        f.write("ir.toml", &format!("[mod.api]\n{invalid}\n"));
        f.rejects(&["check", "ir.toml"], "unterminated");
        f.rejects(&["check", "ir.toml", "--ast"], "unterminated");
    }
    f.write("ir.toml", "[func.run]\n[mod.api]\nfuncs = {\"func.run\"}\n");
    f.ok(&["check", "ir.toml"]);
}

#[test]
fn status_set_round_trips_notes_and_nested_modules() {
    let f = Fixture::new();
    for funcs in ["[]", "{\"func.greet\"}"] {
        let before = format!(
            "[func.greet]\n[mod.api.child]\nfuncs = {funcs}\n[status]\nupdated_at=\"2026-01-01\"\n[status.mod.api.child]\nstate=\"todo\"\nnotes=\"old\" # retained\n"
        );
        f.write("ir.toml", &before);
        let notes = "cost $name \"quoted\" \\ path\nnew line";
        f.ok(&[
            "status",
            "set",
            "mod.api.child",
            "ir.toml",
            "--notes",
            notes,
        ]);
        let content = fs::read_to_string(f.0.join("ir.toml")).unwrap();
        assert!(content.contains(&format!("funcs = {funcs}")));
        assert!(content.contains("# retained"));
        let ast = f.ok(&["check", "ir.toml", "--ast"]);
        let ast: Value = serde_json::from_str(&ast).unwrap();
        let status = ast["sections"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|section| section.get("Status"))
            .unwrap();
        assert_eq!(status["modules"]["api.child"]["notes"], notes);
    }
}

#[test]
fn github_actions_preserves_indentation_of_multiline_commands() {
    let f = Fixture::new();
    f.write(
        "deploy.toml",
        "[deploy.job.hello]\nruns = [\"  echo one\\necho two\"]\n",
    );
    let yaml = f.ok(&["codegen", "github-actions", "deploy.toml"]);
    assert!(yaml.contains("        run: |2-\n            echo one\n          echo two\n"));
}

#[test]
fn diff_compares_known_empty_and_nonempty_structs() {
    let f = Fixture::new();
    f.write(
        "ir.toml",
        "[schema.User]\nkind=\"node\"\nfields={id=\"uuid\"}\n",
    );
    f.write("src/lib.rs", "pub struct User {}\n");
    let args = ["diff", "ir.toml", ".", "--lang", "rust", "--format", "json"];
    let output = f.run(&args);
    let json: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["summary"]["schema_field_mismatches"], 1);
    assert!(String::from_utf8_lossy(&output.stdout).contains("missing implementation field 'id'"));

    f.write("ir.toml", "[schema.User]\nkind=\"node\"\nfields={}\n");
    let json: Value = serde_json::from_str(&f.ok(&args)).unwrap();
    assert_eq!(json["summary"]["schema_field_mismatches"], 0);
}

#[test]
fn pipeline_checks_membership_and_sequence_without_joining_sibling_steps() {
    let f = Fixture::new();
    let ir = r#"
[schema.X]
kind = "node"
[schema.Y]
kind = "node"
[func.first]
input = ["schema.X"]
output = ["schema.Y"]
[func.other]
input = ["schema.X"]
output = ["schema.Y"]
[func.last]
input = ["schema.Y"]
[mod.api]
funcs = ["func.first", "func.other", "func.last"]
"#;
    for step in [
        r#"{ branch = { condition = "ok", on_true = "func.first", on_false = "func.first" } }"#,
        r#"{ parallel = ["func.first", "func.other"] }"#,
    ] {
        f.write(
            "ir.toml",
            &format!("{ir}pipeline = [{step}, \"func.last\"]\n"),
        );
        assert!(f.ok(&["check", "ir.toml"]).contains("No issues found"));
    }
    f.write(
        "ir.toml",
        &format!("{ir}pipeline = [\"func.first\", \"func.other\"]\n"),
    );
    assert!(f.ok(&["check", "ir.toml"]).contains("PipelineTypeMismatch"));

    f.write(
        "ir.toml",
        &format!("{ir}pipeline = [\"func.first\", \"func.first\"]\n"),
    );
    f.rejects(&["check", "ir.toml"], "PipelineCycle");

    f.write(
        "ir.toml",
        &format!(
            "{}pipeline = [\"func.first\"]\n",
            ir.replace("\"func.first\", ", "")
        ),
    );
    f.rejects(&["check", "ir.toml"], "PipelineFuncNotInModule");
}

#[test]
fn module_boundary_is_preserved_in_ast_and_slice() {
    let f = Fixture::new();
    f.write(
        "ir.toml",
        "[mod.api]\npurpose = \"HTTP\"\nboundary = {http = [\"GET /\"]}\n",
    );
    let ast = f.ok(&["check", "ir.toml", "--ast"]);
    let json: Value = serde_json::from_str(&ast).unwrap();
    assert_eq!(json["sections"].as_array().unwrap().len(), 1);
    assert_eq!(json["sections"][0]["Mod"]["boundary"]["http"][0], "GET /");
    let slice = f.ok(&["slice", "mod.api", "ir.toml", "--with-defs"]);
    assert!(slice.contains("boundary = {"));
    assert!(slice.contains("GET /"));
}

#[cfg(unix)]
#[test]
fn init_ir_scans_both_lsp_languages_without_existing_ir() {
    use std::os::unix::fs::PermissionsExt;

    let f = Fixture::new();
    let server = r#"#!/bin/sh
read_request() {
    IFS= read -r header &&
    IFS= read -r separator &&
    length=${header#Content-Length: } &&
    length=${length%?} &&
    message=$(/bin/dd bs=1 count="$length" 2>/dev/null)
}
respond() {
    printf 'Content-Length: %s\r\n\r\n%s' "${#1}" "$1"
}
read_request || exit 1
respond '{"jsonrpc":"2.0","id":1,"result":{"capabilities":{"workspaceSymbolProvider":true}}}'
read_request || exit 1
read_request || exit 1
case "$message" in
    *workspace/symbol*) ;;
    *) exit 1 ;;
esac
case "$0" in
    *rust-analyzer) name=from_rust ;;
    *) name=from_typescript ;;
esac
respond '{"jsonrpc":"2.0","id":2,"result":[{"name":"'"$name"'","kind":12,"location":{"uri":"file://'"$PWD"'/src/lib.rs","range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}}}}]}'
read_request || exit 1
respond '{"jsonrpc":"2.0","id":3,"result":null}'
read_request
"#;
    for name in ["rust-analyzer", "typescript-language-server"] {
        f.write(name, server);
        fs::set_permissions(f.0.join(name), fs::Permissions::from_mode(0o755)).unwrap();
    }
    let output = Command::new(env!("CARGO_BIN_EXE_surc"))
        .current_dir(&f.0)
        .env("PATH", &f.0)
        .args(["init-ir", ".", "--strategy", "lsp"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let ir = String::from_utf8(output.stdout).unwrap();
    assert!(ir.contains("[func.from_rust]"), "{ir}");
    assert!(ir.contains("[func.from_typescript]"), "{ir}");

    f.write(
        "design.toml",
        "[schema.Input]\nkind = \"node\"\n[func.from_rust]\ninput=[\"schema.Input\"]\n[mod.api]\nfuncs=[\"func.from_rust\"]\n",
    );
    f.write("src/lib.rs", "pub fn from_rust() {}\n");
    let plan = |path: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_surc"))
            .current_dir(&f.0)
            .env("PATH", path)
            .args([
                "plan",
                "design.toml",
                ".",
                "--lang",
                "rust",
                "--strategy",
                "lsp",
                "--format",
                "json",
            ])
            .output()
            .unwrap()
    };
    let output = plan(&f.0);
    assert!(output.status.success(), "{output:?}");
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["drifted_symbols"], 1, "{report}");
    let output = plan(&f.0.join("no-language-server"));
    assert_eq!(output.status.code(), Some(1));
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["diagnostics"][0]["kind"], "PlanOverlayFailed");
}

fn plan_json(fixture: &Fixture, args: &[&str], status: &str) -> Value {
    let mut command = vec!["plan"];
    command.extend_from_slice(args);
    command.extend_from_slice(&["--format", "json"]);
    let result = fixture.run(&command);
    assert_eq!(
        result.status.code(),
        Some(if status == "OK" { 0 } else { 1 }),
        "{result:?}"
    );
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["status"], status, "{report}");
    report
}

fn planned_unit<'a>(report: &'a Value, reference: &str) -> &'a Value {
    report["waves"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|wave| wave["units"].as_array().unwrap())
        .find(|task| {
            task["unit"]["member_nodes"]
                .as_array()
                .unwrap()
                .iter()
                .any(|node| node["display_ref"] == reference)
        })
        .unwrap_or_else(|| panic!("{reference} missing from plan"))
}

#[test]
fn plan_condenses_recursive_schemas_and_uses_soft_hints_for_parallel_tracks() {
    let f = Fixture::new();
    f.write(
        "design.toml",
        r#"
[schema.Expr]
kind = "node"
fields = { child = "schema.Expr?", stmt = "Option<Stmt>" }
[schema.Stmt]
kind = "node"
fields = { expr = "schema.Expr" }
[schema.Unrelated]
kind = "node"
[func.zFirst]
input = ["schema.Expr"]
output = ["schema.Stmt"]
[func.aLast]
input = ["schema.Stmt"]
[mod.selected]
schemas = ["schema.Expr", "schema.Stmt"]
funcs = ["func.zFirst", "func.aLast"]
pipeline = ["func.zFirst -> func.aLast"]
[mod.other]
schemas = ["schema.Unrelated"]
"#,
    );
    let report = plan_json(&f, &["design.toml", "--mod", "mod.selected"], "OK");
    assert_eq!(report["workspace_checked"], false);
    assert_eq!(report["total_symbols"], 4);
    let types = planned_unit(&report, "schema.Expr");
    assert_eq!(types["unit"]["unit_kind"], "schema_scc");
    assert_eq!(types["unit"]["member_nodes"].as_array().unwrap().len(), 2);
    assert_eq!(types["wave_index"], 1);
    assert_eq!(planned_unit(&report, "func.zFirst")["wave_index"], 2);
    assert_eq!(planned_unit(&report, "func.aLast")["wave_index"], 2);
    assert_eq!(planned_unit(&report, "mod.selected")["wave_index"], 3);
    let track = &report["parallel_tracks"][0]["ordered_units"];
    assert_eq!(
        track[1]["unit"]["member_nodes"][0]["display_ref"],
        "func.zFirst"
    );
    assert_eq!(
        track[2]["unit"]["member_nodes"][0]["display_ref"],
        "func.aLast"
    );
    let md = f.ok(&["plan", "design.toml", "--format", "md"]);
    assert!(md.starts_with("# Plan for design"));
    assert!(md.contains("## Parallel tracks"));
}

#[test]
fn plan_workspace_overlay_and_status_sync_preserve_notes_and_manual_blockers() {
    let f = Fixture::new();
    f.write(
        "design.toml",
        r#"
[schema.User]
kind = "node"
fields = { id = "string" }
[schema.Drift]
kind = "node"
fields = { id = "string" }
[schema.Web]
kind = "node"
impl.lang = "ts"
[func.consume]
input = ["schema.User"]
[func.wait]
input = ["schema.Drift"]
[mod.api]
submods = ["worker"]
schemas = ["schema.User", "schema.Web"]
funcs = ["func.consume"]
[mod.api.worker]
schemas = ["schema.Drift"]
funcs = ["func.wait"]
[status]
updated_at = "2000-01-01"
[status.mod.api.worker]
state = "blocked"
notes = "Keep this" # keep comment
"#,
    );
    f.write(
        "src/lib.rs",
        "pub struct User { pub id: String }\npub struct Drift {}\n",
    );
    let args = ["design.toml", ".", "--lang", "rust", "--sync-status"];
    let report = plan_json(&f, &args, "OK");
    assert_eq!(report["workspace_checked"], true);
    assert_eq!(report["total_symbols"], 4);
    assert_eq!(report["done_symbols"], 1);
    assert_eq!(report["drifted_symbols"], 1);
    assert_eq!(report["status_files_updated"][0], "design.toml");
    assert_eq!(planned_unit(&report, "schema.User")["wave_index"], 0);
    assert_eq!(
        planned_unit(&report, "schema.Web")["readiness"],
        "out_of_scope"
    );
    assert_eq!(planned_unit(&report, "func.wait")["readiness"], "blocked");
    assert_eq!(
        planned_unit(&report, "mod.api.worker")["readiness"],
        "blocked"
    );
    let updated = fs::read_to_string(f.0.join("design.toml")).unwrap();
    assert!(updated.contains("notes = \"Keep this\" # keep comment"));
    let doc: toml::Value = toml::from_str(&updated).unwrap();
    assert_eq!(
        doc["status"]["mod"]["api"]["state"].as_str(),
        Some("skeleton")
    );
    assert_eq!(
        doc["status"]["mod"]["api"]["coverage"].as_float(),
        Some(0.5)
    );
    assert_eq!(
        doc["status"]["mod"]["api"]["worker"]["state"].as_str(),
        Some("blocked")
    );
    assert!(plan_json(&f, &args, "OK")["status_files_updated"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn plan_keeps_equal_symbol_names_separate_in_projects_and_scoped_plans() {
    let f = Fixture::new();
    f.write(
        "surv.toml",
        r#"
[project]
name = "example"
[paths]
ir_root = "ir"
[packages.first]
root = "ir/first"
namespace = "app.first"
depends = ["second"]
[packages.second]
root = "ir/second"
namespace = "app.second"
"#,
    );
    f.write(
        "ir/first/a.toml",
        r#"
import = ["second as peer"]
require = ["peer.mod.support"]
[schema.Error]
kind = "node"
fields = { message = "string" }
[mod.main]
schemas = ["schema.Error"]
"#,
    );
    f.write(
        "ir/second/b.toml",
        r#"
[schema.Error]
kind = "node"
fields = { code = "int" }
[mod.support]
schemas = ["schema.Error"]
"#,
    );
    f.write("src/first.rs", "pub struct Error { pub message: String }\n");
    f.write("src/second.rs", "pub struct Error {}\n");
    let report = plan_json(
        &f,
        &[
            "surv.toml",
            ".",
            "--lang",
            "rust",
            "--mod",
            "app.first.mod.main",
        ],
        "OK",
    );
    let statuses = report["node_statuses"].as_array().unwrap();
    let first = statuses
        .iter()
        .find(|entry| entry["fq_name"] == "pkg.first.schema.app.first.Error")
        .unwrap();
    let second = statuses
        .iter()
        .find(|entry| entry["fq_name"] == "pkg.second.schema.app.second.Error")
        .unwrap();
    assert_eq!(first["impl_state"], "done");
    assert_eq!(second["impl_state"], "drifted");
    assert_eq!(planned_unit(&report, "mod.main")["wave_index"], 3);
    fs::remove_file(f.0.join("src/second.rs")).unwrap();
    let only_second = plan_json(
        &f,
        &["surv.toml", ".", "--lang", "rust", "--mod", "support"],
        "OK",
    );
    assert_eq!(only_second["done_symbols"], 0);
    assert!(only_second["node_statuses"]
        .as_array()
        .unwrap()
        .iter()
        .any(
            |entry| entry["fq_name"] == "pkg.second.schema.app.second.Error"
                && entry["impl_state"] == "missing"
        ));
}

#[test]
fn plan_rejects_invalid_input_cycles_overlay_and_sync_failures() {
    let f = Fixture::new();
    f.write("design.toml", "[mod.api]\n");
    let report = plan_json(&f, &["design.toml", "--sync-status"], "ERROR");
    assert_eq!(report["diagnostics"][0]["kind"], "InvalidPlanRequest");
    let report = plan_json(&f, &["design.toml", "nonexistent"], "ERROR");
    assert_eq!(report["diagnostics"][0]["kind"], "PlanOverlayFailed");
    f.write("invalid-map.toml", "[unfinished\n");
    let report = plan_json(
        &f,
        &["design.toml", ".", "--mapping", "invalid-map.toml"],
        "ERROR",
    );
    assert_eq!(report["diagnostics"][0]["kind"], "PlanOverlayFailed");
    f.write("design.toml", "[mod.api]\nfuncs=[\"func.missing\"]\n");
    let report = plan_json(&f, &["design.toml"], "ERROR");
    assert!(report["waves"].as_array().unwrap().is_empty());
    f.write("design.toml", "[mod.api]\n[mod.api]\n");
    let report = plan_json(&f, &["design.toml"], "ERROR");
    assert_eq!(report["diagnostics"][0]["kind"], "InvalidPlanTarget");
    f.write("design.toml", "require=[\"mod.api\"]\n[mod.api]\n");
    let report = plan_json(&f, &["design.toml"], "ERROR");
    assert_eq!(report["diagnostics"][0]["kind"], "CyclicPlanDependencies");
    f.write("design.toml", "[mod.api]\n");
    let path = f.0.join("design.toml");
    let mut mode = fs::metadata(&path).unwrap().permissions();
    mode.set_readonly(true);
    fs::set_permissions(path, mode).unwrap();
    let report = plan_json(&f, &["design.toml", ".", "--sync-status"], "ERROR");
    assert_eq!(report["diagnostics"][0]["kind"], "StatusSyncFailed");
    assert_eq!(planned_unit(&report, "mod.api")["readiness"], "ready");
}

#[test]
fn plan_uses_symbol_mappings_and_places_shared_types_outside_tracks() {
    let f = Fixture::new();
    f.write(
        "design.toml",
        r#"
[schema.Shared]
kind = "node"
fields = { id = "int" }
[func.first]
input = ["schema.Shared"]
[func.second]
input = ["schema.Shared"]
[mod.parent]
submods = ["a", "b"]
funcs = ["func.first", "func.second"]
[mod.parent.a]
funcs = ["func.first"]
[mod.parent.b]
funcs = ["func.second"]
"#,
    );
    f.write("src/lib.rs", "pub struct RealShared { pub id: i32 }\n");
    f.write(
        "mapping.toml",
        r#"
version = "0.1"
[[entries]]
stable_id = "pkg.default.schema.global.Shared"
surv_ref = "schema.Shared"
impl_lang = "rust"
impl_path = "RealShared"
source_file = "src/lib.rs"
symbol_name = "RealShared"
kind = "Struct"
confidence = 1.0
"#,
    );
    let report = plan_json(
        &f,
        &[
            "design.toml",
            ".",
            "--lang",
            "rust",
            "--mapping",
            "mapping.toml",
        ],
        "OK",
    );
    let shared = planned_unit(&report, "schema.Shared");
    let id = shared["unit"]["unit_id"].clone();
    assert_eq!(shared["wave_index"], 0);
    assert_eq!(shared["unit"]["is_shared_boundary"], true);
    assert_eq!(report["ready_unit_count"], 2);
    let tracks = report["parallel_tracks"].as_array().unwrap();
    assert_eq!(tracks.len(), 2);
    for track in tracks {
        assert_eq!(
            track["prerequisite_boundary_units"],
            Value::Array(vec![id.clone()])
        );
        assert_eq!(track["ordered_units"].as_array().unwrap().len(), 1);
    }
}

fn verify_behavior(f: &Fixture, flags: &[&str], expected: &str) -> Value {
    let mut args = vec!["verify", "model.behavior.toml", "--format", "json"];
    args.extend_from_slice(flags);
    let result = f.run(&args);
    let expected_code = match expected {
        "PASS" => 0,
        "UNKNOWN" => 2,
        _ => 1,
    };
    assert_eq!(result.status.code(), Some(expected_code), "{result:?}");
    let report: Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["status"], expected, "{report}");
    report
}

#[test]
fn behavior_explores_nondeterminism_and_uses_pre_update_values() {
    let f = Fixture::new();
    let source = r#"
[behavior.order]
terminal = "status in {'paid', 'cancelled'}"
[behavior.order.vars]
status = "enum(pending, paid, cancelled)"
x = "int(0..3)"
y = "int(0..3)"
[behavior.order.init]
status = "pending"
x = 0
y = 1
[behavior.order.action.pay]
guard = "status == 'pending' and not false"
update = { status = "'paid'", x = "y", y = "x" }
[behavior.order.action.cancel]
guard = "status == 'pending'"
[behavior.order.action.cancel.update]
status = "'cancelled'"
[behavior.order.invariant.atomic_swap]
expr = "status == 'paid' => (x == 1 and y == 0)"
[behavior.order.invariant.range]
expr = "(if status == 'pending' then 1 else 2) * (x + y) <= 2"
"#;
    f.write("model.behavior.toml", source);
    f.ok(&["check", "model.behavior.toml"]);
    let report = verify_behavior(&f, &[], "PASS");
    assert_eq!(report["explored_states"], 3);
    assert_eq!(report["max_depth_reached"], 1);

    f.write(
        "model.behavior.toml",
        &source.replace("status in {'paid', 'cancelled'}", "status == 'paid'"),
    );
    let report = verify_behavior(&f, &[], "FAIL");
    assert_eq!(report["trace"]["violation_kind"], "deadlock");
    assert_eq!(report["trace"]["steps"][0]["action_name"], "cancel");
    assert_eq!(
        report["trace"]["steps"][0]["to_state"]["valuations"][0]["value"]["enum_val"],
        "cancelled"
    );
}

#[test]
fn behavior_checks_same_depth_state_violations_before_overflow() {
    let f = Fixture::new();
    let source = r#"
[behavior.layered]
terminal = "false"
[behavior.layered.vars]
x = "int(0..3)"
[behavior.layered.init]
x = 0
[behavior.layered.action.a_path]
guard = "x == 0"
update = { x = "3" }
[behavior.layered.action.b_path]
guard = "x == 0"
update = { x = "2" }
[behavior.layered.action.overflow]
guard = "x == 3"
update = { x = "x + 1" }
[behavior.layered.action.loop]
guard = "x == 2"
[behavior.layered.invariant.safe]
expr = "x != 2"
"#;
    f.write("model.behavior.toml", source);
    let state_violation = verify_behavior(&f, &[], "FAIL");
    assert_eq!(state_violation["trace"]["violation_kind"], "invariant");
    assert_eq!(
        state_violation["trace"]["steps"].as_array().unwrap().len(),
        1
    );
    assert_eq!(
        state_violation["trace"]["steps"][0]["action_name"],
        "b_path"
    );

    f.write("model.behavior.toml", &source.replace("x != 2", "true"));
    let overflow = verify_behavior(&f, &[], "FAIL");
    assert_eq!(overflow["trace"]["violation_kind"], "overflow");
    assert_eq!(overflow["trace"]["steps"].as_array().unwrap().len(), 1);
    assert!(overflow["trace"]["violated_state"].is_null());
    assert_eq!(overflow["trace"]["overflow_attempt"]["step_index"], 2);
    assert_eq!(overflow["trace"]["overflow_attempt"]["attempted_value"], 4);
}

#[test]
fn behavior_reports_limits_and_initial_counterexamples() {
    let f = Fixture::new();
    let source = r#"
[behavior.count]
terminal = "x == 2"
[behavior.count.vars]
x = "int(0..2)"
[behavior.count.init]
x = 0
[behavior.count.action.inc]
guard = "x < 2"
update = { x = "x + 1" }
[behavior.count.invariant.safe]
expr = "x >= 0"
"#;
    f.write("model.behavior.toml", source);
    assert_eq!(
        verify_behavior(&f, &["--max-states", "1"], "UNKNOWN")["stop_reason"],
        "max_states_reached"
    );
    assert_eq!(
        verify_behavior(&f, &["--max-depth", "1"], "UNKNOWN")["stop_reason"],
        "max_depth_reached"
    );
    assert_eq!(
        verify_behavior(&f, &["--max-states", "3", "--max-depth", "2"], "PASS")["pending_states"],
        0
    );

    f.write("model.behavior.toml", &source.replace("x >= 0", "x > 0"));
    let report = verify_behavior(&f, &[], "FAIL");
    assert_eq!(report["trace"]["violated_state"]["depth"], 0);
    assert!(report["trace"]["steps"].as_array().unwrap().is_empty());
}

#[test]
fn behavior_short_circuits_static_errors_and_collects_all_expression_syntax_errors() {
    let f = Fixture::new();
    f.write(
        "model.behavior.toml",
        "[behavior.invalid
",
    );
    assert_eq!(
        verify_behavior(&f, &[], "ERROR")["diagnostics"][0]["code"],
        "E_BEHAVIOR_TOML"
    );
    f.write(
        "model.behavior.toml",
        r#"
[behavior.invalid]
terminal = "x =="
[behavior.invalid.vars]
x = "int(0..3)"
[behavior.invalid.init]
x = 0
[behavior.invalid.action.bad]
guard = "x <"
update = { x = "x +" }
"#,
    );
    let report = verify_behavior(&f, &[], "ERROR");
    assert_eq!(report["explored_states"], 0);
    assert_eq!(report["diagnostics"].as_array().unwrap().len(), 3);
    assert!(report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .all(|diag| diag["code"] == "E_EXPR_SYNTAX"));

    f.write(
        "model.behavior.toml",
        r#"
[behavior.invalid]
terminal = "state == 'shipped'"
[behavior.invalid.vars]
state = "enum(pending, paid)"
[behavior.invalid.init]
state = "pending"
[behavior.invalid.action.bad]
guard = "missing"
update = { state = "2" }
"#,
    );
    let report = verify_behavior(&f, &[], "ERROR");
    for expected in [
        "invalid enum variant",
        "undeclared variable",
        "incompatible types",
    ] {
        assert!(
            report["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|diag| diag["message"].as_str().unwrap().contains(expected)),
            "{report}"
        );
    }
    f.write(
        "bad.toml",
        "[project
",
    );
    assert_eq!(
        verify_behavior(&f, &["--manifest", "bad.toml"], "ERROR")["diagnostics"][0]["code"],
        "E_BEHAVIOR_PROJECT"
    );
}

#[test]
fn behavior_inherits_project_limits_and_reuses_alias_and_dependency_resolution() {
    let f = Fixture::new();
    let manifest = r#"
[project]
name = "behavior_demo"
[paths]
ir_root = "ir"
behavior_root = "."
[verification]
default_max_states = 1
[packages.orders]
root = "ir/orders"
namespace = "app.orders"
depends = ["payments"]
[packages.payments]
root = "ir/payments"
namespace = "app.payments"
"#;
    f.write("surv.toml", manifest);
    f.write("ir/orders/order.toml", "[mod.order_api]\n");
    f.write(
        "ir/payments/pay.toml",
        "[func.processPayment]\n[mod.payment]\nfuncs=[\"func.processPayment\"]\n",
    );
    f.write(
        "model.behavior.toml",
        r#"
package = "orders"
import = ["payments as pay"]
[behavior.order]
owner = "mod.order_api"
terminal = "paid"
[behavior.order.vars]
paid = "bool"
[behavior.order.init]
paid = false
[behavior.order.action.pay]
refs = ["pay.func.processPayment"]
guard = "not paid"
update = { paid = "true" }
"#,
    );
    let report = verify_behavior(&f, &[], "UNKNOWN");
    assert_eq!(report["stop_reason"], "max_states_reached");
    verify_behavior(&f, &["--max-states", "2"], "PASS");
    assert!(f
        .ok(&["check", "surv.toml"])
        .contains("No project issues found"));
    let refs = f.ok(&[
        "refs",
        "app.payments.func.processPayment",
        "surv.toml",
        "--format",
        "json",
    ]);
    assert!(refs.contains("behavior.order.action.pay"));
    assert!(f
        .ok(&["inspect", "mod.order_api", "surv.toml"])
        .contains("behavior.order"));

    f.write(
        "surv.toml",
        &manifest.replace("depends = [\"payments\"]", "depends = []"),
    );
    let report = verify_behavior(&f, &[], "ERROR");
    assert_eq!(report["diagnostics"][0]["code"], "E_UNDECLARED_DEPENDENCY");
}

#[test]
fn behavior_codegen_emits_tla_and_config_from_the_validated_model() {
    let f = Fixture::new();
    f.write(
        "model.behavior.toml",
        r#"
[meta]
ir = "surv-behavior"
[behavior.counter]
terminal = "x == 1"
[behavior.counter.vars]
x = "int(0..1)"
keep = "bool"
[behavior.counter.init]
x = 0
keep = true
[behavior.counter.action.inc]
update = { x = "x + 1" }
[behavior.counter.invariant.safe]
expr = "x <= 1"
"#,
    );
    let output = f.ok(&["codegen", "tla+", "model.behavior.toml", "--format", "json"]);
    let spec: Value = serde_json::from_str(&output).unwrap();
    assert_eq!(spec["module_name"], "Surv_counter");
    let tla = spec["tla_source"].as_str().unwrap();
    assert!(tla.contains("surv_overflow' = TRUE"));
    assert!(tla.contains("UNCHANGED <<v_0_keep>>"));
    assert!(tla.contains("Spec == Init /\\ [][Next]_vars"));
    let cfg = spec["cfg_source"].as_str().unwrap();
    assert!(cfg.contains("INVARIANT NoOverflow"));
    assert!(cfg.contains("INVARIANT Invariant_0_safe"));
}

#[test]
fn behavior_discovers_metadata_files_and_checks_declared_dotted_variables() {
    let f = Fixture::new();
    f.write(
        "surv.toml",
        "[project]\nname=\"embedded\"\n[paths]\nir_root=\"ir\"\n",
    );
    f.write(
        "ir/controller.toml",
        r#"
[meta]
ir = "surv-behavior"

[behavior.controller]
terminal = "record.done"
[behavior.controller.vars]
record.done = "bool"
[behavior.controller.init]
record.done = false
[behavior.controller.action.finish]
guard = "not record.done"
update = { record.done = "true" }
"#,
    );
    let ast = f.ok(&["check", "ir/controller.toml", "--ast"]);
    let ast: Value = serde_json::from_str(&ast).unwrap();
    assert_eq!(ast["model"]["domains"][0]["var_name"], "record.done");
    assert!(f
        .ok(&["check", "surv.toml"])
        .contains("No project issues found"));
    assert!(f.ok(&["verify", "surv.toml"]).contains("PASS: controller"));
    assert!(f
        .ok(&["codegen", "tla+", "surv.toml", "--format", "cfg"])
        .contains("INVARIANT NoOverflow"));
}

#[test]
fn check_rejects_unknown_keys_invalid_kinds_and_duplicate_keys() {
    let f = Fixture::new();
    for (content, diagnostic) in [
        (
            "[schema.S]\nkind = \"node\"\nfilds = {}\n",
            "unknown schema key 'filds'",
        ),
        ("[schema.S]\nkind = \"bogus\"\n", "unknown kind 'bogus'"),
        (
            "[schema.S]\nkind = \"edge\"\nfrom = \"schema.S\"\n",
            "edge requires 'to'",
        ),
        (
            "[schema.S]\nkind = \"node\"\nkind = \"node\"\n",
            "duplicate key",
        ),
        ("[nonsense]\nx = 1\n", "unknown top-level key 'nonsense'"),
    ] {
        f.write("ir.toml", content);
        f.rejects(&["check", "ir.toml"], diagnostic);
    }
    f.write("ir.toml", "[schema.S]\nkind = \"node\"\n");
    f.rejects(&["check", "ir.toml", "--bogus"], "Unknown option");
}

#[test]
fn project_and_single_file_checks_agree_on_pipeline_cycles() {
    let f = Fixture::new();
    f.write(
        "surv.toml",
        r#"
[project]
name = "p"
[paths]
ir_root = "ir"
[packages.a]
root = "ir/a"
"#,
    );
    f.write(
        "ir/a/x.toml",
        r#"
[schema.A]
kind = "node"
[func.f]
intent = "f"
input = ["schema.A"]
output = ["schema.A"]
[mod.m]
purpose = "m"
funcs = ["func.f"]
pipeline = ["func.f", "func.f"]
"#,
    );
    f.rejects(&["check", "ir/a/x.toml"], "PipelineCycle");
    f.rejects(&["check", "surv.toml"], "PipelineCycle");
    f.rejects(&["plan", "surv.toml"], "PipelineCycle");
}

#[test]
fn status_init_quotes_keys_and_legacy_status_is_editable() {
    let f = Fixture::new();
    f.write(
        "weird.toml",
        "[mod.\"weird name\"]\npurpose = \"m\"\nschemas = []\nfuncs = []\npipeline = []\n",
    );
    f.ok(&["status", "init", "weird.toml"]);
    let content = fs::read_to_string(f.0.join("weird.toml")).unwrap();
    toml::from_str::<toml::Value>(&content).expect("status init must write valid TOML");

    f.write(
        "legacy.toml",
        "[mod.m]\npurpose = \"m\"\nschemas = []\nfuncs = []\npipeline = []\n\n[status]\nupdated_at = 2026-01-01\n\n[status.mod.m]\nstate = done\ncoverage = 0.5\nnotes = old note\n",
    );
    f.ok(&[
        "status",
        "set",
        "mod.m",
        "legacy.toml",
        "--state",
        "partial",
    ]);
    let content = fs::read_to_string(f.0.join("legacy.toml")).unwrap();
    assert!(content.contains("state = \"partial\""), "{content}");
    f.rejects(
        &["status", "set", "mod.m", "legacy.toml", "--coverage", "1.5"],
        "between 0.0 and 1.0",
    );
    f.rejects(
        &["status", "set", "mod.m", "legacy.toml", "--state", "bogus"],
        "must be todo, skeleton, partial, done, or blocked",
    );
}

#[test]
fn plan_sync_preserves_out_of_scope_status_and_derives_parent_state() {
    let f = Fixture::new();
    f.write(
        "ts.toml",
        r#"
[schema.T]
kind = "node"
impl.lang = "ts"
[func.make]
intent = "make"
input = ["schema.T"]
output = ["schema.T"]
impl.lang = "ts"
[mod.tsmod]
purpose = "ts"
schemas = ["schema.T"]
funcs = ["func.make"]
pipeline = ["func.make"]
"#,
    );
    f.ok(&["status", "init", "ts.toml"]);
    f.ok(&[
        "status",
        "set",
        "mod.tsmod",
        "ts.toml",
        "--state",
        "done",
        "--coverage",
        "1.0",
    ]);
    let output = f.ok(&["plan", "ts.toml", ".", "--lang", "rust", "--sync-status"]);
    assert!(output.contains("out_of_scope"), "{output}");
    let content = fs::read_to_string(f.0.join("ts.toml")).unwrap();
    assert!(content.contains("state = \"done\""), "{content}");
    assert!(content.contains("coverage = 1.0"), "{content}");

    f.write(
        "parent.toml",
        r#"
[schema.S]
kind = "node"
impl.bind = "S"
impl.lang = "rust"
[mod.api]
purpose = "api"
submods = ["worker"]
[mod.api.worker]
purpose = "worker"
schemas = ["schema.S"]
"#,
    );
    f.write("src/lib.rs", "pub struct S { pub id: String }\n");
    f.ok(&["status", "init", "parent.toml"]);
    f.ok(&[
        "plan",
        "parent.toml",
        ".",
        "--lang",
        "rust",
        "--sync-status",
    ]);
    let content = fs::read_to_string(f.0.join("parent.toml")).unwrap();
    assert!(
        content.contains("[status.mod.api]\nstate = \"done\""),
        "{content}"
    );
}

#[test]
fn deps_uses_manifest_packages_and_resolves_qualified_requires() {
    let f = Fixture::new();
    f.write(
        "surv.toml",
        r#"
[project]
name = "deps"
[paths]
ir_root = "ir"
[packages.a]
root = "ir/a"
[packages.b]
root = "ir/b"
"#,
    );
    f.write(
        "ir/a/main.toml",
        "require = [\"b.mod.other\"]\n[mod.main]\npurpose = \"main\"\n",
    );
    f.write("ir/b/other.toml", "[mod.other]\npurpose = \"other\"\n");
    let output = f.ok(&["deps", "surv.toml", "--package", "a"]);
    assert!(output.contains("mod.main"), "{output}");
    assert!(output.contains("mod.other (from b package)"), "{output}");
    let output = f.ok(&["deps", "surv.toml", "--cross-package"]);
    assert!(output.contains("a.mod.main → b.mod.other"), "{output}");
}

#[test]
fn diff_reports_ambiguous_symbols_across_packages() {
    let f = Fixture::new();
    f.write(
        "surv.toml",
        r#"
[project]
name = "p"
[paths]
ir_root = "ir"
[packages.a]
root = "ir/a"
[packages.b]
root = "ir/b"
"#,
    );
    f.write("ir/a/a.toml", "[schema.User]\nkind = \"node\"\n");
    f.write("ir/b/b.toml", "[schema.User]\nkind = \"node\"\n");
    f.write("src/lib.rs", "pub struct User { pub id: String }\n");
    f.rejects(&["diff", "surv.toml", "src", "--lang", "rust"], "Ambiguous");
}

#[test]
fn diff_rejects_field_type_suffix_as_no_match() {
    let f = Fixture::new();
    f.write(
        "design.toml",
        r#"
[schema.User]
kind = "node"
[schema.Widget]
kind = "node"
fields = { owner = "schema.User" }
"#,
    );
    f.write(
        "src/lib.rs",
        "pub struct User;\npub struct Widget { pub owner: UserView }\n",
    );
    f.rejects(
        &["diff", "design.toml", "src", "--lang", "rust"],
        "Schema field mismatch",
    );
}

#[test]
fn html_export_escapes_scripts_and_resolves_qualified_links() {
    let f = Fixture::new();
    f.write(
        "xss.toml",
        r#"
[func.f]
intent = "</script><script>alert(1)</script>"
[mod.m]
purpose = "</script><script>alert(2)</script>"
funcs = ["func.f"]
"#,
    );
    let html = f.ok(&["export", "html", "xss.toml"]);
    assert!(!html.contains("</script><script>alert"), "{html}");
    assert!(html.contains("\\u003c/script\\u003e"), "{html}");

    f.write(
        "surv.toml",
        r#"
[project]
name = "p"
[paths]
ir_root = "ir"
[packages.a]
root = "ir/a"
[packages.b]
root = "ir/b"
"#,
    );
    f.write(
        "ir/a/main.toml",
        "require = [\"b.mod.other\"]\n[mod.main]\npurpose = \"main\"\n",
    );
    f.write("ir/b/other.toml", "[mod.other]\npurpose = \"other\"\n");
    let html = f.ok(&["export", "html", "surv.toml"]);
    assert!(html.contains("\"target\": \"mod.other\""), "{html}");
    assert!(!html.contains("\"target\": \"b.mod.other\""), "{html}");
}

#[test]
fn gitlab_codegen_quotes_scripts_and_orders_stages() {
    let f = Fixture::new();
    f.write(
        "deploy.toml",
        r#"
[deploy.job.build]
runs = ["echo hello: world", "echo one\necho two"]
[deploy.job.test]
requires = ["job.build"]
runs = ["npm test"]
[deploy.job.deploy_staging]
requires = ["job.test"]
runs = ["kubectl apply -f staging.yaml"]
[deploy.job.integration_test]
requires = ["job.deploy_staging"]
runs = ["npm run test:integration"]
"#,
    );
    let yaml = f.ok(&["codegen", "gitlab-ci", "deploy.toml"]);
    assert!(
        yaml.contains("  - build\n  - test\n  - deploy\n  - deploy_3"),
        "{yaml}"
    );
    assert!(
        yaml.contains("    - |\n      echo hello: world\n"),
        "{yaml}"
    );
    assert!(yaml.contains("      echo one\n      echo two\n"), "{yaml}");
    assert!(
        yaml.contains("integration_test:\n  stage: deploy_3"),
        "{yaml}"
    );

    f.write(
        "cycle.toml",
        "[deploy.job.a]\nrequires = [\"job.b\"]\nruns = [\"echo a\"]\n[deploy.job.b]\nrequires = [\"job.a\"]\nruns = [\"echo b\"]\n",
    );
    f.rejects(&["codegen", "github-actions", "cycle.toml"], "DeployCycle");
}

#[test]
fn diff_skips_unreadable_non_utf8_files() {
    let f = Fixture::new();
    f.write("design.toml", "[schema.User]\nkind = \"node\"\n");
    f.write("src/lib.rs", "pub struct User { pub id: String }\n");
    fs::write(f.0.join("src/broken.rs"), [0xff, 0xfe, 0x00]).unwrap();
    let output = f.run(&["diff", "design.toml", "src", "--lang", "rust"]);
    assert!(output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("skipping src/broken.rs"), "{stderr}");
}

#[test]
fn deps_distinguishes_same_module_names_across_packages() {
    let f = Fixture::new();
    f.write(
        "surv.toml",
        r#"
[project]
name = "deps"
[paths]
ir_root = "ir"
[packages.a]
root = "ir/a"
[packages.b]
root = "ir/b"
"#,
    );
    f.write("ir/a/a.toml", "[mod.same]\npurpose = \"a\"\n");
    f.write("ir/b/b.toml", "[mod.same]\npurpose = \"b\"\n");
    assert!(f
        .ok(&["deps", "surv.toml", "--package", "a"])
        .contains("mod.same"));
    assert!(f
        .ok(&["deps", "surv.toml", "--package", "b"])
        .contains("mod.same"));
    let qualified = f.ok(&["deps", "surv.toml", "--module", "a.mod.same"]);
    assert!(qualified.contains("in a package"), "{qualified}");
    f.rejects(&["deps", "surv.toml", "--module", "mod.same"], "Ambiguous");
}

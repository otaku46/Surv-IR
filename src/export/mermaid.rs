use crate::ast::{ModSection, Section};
use crate::deploy::ast::DeployFile;
use crate::project::ProjectAST;
use std::collections::{HashMap, HashSet};

pub struct MermaidExporter;

impl MermaidExporter {
    pub fn new() -> Self {
        Self
    }

    /// Export Deploy IR job DAG as a Mermaid flowchart
    pub fn export_deploy_pipeline(&self, deploy: &DeployFile) -> String {
        let mut output = String::from("---\n");
        if let Some(pipeline) = &deploy.pipeline {
            output.push_str(&format!("title: Deploy Pipeline - {}\n", pipeline.name));
        } else {
            output.push_str("title: Deploy Pipeline\n");
        }
        output.push_str("---\n");
        output.push_str("flowchart TD\n");

        if deploy.jobs.is_empty() {
            output.push_str("    empty[No jobs defined]\n");
            return output;
        }

        // Generate job nodes
        for (job_name, job) in &deploy.jobs {
            let job_id = Self::sanitize_id(job_name);

            // Determine node style based on target
            let style = if !job.uses_target.is_empty() {
                let target_name = job
                    .uses_target
                    .strip_prefix("target.")
                    .unwrap_or(&job.uses_target);
                if let Some(target) = deploy.targets.get(target_name) {
                    match target.kind.as_str() {
                        "production" => ":::prod",
                        "staging" => ":::staging",
                        _ => "",
                    }
                } else {
                    ""
                }
            } else {
                ""
            };

            // Build node label with metadata
            let mut label = job_name.to_string();
            if !job.uses_target.is_empty() {
                label.push_str(&format!(
                    "<br/><small>target: {}</small>",
                    job.uses_target
                        .strip_prefix("target.")
                        .unwrap_or(&job.uses_target)
                ));
            }
            if !job.side_effects.is_empty() {
                label.push_str(&format!(
                    "<br/><small>⚠ {}</small>",
                    job.side_effects.join(", ")
                ));
            }

            output.push_str(&format!("    {}[\"{}\"]{}\n", job_id, label, style));
        }

        // Generate edges
        for (job_name, job) in &deploy.jobs {
            let job_id = Self::sanitize_id(job_name);
            for req in &job.requires {
                let req_name = req.strip_prefix("job.").unwrap_or(req);
                let req_id = Self::sanitize_id(req_name);
                output.push_str(&format!("    {} --> {}\n", req_id, job_id));
            }
        }

        // Add style definitions
        output.push_str("\n    classDef prod fill:#ff6b6b,stroke:#c92a2a,color:#fff\n");
        output.push_str("    classDef staging fill:#ffd43b,stroke:#f59f00,color:#000\n");

        output
    }

    /// Export a single module's pipeline as a Mermaid flowchart
    pub fn export_pipeline(&self, module: &ModSection, project: &ProjectAST) -> String {
        let mut output = String::from("---\n");
        output.push_str(&format!(
            "title: Pipeline - {}\n",
            Self::escape_label(&module.name)
        ));
        output.push_str("---\n");
        output.push_str("flowchart LR\n");

        if module.pipeline.is_empty() {
            output.push_str("    empty[No pipeline defined]\n");
            return output;
        }

        // Build func index
        let mut funcs = HashMap::new();
        for (_, file) in &project.files {
            for section in &file.sections {
                if let Section::Func(func) = section {
                    funcs.insert(format!("func.{}", func.name), func);
                }
            }
        }

        // Generate nodes and edges
        let pipeline_calls: Vec<String> = module
            .get_pipeline_calls()
            .into_iter()
            .map(|call| local_func_ref(&call))
            .collect();
        for (i, func_ref) in pipeline_calls.iter().enumerate() {
            let func_id = format!("f{}", i);
            let func_name = func_ref.strip_prefix("func.").unwrap_or(func_ref);

            // Add node with intent as subtext if available
            if let Some(func) = funcs.get(func_ref) {
                let intent = if func.intent.is_empty() {
                    String::new()
                } else {
                    format!("<br/><small>{}</small>", Self::escape_html(&func.intent))
                };
                output.push_str(&format!(
                    "    {}[\"{}{}\"]",
                    func_id,
                    Self::escape_label(func_name),
                    intent
                ));
            } else {
                output.push_str(&format!(
                    "    {}[\"{}⚠\"]",
                    func_id,
                    Self::escape_label(func_name)
                ));
            }

            // Add styling for undefined funcs
            if !funcs.contains_key(func_ref) {
                output.push_str(":::error");
            }
            output.push('\n');

            // Add edge from previous
            if i > 0 {
                let prev_id = format!("f{}", i - 1);

                // Check schema compatibility
                let prev_func = pipeline_calls.get(i - 1).and_then(|r| funcs.get(r));
                let curr_func = funcs.get(func_ref);

                if let (Some(prev), Some(curr)) = (prev_func, curr_func) {
                    let prev_output: Vec<String> = prev
                        .output
                        .iter()
                        .map(|reference| local_schema_ref(reference))
                        .collect();
                    let curr_input: Vec<String> = curr
                        .input
                        .iter()
                        .map(|reference| local_schema_ref(reference))
                        .collect();
                    let common = Self::find_common_schemas(&prev_output, &curr_input);
                    if common.is_empty() {
                        output.push_str(&format!(
                            "    {} -.->|⚠ no common schema| {}\n",
                            prev_id, func_id
                        ));
                    } else {
                        let label = common
                            .iter()
                            .map(|s| Self::escape_label(s.strip_prefix("schema.").unwrap_or(s)))
                            .collect::<Vec<_>>()
                            .join(", ");
                        output.push_str(&format!("    {} -->|{}| {}\n", prev_id, label, func_id));
                    }
                } else {
                    output.push_str(&format!("    {} --> {}\n", prev_id, func_id));
                }
            }
        }

        // Add styling
        output.push_str("\n    classDef error fill:#ffdddd,stroke:#ff0000\n");

        output
    }

    /// Export module dependency graph
    pub fn export_module_dependencies(&self, project: &ProjectAST) -> String {
        let mut output = String::from("---\ntitle: Module Dependencies\n---\n");
        output.push_str("flowchart TD\n");

        let requires = project.collect_normalized_requires();

        if requires.is_empty() && project.mods.is_empty() {
            output.push_str("    empty[No modules defined]\n");
            return output;
        }

        // Add all modules as nodes
        let mut defined_mods = HashSet::new();
        for mod_id in project.mods.keys() {
            let mod_name = mod_id.strip_prefix("mod.").unwrap_or(mod_id);
            output.push_str(&format!(
                "    {}[\"{}\"]\n",
                Self::sanitize_id(mod_id),
                Self::escape_label(mod_name)
            ));
            defined_mods.insert(local_mod_ref(mod_id));
        }

        // Add edges
        let mut seen_edges = HashSet::new();
        for req in &requires {
            let from_mod = local_mod_ref(&req.from_mod);
            let to_mod = local_mod_ref(&req.to_mod);
            let edge = (from_mod.clone(), to_mod.clone());
            if seen_edges.insert(edge) {
                let from_id = Self::sanitize_id(&from_mod);
                let to_id = Self::sanitize_id(&to_mod);

                // Check if target exists
                if !defined_mods.contains(&to_mod) {
                    output.push_str(&format!(
                        "    {}[\"{}⚠\"]:::error\n",
                        to_id,
                        Self::escape_label(to_mod.strip_prefix("mod.").unwrap_or(&to_mod))
                    ));
                }

                output.push_str(&format!("    {} --> {}\n", from_id, to_id));
            }
        }

        // Add styling
        output.push_str("\n    classDef error fill:#ffdddd,stroke:#ff0000\n");

        output
    }

    /// Export schema relationship graph
    pub fn export_schema_graph(&self, project: &ProjectAST) -> String {
        let mut output = String::from("---\ntitle: Schema Graph\n---\n");
        output.push_str("flowchart TD\n");

        // Collect all schemas
        let mut schemas = HashMap::new();
        for (_, file) in &project.files {
            for section in &file.sections {
                if let Section::Schema(schema) = section {
                    schemas.insert(format!("schema.{}", schema.name), schema);
                }
            }
        }

        if schemas.is_empty() {
            output.push_str("    empty[No schemas defined]\n");
            return output;
        }

        // Add nodes with kind/role
        for (schema_id, schema) in &schemas {
            let label = format!(
                "{}<br/><small>{}/{}</small>",
                Self::escape_label(&schema.name),
                Self::escape_label(&schema.kind),
                Self::escape_label(&schema.role)
            );
            let node_id = Self::sanitize_id(schema_id);

            output.push_str(&format!(
                "    {}[\"{}\"]{}\n",
                node_id,
                label,
                Self::get_schema_style(&schema.kind)
            ));
        }

        // Add edges for relationships
        for (schema_id, schema) in &schemas {
            match schema.kind.as_str() {
                "edge" => {
                    // edge: from -> to
                    if !schema.from.is_empty() && !schema.to.is_empty() {
                        let from_id = Self::sanitize_id(&local_schema_ref(&schema.from));
                        let to_id = Self::sanitize_id(&local_schema_ref(&schema.to));
                        output.push_str(&format!(
                            "    {} -.->|{}| {}\n",
                            from_id,
                            Self::escape_label(&schema.name),
                            to_id
                        ));
                    }
                }
                "boundary" => {
                    // boundary: contains schemas in 'over'
                    let boundary_id = Self::sanitize_id(schema_id);
                    for over_schema in &schema.over {
                        let over_id = Self::sanitize_id(&local_schema_ref(over_schema));
                        output.push_str(&format!("    {} -.-> {}\n", boundary_id, over_id));
                    }
                }
                "space" => {
                    // space: based on another schema
                    if !schema.base.is_empty() {
                        let base_id = Self::sanitize_id(&local_schema_ref(&schema.base));
                        let space_id = Self::sanitize_id(schema_id);
                        output.push_str(&format!("    {} ==> {}\n", space_id, base_id));
                    }
                }
                _ => {}
            }
        }

        // Add styling
        output.push_str("\n    classDef node fill:#d4e6f1,stroke:#2980b9\n");
        output.push_str("    classDef edge fill:#d5f4e6,stroke:#27ae60\n");
        output.push_str("    classDef boundary fill:#fdeaa8,stroke:#f39c12\n");
        output.push_str("    classDef space fill:#e8daef,stroke:#8e44ad\n");

        output
    }

    /// Export all schemas and funcs used by a specific module
    pub fn export_module_detail(&self, module: &ModSection, project: &ProjectAST) -> String {
        let mut output = String::from("---\n");
        output.push_str(&format!(
            "title: Module - {}\n",
            Self::escape_label(&module.name)
        ));
        output.push_str("---\n");
        output.push_str("flowchart TD\n");

        // Build indexes
        let mut schemas = HashMap::new();
        let mut funcs = HashMap::new();
        for (_, file) in &project.files {
            for section in &file.sections {
                match section {
                    Section::Schema(s) => {
                        schemas.insert(format!("schema.{}", s.name), s);
                    }
                    Section::Func(f) => {
                        funcs.insert(format!("func.{}", f.name), f);
                    }
                    _ => {}
                }
            }
        }

        // Add module node
        let mod_id = "MOD";
        output.push_str(&format!(
            "    {}[[\"{}\"]]\n",
            mod_id,
            Self::escape_label(&module.name)
        ));

        // Add schemas
        for schema_ref in &module.schemas {
            let schema_ref = local_schema_ref(schema_ref);
            let schema_id = Self::sanitize_id(&schema_ref);
            if let Some(schema) = schemas.get(&schema_ref) {
                output.push_str(&format!(
                    "    {}[\"schema: {}\"]:::schema\n",
                    schema_id,
                    Self::escape_label(&schema.name)
                ));
            } else {
                output.push_str(&format!(
                    "    {}[\"schema: {}⚠\"]:::error\n",
                    schema_id,
                    Self::escape_label(&schema_ref)
                ));
            }
            output.push_str(&format!("    {} -.-> {}\n", mod_id, schema_id));
        }

        // Add funcs
        for func_ref in &module.funcs {
            let func_ref = local_func_ref(func_ref);
            let func_id = Self::sanitize_id(&func_ref);
            if let Some(func) = funcs.get(&func_ref) {
                output.push_str(&format!(
                    "    {}[\"func: {}\"]:::func\n",
                    func_id,
                    Self::escape_label(&func.name)
                ));
            } else {
                output.push_str(&format!(
                    "    {}[\"func: {}⚠\"]:::error\n",
                    func_id,
                    Self::escape_label(&func_ref)
                ));
            }
            output.push_str(&format!("    {} --> {}\n", mod_id, func_id));
        }

        // Add styling
        output.push_str("\n    classDef schema fill:#d4e6f1,stroke:#2980b9\n");
        output.push_str("    classDef func fill:#d5f4e6,stroke:#27ae60\n");
        output.push_str("    classDef error fill:#ffdddd,stroke:#ff0000\n");

        output
    }

    // Helper functions

    fn find_common_schemas(a: &[String], b: &[String]) -> Vec<String> {
        let set_a: HashSet<_> = a.iter().collect();
        b.iter()
            .filter(|item| set_a.contains(item))
            .cloned()
            .collect()
    }

    fn sanitize_id(id: &str) -> String {
        let mut out = String::new();
        for ch in id.chars() {
            match ch {
                ch if ch.is_ascii_alphanumeric() => out.push(ch),
                '_' => out.push_str("__"),
                '.' => out.push('_'),
                '-' => out.push_str("_h_"),
                ch => out.push_str(&format!("_x{:x}_", ch as u32)),
            }
        }
        if out.is_empty() {
            "node".to_string()
        } else if out.starts_with(|ch: char| ch.is_ascii_digit()) {
            format!("n_{out}")
        } else {
            out
        }
    }

    fn escape_html(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }

    fn escape_label(s: &str) -> String {
        Self::escape_html(&s.replace('\n', " "))
    }

    fn get_schema_style(kind: &str) -> &'static str {
        match kind {
            "node" => ":::node",
            "edge" => ":::edge",
            "boundary" => ":::boundary",
            "space" => ":::space",
            _ => "",
        }
    }
}

impl Default for MermaidExporter {
    fn default() -> Self {
        Self::new()
    }
}

fn local_schema_ref(reference: &str) -> String {
    let reference = reference.trim();
    if let Some((_, name)) = reference.rsplit_once(".schema.") {
        format!("schema.{name}")
    } else if reference.starts_with("schema.") {
        reference.to_string()
    } else {
        format!("schema.{reference}")
    }
}

fn local_func_ref(reference: &str) -> String {
    let reference = reference.trim();
    if let Some((_, name)) = reference.rsplit_once(".func.") {
        format!("func.{name}")
    } else if reference.starts_with("func.") {
        reference.to_string()
    } else {
        format!("func.{reference}")
    }
}

fn local_mod_ref(reference: &str) -> String {
    let reference = reference.trim();
    if let Some((_, name)) = reference.rsplit_once(".mod.") {
        format!("mod.{name}")
    } else if reference.starts_with("mod.") {
        reference.to_string()
    } else {
        format!("mod.{reference}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::SurvFile;
    use crate::parser::parse_surv_file;
    use std::io::Cursor;
    use std::path::PathBuf;

    fn file(path: &str, text: &str) -> (PathBuf, SurvFile) {
        let parsed = parse_surv_file(Cursor::new(text)).expect("parse");
        (PathBuf::from(path), parsed)
    }

    #[test]
    fn exports_simple_pipeline() {
        let files = vec![file(
            "test.toml",
            r#"
[schema.user]
kind = "node"
type = "User"

[func.create_user]
intent = "Create a user"
input = ["schema.user"]
output = ["schema.user"]

[func.save_user]
intent = "Save user to DB"
input = ["schema.user"]
output = ["schema.user"]

[mod.api]
purpose = "User API"
schemas = ["schema.user"]
funcs = ["func.create_user", "func.save_user"]
pipeline = ["func.create_user", "func.save_user"]
"#,
        )];

        let project = ProjectAST::from_files(files);
        let exporter = MermaidExporter::new();

        let module = project.files[0]
            .1
            .sections
            .iter()
            .find_map(|s| match s {
                Section::Mod(m) => Some(m),
                _ => None,
            })
            .unwrap();

        let output = exporter.export_pipeline(module, &project);
        assert!(output.contains("flowchart LR"));
        assert!(output.contains("f0"));
        assert!(output.contains("f1"));
        assert!(output.contains("f0 -->"));
    }

    #[test]
    fn exports_module_dependencies() {
        let files = vec![
            file(
                "a.toml",
                r#"
require = ["mod.beta"]

[mod.alpha]
purpose = "test"
schemas = []
funcs = []
"#,
            ),
            file(
                "b.toml",
                r#"
[mod.beta]
purpose = "test"
schemas = []
funcs = []
"#,
            ),
        ];

        let project = ProjectAST::from_files(files);
        let exporter = MermaidExporter::new();
        let output = exporter.export_module_dependencies(&project);

        assert!(output.contains("flowchart TD"));
        assert!(output.contains("mod_alpha"));
        assert!(output.contains("mod_beta"));
        assert!(output.contains("-->"));
    }

    #[test]
    fn exports_schema_graph() {
        let files = vec![file(
            "test.toml",
            r#"
[schema.user]
kind = "node"
role = "data"
type = "User"

[schema.post]
kind = "node"
role = "data"
type = "Post"

[schema.user_posts]
kind = "edge"
from = "schema.user"
to = "schema.post"
"#,
        )];

        let project = ProjectAST::from_files(files);
        let exporter = MermaidExporter::new();
        let output = exporter.export_schema_graph(&project);

        assert!(output.contains("flowchart TD"));
        assert!(output.contains("schema_user"));
        assert!(output.contains("schema_post"));
        assert!(output.contains("node/data"));
    }
}

use crate::ast::{Section, SurvFile};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct ModRef {
    pub id: String,
    pub file: PathBuf,
    pub name: String,
}

#[derive(Debug)]
pub struct ProjectAST {
    pub files: Vec<(PathBuf, SurvFile)>,
    pub mods: HashMap<String, ModRef>,
}

#[derive(Debug, Clone)]
pub struct NormalizedRequire {
    pub from_mod: String,
    pub to_mod: String,
    pub raw: String,
    pub file: PathBuf,
}

impl ProjectAST {
    pub fn from_files(files: Vec<(PathBuf, SurvFile)>) -> Self {
        let mut mods = HashMap::new();

        for (path, file) in &files {
            for section in &file.sections {
                if let Section::Mod(m) = section {
                    let id = format!("mod.{}", m.name);
                    let entry = ModRef {
                        id: id.clone(),
                        file: path.clone(),
                        name: m.name.clone(),
                    };
                    mods.insert(id, entry);
                }
            }
        }

        ProjectAST { files, mods }
    }

    pub fn collect_normalized_requires(&self) -> Vec<NormalizedRequire> {
        let mut deps = Vec::new();

        for (path, file) in &self.files {
            let mods_in_file: Vec<String> = file
                .sections
                .iter()
                .filter_map(|section| match section {
                    Section::Mod(m) => Some(format!("mod.{}", m.name)),
                    _ => None,
                })
                .collect();

            if mods_in_file.is_empty() {
                continue;
            }

            for require in &file.requires {
                for from_mod in &mods_in_file {
                    deps.push(NormalizedRequire {
                        from_mod: from_mod.clone(),
                        to_mod: require.target.clone(),
                        raw: require.target.clone(),
                        file: path.clone(),
                    });
                }
            }
        }

        let mut seen = HashSet::new();
        deps.retain(|edge| seen.insert((edge.from_mod.clone(), edge.to_mod.clone())));
        deps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::parse_surv_file;
    use std::collections::HashSet;
    use std::io::Cursor;

    fn file(path: &str, text: &str) -> (PathBuf, SurvFile) {
        let parsed = parse_surv_file(Cursor::new(text)).expect("parse");
        (PathBuf::from(path), parsed)
    }

    #[test]
    fn normalized_requires_per_mod() {
        let files = vec![file(
            "a.toml",
            r#"
require = ["mod.shared"]

[mod.alpha]
purpose = "test"
schemas = []
funcs = []

[mod.beta]
purpose = "test"
schemas = []
funcs = []
"#,
        )];

        let project = ProjectAST::from_files(files);
        let deps = project.collect_normalized_requires();
        assert_eq!(deps.len(), 2);
        let from: HashSet<_> = deps.into_iter().map(|d| d.from_mod).collect();
        assert!(from.contains("mod.alpha"));
        assert!(from.contains("mod.beta"));
    }

    #[test]
    fn normalized_requires_dedup() {
        let files = vec![file(
            "a.toml",
            r#"
require = ["mod.shared", "mod.shared"]

[mod.alpha]
purpose = "test"
schemas = []
funcs = []
"#,
        )];

        let project = ProjectAST::from_files(files);
        let deps = project.collect_normalized_requires();
        assert_eq!(deps.len(), 1);
        let dep = &deps[0];
        assert_eq!(dep.from_mod, "mod.alpha");
        assert_eq!(dep.to_mod, "mod.shared");
    }
}

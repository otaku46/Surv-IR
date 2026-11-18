use serde::Serialize;
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize)]
pub struct PlanRequest {
    pub ir_path: PathBuf,
    pub workspace_root: Option<PathBuf>,
    pub filter_mod: Option<String>,
    pub lang: String,
    pub strategy: String,
    pub mapping_path: Option<PathBuf>,
    pub sync_status: bool,
    pub format: String,
}

impl PlanRequest {
    pub fn parse(args: &[String]) -> Result<Self, String> {
        let mut request = Self {
            ir_path: PathBuf::new(),
            workspace_root: None,
            filter_mod: None,
            lang: "both".into(),
            strategy: "static".into(),
            mapping_path: None,
            sync_status: false,
            format: "text".into(),
        };
        let mut args = args.iter();
        let mut options = true;
        while let Some(arg) = args.next() {
            if options && arg == "--" {
                options = false;
                continue;
            }
            if options && arg.starts_with('-') {
                if arg == "--sync-status" {
                    request.sync_status = true;
                    continue;
                }
                if !matches!(
                    arg.as_str(),
                    "--mod" | "--lang" | "--strategy" | "--mapping" | "--format"
                ) {
                    return Err(format!("Unknown plan option '{arg}'"));
                }
                let value = args
                    .next()
                    .filter(|value| !value.starts_with('-') && !value.is_empty())
                    .ok_or_else(|| format!("{arg} requires a value"))?;
                match arg.as_str() {
                    "--mod" => request.filter_mod = Some(value.clone()),
                    "--lang" => request.lang = value.clone(),
                    "--strategy" => request.strategy = value.clone(),
                    "--mapping" => request.mapping_path = Some(PathBuf::from(value)),
                    "--format" => request.format = value.clone(),
                    _ => unreachable!(),
                }
            } else if request.ir_path.as_os_str().is_empty() {
                request.ir_path = PathBuf::from(arg);
            } else if request.workspace_root.is_none() {
                request.workspace_root = Some(PathBuf::from(arg));
            } else {
                return Err(format!("Unexpected plan argument '{arg}'"));
            }
        }
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.ir_path.as_os_str().is_empty() {
            return Err("Missing Surv IR or surv.toml path".into());
        }
        if !matches!(
            self.lang.as_str(),
            "rust" | "ts" | "c" | "zig" | "both" | "all"
        ) {
            return Err(format!("Invalid plan language '{}'", self.lang));
        }
        if !matches!(self.strategy.as_str(), "static" | "lsp") {
            return Err(format!("Invalid plan strategy '{}'", self.strategy));
        }
        if !matches!(self.format.as_str(), "text" | "json" | "md") {
            return Err(format!("Invalid plan format '{}'", self.format));
        }
        if self.sync_status && self.workspace_root.is_none() {
            return Err("--sync-status requires workspace_root".into());
        }
        Ok(())
    }
}

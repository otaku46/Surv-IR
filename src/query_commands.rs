use serde::Serialize;
use std::collections::{HashSet, VecDeque};
use std::error::Error;
use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};

use survibe_parser_rs::{load_project, parse_surv_file, Manifest, Section, SurvFile, SymbolKind};

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
struct ItemKey {
    kind: SymbolKind,
    namespace: Option<String>,
    name: String,
    file_index: usize,
}

#[derive(Clone, Debug)]
struct Definition {
    kind: SymbolKind,
    namespace: Option<String>,
    name: String,
    file_index: usize,
}

#[derive(Clone, Debug)]
struct QualifiedRef {
    kind: SymbolKind,
    namespace: Option<String>,
    name: String,
}

#[derive(Clone, Debug)]
struct Target {
    kind: SymbolKind,
    namespace: Option<String>,
    name: String,
}

#[derive(Clone, Debug)]
struct SourceFile {
    path: PathBuf,
    file: SurvFile,
}

#[derive(Clone, Copy)]
enum OutputFormat {
    List,
    Json,
    Toml,
}

#[derive(Clone, Copy)]
enum TraceDirection {
    Up,
    Down,
    Both,
}

#[derive(Clone, Copy)]
struct SliceInclude {
    schemas: bool,
    funcs: bool,
    mods: bool,
}

#[derive(Clone, Copy, Default)]
struct RefsInclude {
    import: bool,
    impl_meta: bool,
    boundary: bool,
}

#[derive(Serialize)]
struct OutputItem {
    kind: String,
    name: String,
    source: String,
}

pub fn run_slice(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_slice_usage();
        return Ok(());
    }
    if args.len() < 2 {
        print_slice_usage();
        std::process::exit(1);
    }

    let target_raw = &args[0];
    let file_path = &args[1];
    let mut include = SliceInclude {
        schemas: true,
        funcs: true,
        mods: true,
    };
    let mut with_defs = false;
    let mut closure = false;
    let mut mod_context: Option<String> = None;
    let mut format = OutputFormat::List;
    let mut format_set = false;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--include" => {
                if i + 1 >= args.len() {
                    return Err("--include requires a value".into());
                }
                include = parse_slice_include(&args[i + 1])?;
                i += 2;
            }
            "--with-defs" => {
                with_defs = true;
                i += 1;
            }
            "--closure" => {
                closure = true;
                i += 1;
            }
            "--mod" => {
                if i + 1 >= args.len() {
                    return Err("--mod requires a module name".into());
                }
                mod_context = Some(args[i + 1].clone());
                i += 2;
            }
            "--format" => {
                if i + 1 >= args.len() {
                    return Err("--format requires a value".into());
                }
                format = parse_format(&args[i + 1])?;
                format_set = true;
                i += 2;
            }
            other => {
                return Err(format!("Unknown option: {}", other).into());
            }
        }
    }

    if matches!(format, OutputFormat::Toml) && !with_defs {
        return Err("toml format requires --with-defs".into());
    }
    if with_defs && !format_set {
        format = OutputFormat::Toml;
    }

    let sources = load_sources(Path::new(file_path))?;
    let defs = collect_definitions(&sources);
    let target = parse_target(target_raw)?;
    let target_def = resolve_target_definition(&target, &defs, &sources)?;

    let module_context = if target_def.kind == SymbolKind::Func {
        resolve_function_context(&target_def, &sources, mod_context.as_deref())?
    } else {
        None
    };

    let mut seen: HashSet<ItemKey> = HashSet::new();
    let mut ordered: Vec<ItemKey> = Vec::new();
    let target_item = ItemKey {
        kind: target_def.kind,
        namespace: target_def.namespace.clone(),
        name: target_def.name.clone(),
        file_index: target_def.file_index,
    };
    seen.insert(target_item.clone());
    ordered.push(target_item.clone());

    let mut queue: VecDeque<(ItemKey, bool)> = VecDeque::new();
    queue.push_back((target_item, true));

    while let Some((item, is_root)) = queue.pop_front() {
        let deps = expand_slice_item(
            &item,
            is_root,
            include,
            &sources,
            &defs,
            module_context.as_ref(),
        );

        for dep in deps {
            if seen.insert(dep.clone()) {
                ordered.push(dep.clone());
                if closure {
                    queue.push_back((dep, false));
                }
            }
        }

        if !closure && is_root {
            break;
        }
    }

    emit_slice_output(&ordered, &sources, format, with_defs)?;
    Ok(())
}

pub fn run_refs(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_refs_usage();
        return Ok(());
    }
    if args.len() < 2 {
        print_refs_usage();
        std::process::exit(1);
    }

    let target_raw = &args[0];
    let file_path = &args[1];

    let mut format = OutputFormat::List;
    let mut kind_filter: Option<SymbolKind> = None;
    let mut include = RefsInclude::default();

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--format" => {
                if i + 1 >= args.len() {
                    return Err("--format requires a value".into());
                }
                format = parse_format(&args[i + 1])?;
                i += 2;
            }
            "--kind" => {
                if i + 1 >= args.len() {
                    return Err("--kind requires a value".into());
                }
                kind_filter = Some(parse_kind(&args[i + 1])?);
                i += 2;
            }
            "--include" => {
                if i + 1 >= args.len() {
                    return Err("--include requires a value".into());
                }
                include = parse_refs_include(&args[i + 1])?;
                i += 2;
            }
            other => {
                return Err(format!("Unknown option: {}", other).into());
            }
        }
    }

    if matches!(format, OutputFormat::Toml) {
        return Err("refs does not support toml output".into());
    }

    let sources = load_sources(Path::new(file_path))?;
    let target = parse_target(target_raw)?;
    let mut results: Vec<ItemKey> = Vec::new();
    let mut seen: HashSet<ItemKey> = HashSet::new();

    for (file_index, source) in sources.iter().enumerate() {
        let namespace = source.file.namespace.as_deref();
        let modules_in_file: Vec<String> = source
            .file
            .sections
            .iter()
            .filter_map(|section| match section {
                Section::Mod(m) => Some(m.name.clone()),
                _ => None,
            })
            .collect();

        for section in &source.file.sections {
            match section {
                Section::Schema(schema) => {
                    if kind_filter
                        .map(|k| k != SymbolKind::Schema)
                        .unwrap_or(false)
                    {
                        continue;
                    }
                    if section_references_target(
                        &schema_reference_sites(schema, include),
                        namespace,
                        &target,
                    ) {
                        let item = ItemKey {
                            kind: SymbolKind::Schema,
                            namespace: source.file.namespace.clone(),
                            name: schema.name.clone(),
                            file_index,
                        };
                        if seen.insert(item.clone()) {
                            results.push(item);
                        }
                    }
                }
                Section::Func(func) => {
                    if kind_filter.map(|k| k != SymbolKind::Func).unwrap_or(false) {
                        continue;
                    }
                    if section_references_target(
                        &func_reference_sites(func, include),
                        namespace,
                        &target,
                    ) {
                        let item = ItemKey {
                            kind: SymbolKind::Func,
                            namespace: source.file.namespace.clone(),
                            name: func.name.clone(),
                            file_index,
                        };
                        if seen.insert(item.clone()) {
                            results.push(item);
                        }
                    }
                }
                Section::Mod(module) => {
                    if kind_filter.map(|k| k != SymbolKind::Mod).unwrap_or(false) {
                        continue;
                    }
                    if section_references_target(
                        &mod_reference_sites(module, include),
                        namespace,
                        &target,
                    ) {
                        let item = ItemKey {
                            kind: SymbolKind::Mod,
                            namespace: source.file.namespace.clone(),
                            name: module.name.clone(),
                            file_index,
                        };
                        if seen.insert(item.clone()) {
                            results.push(item);
                        }
                    }
                }
                Section::Meta(_) | Section::Status(_) => {}
            }
        }

        if kind_filter.map(|k| k == SymbolKind::Mod).unwrap_or(true) {
            for require in &source.file.requires {
                if reference_matches_target(&require.target, namespace, &target) {
                    for mod_name in &modules_in_file {
                        let item = ItemKey {
                            kind: SymbolKind::Mod,
                            namespace: source.file.namespace.clone(),
                            name: mod_name.clone(),
                            file_index,
                        };
                        if seen.insert(item.clone()) {
                            results.push(item);
                        }
                    }
                }
            }
        }

        if include.import {
            for import in &source.file.imports {
                if reference_matches_target(&import.target, namespace, &target) {
                    for mod_name in &modules_in_file {
                        let item = ItemKey {
                            kind: SymbolKind::Mod,
                            namespace: source.file.namespace.clone(),
                            name: mod_name.clone(),
                            file_index,
                        };
                        if seen.insert(item.clone()) {
                            results.push(item);
                        }
                    }
                }
            }
        }
    }

    emit_items_output(&results, &sources, format, true)?;
    Ok(())
}

pub fn run_trace(args: &[String]) -> Result<(), Box<dyn Error>> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_trace_usage();
        return Ok(());
    }
    if args.len() < 2 {
        print_trace_usage();
        std::process::exit(1);
    }

    let target_raw = &args[0];
    let file_path = &args[1];
    let mut direction = TraceDirection::Both;
    let mut mod_context: Option<String> = None;
    let mut depth: Option<usize> = None;
    let mut format = OutputFormat::List;

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "--up" => {
                direction = TraceDirection::Up;
                i += 1;
            }
            "--down" => {
                direction = TraceDirection::Down;
                i += 1;
            }
            "--both" => {
                direction = TraceDirection::Both;
                i += 1;
            }
            "--mod" => {
                if i + 1 >= args.len() {
                    return Err("--mod requires a module name".into());
                }
                mod_context = Some(args[i + 1].clone());
                i += 2;
            }
            "--depth" => {
                if i + 1 >= args.len() {
                    return Err("--depth requires a value".into());
                }
                depth = Some(args[i + 1].parse()?);
                i += 2;
            }
            "--format" => {
                if i + 1 >= args.len() {
                    return Err("--format requires a value".into());
                }
                format = parse_format(&args[i + 1])?;
                i += 2;
            }
            other => {
                return Err(format!("Unknown option: {}", other).into());
            }
        }
    }

    if matches!(format, OutputFormat::Toml) {
        return Err("trace does not support toml output".into());
    }

    let sources = load_sources(Path::new(file_path))?;
    let defs = collect_definitions(&sources);
    let target = parse_target(target_raw)?;
    let target_def = resolve_target_definition(&target, &defs, &sources)?;

    let items = match target_def.kind {
        SymbolKind::Func => {
            let module_context =
                resolve_function_context(&target_def, &sources, mod_context.as_deref())?
                    .ok_or("Function is not referenced in any module pipeline")?;
            trace_function_pipeline(&target_def, &module_context, &sources, direction, depth)?
        }
        SymbolKind::Mod => trace_module_flow(&target_def, &sources, depth)?,
        _ => {
            return Err("trace only supports func.* or mod.* targets".into());
        }
    };

    emit_items_output(&items, &sources, format, false)?;
    Ok(())
}

fn parse_slice_include(raw: &str) -> Result<SliceInclude, Box<dyn Error>> {
    let mut include = SliceInclude {
        schemas: false,
        funcs: false,
        mods: false,
    };
    for item in raw.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        match item {
            "schemas" | "schema" => include.schemas = true,
            "funcs" | "func" => include.funcs = true,
            "mods" | "mod" => include.mods = true,
            other => return Err(format!("Unknown include: {}", other).into()),
        }
    }
    if !(include.schemas || include.funcs || include.mods) {
        return Err("include list is empty".into());
    }
    Ok(include)
}

fn parse_refs_include(raw: &str) -> Result<RefsInclude, Box<dyn Error>> {
    let mut include = RefsInclude::default();
    for item in raw.split(',').map(|s| s.trim()).filter(|s| !s.is_empty()) {
        match item {
            "all" => {
                include.import = true;
                include.impl_meta = true;
                include.boundary = true;
            }
            "import" => include.import = true,
            "impl" => include.impl_meta = true,
            "boundary" => include.boundary = true,
            other => return Err(format!("Unknown include: {}", other).into()),
        }
    }
    Ok(include)
}

fn parse_format(raw: &str) -> Result<OutputFormat, Box<dyn Error>> {
    match raw {
        "list" => Ok(OutputFormat::List),
        "json" => Ok(OutputFormat::Json),
        "toml" => Ok(OutputFormat::Toml),
        other => Err(format!("Unknown format: {}", other).into()),
    }
}

fn parse_kind(raw: &str) -> Result<SymbolKind, Box<dyn Error>> {
    match raw {
        "schema" => Ok(SymbolKind::Schema),
        "func" => Ok(SymbolKind::Func),
        "mod" => Ok(SymbolKind::Mod),
        other => Err(format!("Unknown kind: {}", other).into()),
    }
}

fn parse_target(raw: &str) -> Result<Target, Box<dyn Error>> {
    let (kind, namespace, name) =
        parse_reference(raw).ok_or_else(|| format!("Invalid target reference: {}", raw))?;
    Ok(Target {
        kind,
        namespace,
        name,
    })
}

fn parse_reference(raw: &str) -> Option<(SymbolKind, Option<String>, String)> {
    let parts: Vec<&str> = raw.split('.').filter(|part| !part.is_empty()).collect();
    if parts.len() < 2 {
        return None;
    }
    let mut kind_index = None;
    for (i, part) in parts.iter().enumerate() {
        if *part == "schema" || *part == "func" || *part == "mod" {
            kind_index = Some(i);
            break;
        }
    }
    let idx = kind_index?;
    if idx + 1 >= parts.len() {
        return None;
    }
    let kind = match parts[idx] {
        "schema" => SymbolKind::Schema,
        "func" => SymbolKind::Func,
        "mod" => SymbolKind::Mod,
        _ => return None,
    };
    let namespace = if idx == 0 {
        None
    } else {
        Some(parts[..idx].join("."))
    };
    let name = parts[idx + 1..].join(".");
    Some((kind, namespace, name))
}

fn qualify_reference(raw: &str, namespace: Option<&str>) -> Option<QualifiedRef> {
    let (kind, ref_namespace, name) = parse_reference(raw)?;
    let namespace = ref_namespace.or_else(|| namespace.map(|ns| ns.to_string()));
    Some(QualifiedRef {
        kind,
        namespace,
        name,
    })
}

fn format_ref(kind: SymbolKind, namespace: Option<&str>, name: &str) -> String {
    let prefix = match kind {
        SymbolKind::Schema => "schema",
        SymbolKind::Func => "func",
        SymbolKind::Mod => "mod",
    };
    match namespace {
        Some(ns) => format!("{ns}.{prefix}.{name}"),
        None => format!("{prefix}.{name}"),
    }
}

fn target_matches_ref(target: &Target, reference: &QualifiedRef) -> bool {
    if target.kind != reference.kind {
        return false;
    }
    if let Some(ns) = &target.namespace {
        reference.namespace.as_deref() == Some(ns.as_str()) && target.name == reference.name
    } else {
        target.name == reference.name
    }
}

fn collect_definitions(sources: &[SourceFile]) -> Vec<Definition> {
    let mut defs = Vec::new();
    for (file_index, source) in sources.iter().enumerate() {
        let namespace = source.file.namespace.clone();
        for section in &source.file.sections {
            match section {
                Section::Schema(schema) => defs.push(Definition {
                    kind: SymbolKind::Schema,
                    namespace: namespace.clone(),
                    name: schema.name.clone(),
                    file_index,
                }),
                Section::Func(func) => defs.push(Definition {
                    kind: SymbolKind::Func,
                    namespace: namespace.clone(),
                    name: func.name.clone(),
                    file_index,
                }),
                Section::Mod(module) => defs.push(Definition {
                    kind: SymbolKind::Mod,
                    namespace: namespace.clone(),
                    name: module.name.clone(),
                    file_index,
                }),
                Section::Meta(_) | Section::Status(_) => {}
            }
        }
    }
    defs
}

fn resolve_target_definition(
    target: &Target,
    defs: &[Definition],
    sources: &[SourceFile],
) -> Result<Definition, Box<dyn Error>> {
    let matches: Vec<Definition> = defs
        .iter()
        .filter(|def| def.kind == target.kind)
        .filter(|def| def.name == target.name)
        .filter(|def| {
            target
                .namespace
                .as_deref()
                .map(|ns| def.namespace.as_deref() == Some(ns))
                .unwrap_or(true)
        })
        .cloned()
        .collect();

    if matches.is_empty() {
        return Err(format!("Target not found: {}", format_target(target)).into());
    }
    if matches.len() > 1 {
        let mut candidates = Vec::new();
        for def in matches {
            let source = &sources[def.file_index];
            let name = format_ref(def.kind, def.namespace.as_deref(), &def.name);
            candidates.push(format!("{} ({})", name, source.path.display()));
        }
        return Err(format!("Target is ambiguous; candidates: {}", candidates.join(", ")).into());
    }

    Ok(matches[0].clone())
}

fn format_target(target: &Target) -> String {
    format_ref(target.kind, target.namespace.as_deref(), &target.name)
}

fn load_sources(path: &Path) -> Result<Vec<SourceFile>, Box<dyn Error>> {
    let raw = fs::read_to_string(path)?;
    if toml::from_str::<Manifest>(&raw).is_ok() {
        let project = load_project(path)?;
        let files = project
            .files
            .into_iter()
            .map(|(path, file)| SourceFile { path, file })
            .collect();
        return Ok(files);
    }
    let file = File::open(path)?;
    let parsed = parse_surv_file(file)?;
    Ok(vec![SourceFile {
        path: path.to_path_buf(),
        file: parsed,
    }])
}

fn resolve_function_context(
    target: &Definition,
    sources: &[SourceFile],
    mod_context: Option<&str>,
) -> Result<Option<Definition>, Box<dyn Error>> {
    let target_ref = QualifiedRef {
        kind: SymbolKind::Func,
        namespace: target.namespace.clone(),
        name: target.name.clone(),
    };

    let mut candidates = Vec::new();
    for (file_index, source) in sources.iter().enumerate() {
        let namespace = source.file.namespace.as_deref();
        for section in &source.file.sections {
            if let Section::Mod(module) = section {
                if module_contains_function(module, namespace, &target_ref) {
                    candidates.push(Definition {
                        kind: SymbolKind::Mod,
                        namespace: source.file.namespace.clone(),
                        name: module.name.clone(),
                        file_index,
                    });
                }
            }
        }
    }

    let chosen = if let Some(mod_raw) = mod_context {
        let mod_target =
            parse_target(mod_raw).or_else(|_| parse_target(&format!("mod.{}", mod_raw)))?;
        if mod_target.kind != SymbolKind::Mod {
            return Err("--mod must be a mod.* reference".into());
        }
        let resolved =
            resolve_target_definition(&mod_target, &collect_definitions(sources), sources)?;
        if !candidates
            .iter()
            .any(|def| def.name == resolved.name && def.namespace == resolved.namespace)
        {
            return Err(format!(
                "Function {} is not referenced in {}",
                format_ref(target.kind, target.namespace.as_deref(), &target.name),
                format_ref(resolved.kind, resolved.namespace.as_deref(), &resolved.name)
            )
            .into());
        }
        Some(resolved)
    } else {
        match candidates.len() {
            0 => None,
            1 => Some(candidates[0].clone()),
            _ => {
                let mut names = Vec::new();
                for candidate in candidates {
                    let source = &sources[candidate.file_index];
                    let name = format_ref(
                        candidate.kind,
                        candidate.namespace.as_deref(),
                        &candidate.name,
                    );
                    names.push(format!("{} ({})", name, source.path.display()));
                }
                return Err(format!(
                    "Function appears in multiple modules; use --mod: {}",
                    names.join(", ")
                )
                .into());
            }
        }
    };

    Ok(chosen)
}

fn module_contains_function(
    module: &survibe_parser_rs::ModSection,
    namespace: Option<&str>,
    target: &QualifiedRef,
) -> bool {
    let mut refs = Vec::new();
    refs.extend(module.funcs.iter().cloned());
    refs.extend(module.get_pipeline_calls());
    refs.iter().any(|item| {
        qualify_reference(item, namespace)
            .map(|q| {
                target_matches_ref(
                    &Target {
                        kind: target.kind,
                        namespace: target.namespace.clone(),
                        name: target.name.clone(),
                    },
                    &q,
                )
            })
            .unwrap_or(false)
    })
}

fn expand_slice_item(
    item: &ItemKey,
    is_root: bool,
    include: SliceInclude,
    sources: &[SourceFile],
    defs: &[Definition],
    mod_context: Option<&Definition>,
) -> Vec<ItemKey> {
    let mut deps = Vec::new();
    let source = &sources[item.file_index];
    let namespace = source.file.namespace.as_deref();

    match item.kind {
        SymbolKind::Mod => {
            if !include.schemas && !include.funcs {
                return deps;
            }
            let module = find_module(&source.file, &item.name);
            if let Some(module) = module {
                if include.schemas {
                    deps.extend(resolve_references(
                        &module.schemas,
                        namespace,
                        defs,
                        SymbolKind::Schema,
                    ));
                }
                if include.funcs {
                    let mut func_refs = module.funcs.clone();
                    func_refs.extend(module.get_pipeline_calls());
                    deps.extend(resolve_references(
                        &func_refs,
                        namespace,
                        defs,
                        SymbolKind::Func,
                    ));
                }
            }
        }
        SymbolKind::Func => {
            if !include.schemas && !include.funcs {
                return deps;
            }
            let func = find_func(&source.file, &item.name);
            if let Some(func) = func {
                if include.schemas {
                    deps.extend(resolve_references(
                        &func.input,
                        namespace,
                        defs,
                        SymbolKind::Schema,
                    ));
                    deps.extend(resolve_references(
                        &func.output,
                        namespace,
                        defs,
                        SymbolKind::Schema,
                    ));
                }
            }
            if is_root && include.funcs {
                if let Some(context) = mod_context {
                    if let Some(adjacent) = pipeline_adjacency(context, item, sources) {
                        deps.extend(adjacent);
                    }
                }
            }
        }
        SymbolKind::Schema => {
            if !include.schemas {
                return deps;
            }
            let schema = find_schema(&source.file, &item.name);
            if let Some(schema) = schema {
                let mut references = Vec::new();
                if !schema.base.is_empty() {
                    references.push(schema.base.clone());
                }
                if !schema.from.is_empty() {
                    references.push(schema.from.clone());
                }
                if !schema.to.is_empty() {
                    references.push(schema.to.clone());
                }
                if !schema.r#type.is_empty() {
                    references.push(schema.r#type.clone());
                }
                references.extend(schema.over.clone());
                references.extend(extract_field_references(schema));
                deps.extend(resolve_references(
                    &references,
                    namespace,
                    defs,
                    SymbolKind::Schema,
                ));
            }
        }
    }

    deps
}

fn pipeline_adjacency(
    mod_context: &Definition,
    target: &ItemKey,
    sources: &[SourceFile],
) -> Option<Vec<ItemKey>> {
    let source = sources.get(mod_context.file_index)?;
    let namespace = source.file.namespace.as_deref();
    let module = find_module(&source.file, &mod_context.name)?;

    let pipeline: Vec<QualifiedRef> = module
        .get_pipeline_calls()
        .iter()
        .filter_map(|step| qualify_reference(step, namespace))
        .collect();

    let target_ref = QualifiedRef {
        kind: SymbolKind::Func,
        namespace: target.namespace.clone(),
        name: target.name.clone(),
    };

    let mut deps = Vec::new();
    if let Some(idx) = pipeline.iter().position(|step| {
        step.kind == target_ref.kind
            && step.name == target_ref.name
            && step.namespace == target_ref.namespace
    }) {
        if idx > 0 {
            if let Some(prev) = pipeline.get(idx - 1) {
                deps.extend(resolve_qualified_with_sources(
                    prev,
                    sources,
                    SymbolKind::Func,
                ));
            }
        }
        if let Some(next) = pipeline.get(idx + 1) {
            deps.extend(resolve_qualified_with_sources(
                next,
                sources,
                SymbolKind::Func,
            ));
        }
    }

    Some(deps)
}

fn resolve_references(
    refs: &[String],
    namespace: Option<&str>,
    defs: &[Definition],
    kind: SymbolKind,
) -> Vec<ItemKey> {
    let mut items = Vec::new();
    for raw in refs {
        if let Some(qref) = qualify_reference(raw, namespace) {
            if qref.kind != kind {
                continue;
            }
            items.extend(resolve_qualified(&qref, defs, kind));
        }
    }
    items
}

fn resolve_qualified(qref: &QualifiedRef, defs: &[Definition], kind: SymbolKind) -> Vec<ItemKey> {
    let matches: Vec<_> = defs
        .iter()
        .filter(|def| def.kind == kind && def.name == qref.name)
        .filter(|def| {
            qref.namespace
                .as_deref()
                .map(|ns| def.namespace.as_deref() == Some(ns))
                .unwrap_or(true)
        })
        .cloned()
        .collect();

    matches
        .into_iter()
        .map(|def| ItemKey {
            kind: def.kind,
            namespace: def.namespace.clone(),
            name: def.name.clone(),
            file_index: def.file_index,
        })
        .collect()
}

fn resolve_qualified_with_sources(
    qref: &QualifiedRef,
    sources: &[SourceFile],
    kind: SymbolKind,
) -> Vec<ItemKey> {
    let defs = collect_definitions(sources);
    resolve_qualified(qref, &defs, kind)
}

fn trace_function_pipeline(
    target: &Definition,
    module_def: &Definition,
    sources: &[SourceFile],
    direction: TraceDirection,
    depth: Option<usize>,
) -> Result<Vec<ItemKey>, Box<dyn Error>> {
    let source = &sources[module_def.file_index];
    let module = find_module(&source.file, &module_def.name)
        .ok_or("Module context missing from source file")?;
    let namespace = source.file.namespace.as_deref();

    let pipeline: Vec<QualifiedRef> = module
        .get_pipeline_calls()
        .iter()
        .filter_map(|step| qualify_reference(step, namespace))
        .collect();

    let target_ref = QualifiedRef {
        kind: SymbolKind::Func,
        namespace: target.namespace.clone(),
        name: target.name.clone(),
    };

    let idx = pipeline
        .iter()
        .position(|step| {
            step.kind == target_ref.kind
                && step.name == target_ref.name
                && step.namespace == target_ref.namespace
        })
        .ok_or("Function not found in module pipeline")?;

    let start = match direction {
        TraceDirection::Up => idx.saturating_sub(depth.unwrap_or(idx)),
        TraceDirection::Down => idx,
        TraceDirection::Both => idx.saturating_sub(depth.unwrap_or(idx)),
    };
    let end = match direction {
        TraceDirection::Up => idx,
        TraceDirection::Down => {
            let max = pipeline.len().saturating_sub(1);
            let offset = depth.unwrap_or(max - idx);
            (idx + offset).min(max)
        }
        TraceDirection::Both => {
            let max = pipeline.len().saturating_sub(1);
            let offset = depth.unwrap_or(max - idx);
            (idx + offset).min(max)
        }
    };

    let defs = collect_definitions(sources);
    let mut items = Vec::new();
    for step in &pipeline[start..=end] {
        let mut resolved = resolve_qualified(step, &defs, SymbolKind::Func);
        if resolved.is_empty() {
            resolved.push(ItemKey {
                kind: SymbolKind::Func,
                namespace: step.namespace.clone(),
                name: step.name.clone(),
                file_index: module_def.file_index,
            });
        }
        items.extend(resolved);
    }

    Ok(items)
}

fn trace_module_flow(
    target: &Definition,
    sources: &[SourceFile],
    depth: Option<usize>,
) -> Result<Vec<ItemKey>, Box<dyn Error>> {
    let defs = collect_definitions(sources);
    let mut results = Vec::new();
    let mut seen_items: HashSet<ItemKey> = HashSet::new();

    let mut queue: VecDeque<(Definition, usize)> = VecDeque::new();
    queue.push_back((target.clone(), 0));

    while let Some((current, distance)) = queue.pop_front() {
        let item = ItemKey {
            kind: current.kind,
            namespace: current.namespace.clone(),
            name: current.name.clone(),
            file_index: current.file_index,
        };
        if !seen_items.insert(item.clone()) {
            continue;
        }
        results.push(item);

        let next_distance = distance + 1;
        if depth.map(|limit| next_distance > limit).unwrap_or(false) {
            continue;
        }

        let adjacent = adjacent_modules(&current, sources, &defs);
        for module in adjacent {
            queue.push_back((module, next_distance));
        }
    }

    let pipeline_funcs = module_pipeline_functions(target, sources, &defs);
    for item in pipeline_funcs {
        if seen_items.insert(item.clone()) {
            results.push(item);
        }
    }
    Ok(results)
}

fn adjacent_modules(
    target: &Definition,
    sources: &[SourceFile],
    defs: &[Definition],
) -> Vec<Definition> {
    let target_schemas = module_schema_refs(target, sources);
    if target_schemas.is_empty() {
        return Vec::new();
    }
    let mut adjacent = Vec::new();
    for def in defs.iter().filter(|def| def.kind == SymbolKind::Mod) {
        if def.name == target.name && def.namespace == target.namespace {
            continue;
        }
        let schemas = module_schema_refs(def, sources);
        if schemas.iter().any(|schema| target_schemas.contains(schema)) {
            adjacent.push(def.clone());
        }
    }
    adjacent
}

fn module_schema_refs(target: &Definition, sources: &[SourceFile]) -> HashSet<String> {
    let mut set = HashSet::new();
    let source = match sources.get(target.file_index) {
        Some(source) => source,
        None => return set,
    };
    let namespace = source.file.namespace.as_deref();
    let module = match find_module(&source.file, &target.name) {
        Some(module) => module,
        None => return set,
    };

    for schema in &module.schemas {
        if let Some(qref) = qualify_reference(schema, namespace) {
            if qref.kind == SymbolKind::Schema {
                set.insert(format_ref(qref.kind, qref.namespace.as_deref(), &qref.name));
            }
        }
    }
    set
}

fn module_pipeline_functions(
    target: &Definition,
    sources: &[SourceFile],
    defs: &[Definition],
) -> Vec<ItemKey> {
    let source = match sources.get(target.file_index) {
        Some(source) => source,
        None => return Vec::new(),
    };
    let namespace = source.file.namespace.as_deref();
    let module = match find_module(&source.file, &target.name) {
        Some(module) => module,
        None => return Vec::new(),
    };
    let mut items = Vec::new();
    for step in module.get_pipeline_calls() {
        if let Some(qref) = qualify_reference(&step, namespace) {
            if qref.kind != SymbolKind::Func {
                continue;
            }
            items.extend(resolve_qualified(&qref, defs, SymbolKind::Func));
        }
    }
    items
}

fn find_schema<'a>(file: &'a SurvFile, name: &str) -> Option<&'a survibe_parser_rs::SchemaSection> {
    file.sections.iter().find_map(|section| match section {
        Section::Schema(schema) if schema.name == name => Some(schema),
        _ => None,
    })
}

fn find_func<'a>(file: &'a SurvFile, name: &str) -> Option<&'a survibe_parser_rs::FuncSection> {
    file.sections.iter().find_map(|section| match section {
        Section::Func(func) if func.name == name => Some(func),
        _ => None,
    })
}

fn find_module<'a>(file: &'a SurvFile, name: &str) -> Option<&'a survibe_parser_rs::ModSection> {
    file.sections.iter().find_map(|section| match section {
        Section::Mod(module) if module.name == name => Some(module),
        _ => None,
    })
}

fn extract_field_references(schema: &survibe_parser_rs::SchemaSection) -> Vec<String> {
    let mut refs = Vec::new();
    for field_type in schema.fields.values() {
        refs.extend(extract_reference_tokens(field_type));
    }
    refs
}

fn extract_reference_tokens(raw: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut buffer = String::new();
    let mut capture = false;
    for ch in raw.chars() {
        if ch.is_alphanumeric() || ch == '_' || ch == '.' {
            buffer.push(ch);
            if buffer.ends_with("schema.") || buffer.ends_with("func.") || buffer.ends_with("mod.")
            {
                capture = true;
            }
        } else {
            if capture {
                if let Some(token) = finalize_reference_token(&buffer) {
                    tokens.push(token);
                }
            }
            buffer.clear();
            capture = false;
        }
    }
    if capture {
        if let Some(token) = finalize_reference_token(&buffer) {
            tokens.push(token);
        }
    }
    tokens
}

fn finalize_reference_token(buffer: &str) -> Option<String> {
    let (kind, _, _) = parse_reference(buffer)?;
    let parts: Vec<&str> = buffer.split('.').collect();
    let mut idx = None;
    for (i, part) in parts.iter().enumerate() {
        if *part == "schema" || *part == "func" || *part == "mod" {
            idx = Some(i);
            break;
        }
    }
    let idx = idx?;
    if idx + 1 >= parts.len() {
        return None;
    }
    let prefix = parts[..idx].join(".");
    let name = parts[idx + 1..].join(".");
    let kind_str = match kind {
        SymbolKind::Schema => "schema",
        SymbolKind::Func => "func",
        SymbolKind::Mod => "mod",
    };
    if prefix.is_empty() {
        Some(format!("{}.{}", kind_str, name))
    } else {
        Some(format!("{}.{}.{}", prefix, kind_str, name))
    }
}

fn schema_reference_sites(
    schema: &survibe_parser_rs::SchemaSection,
    include: RefsInclude,
) -> Vec<String> {
    let mut refs = Vec::new();
    if !schema.base.is_empty() {
        refs.push(schema.base.clone());
    }
    if !schema.from.is_empty() {
        refs.push(schema.from.clone());
    }
    if !schema.to.is_empty() {
        refs.push(schema.to.clone());
    }
    if !schema.r#type.is_empty() {
        refs.push(schema.r#type.clone());
    }
    refs.extend(schema.over.clone());
    refs.extend(extract_field_references(schema));

    if include.impl_meta {
        if let Some(bind) = &schema.impl_bind {
            refs.push(bind.clone());
        }
        if let Some(lang) = &schema.impl_lang {
            refs.push(lang.clone());
        }
        if let Some(path) = &schema.impl_path {
            refs.push(path.clone());
        }
    }
    refs
}

fn func_reference_sites(
    func: &survibe_parser_rs::FuncSection,
    include: RefsInclude,
) -> Vec<String> {
    let mut refs = Vec::new();
    refs.extend(func.input.clone());
    refs.extend(func.output.clone());
    if include.impl_meta {
        if let Some(bind) = &func.impl_bind {
            refs.push(bind.clone());
        }
        if let Some(lang) = &func.impl_lang {
            refs.push(lang.clone());
        }
        if let Some(path) = &func.impl_path {
            refs.push(path.clone());
        }
    }
    refs
}

fn mod_reference_sites(
    module: &survibe_parser_rs::ModSection,
    _include: RefsInclude,
) -> Vec<String> {
    let mut refs = Vec::new();
    refs.extend(module.schemas.clone());
    refs.extend(module.funcs.clone());
    refs.extend(module.get_pipeline_calls());
    refs
}

fn section_references_target(refs: &[String], namespace: Option<&str>, target: &Target) -> bool {
    refs.iter()
        .any(|reference| reference_matches_target(reference, namespace, target))
}

fn reference_matches_target(raw: &str, namespace: Option<&str>, target: &Target) -> bool {
    let ref_token = raw.split_whitespace().next().unwrap_or(raw);
    let Some(reference) = qualify_reference(ref_token, namespace) else {
        return false;
    };
    target_matches_ref(target, &reference)
}

fn emit_slice_output(
    items: &[ItemKey],
    sources: &[SourceFile],
    format: OutputFormat,
    with_defs: bool,
) -> Result<(), Box<dyn Error>> {
    match format {
        OutputFormat::List => {
            for item in items {
                println!(
                    "{}",
                    format_ref(item.kind, item.namespace.as_deref(), &item.name)
                );
            }
        }
        OutputFormat::Json => {
            let output = build_output_items(items, sources);
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        OutputFormat::Toml => {
            if !with_defs {
                return Err("toml format requires --with-defs".into());
            }
            let file_indices: HashSet<usize> = items.iter().map(|item| item.file_index).collect();
            if file_indices.len() != 1 {
                let mut files = Vec::new();
                for index in file_indices {
                    files.push(sources[index].path.display().to_string());
                }
                return Err(format!(
                    "toml output requires a single source file; got: {}",
                    files.join(", ")
                )
                .into());
            }
            let file_index = *file_indices.iter().next().unwrap();
            let source = &sources[file_index];
            let fragment = render_ir_fragment(&source.file, items)?;
            print!("{}", fragment);
        }
    }
    Ok(())
}

fn emit_items_output(
    items: &[ItemKey],
    sources: &[SourceFile],
    format: OutputFormat,
    sort: bool,
) -> Result<(), Box<dyn Error>> {
    match format {
        OutputFormat::List => {
            let mut output = items.to_vec();
            if sort {
                output.sort_by(|a, b| {
                    format_ref(a.kind, a.namespace.as_deref(), &a.name).cmp(&format_ref(
                        b.kind,
                        b.namespace.as_deref(),
                        &b.name,
                    ))
                });
            }
            for item in output {
                println!(
                    "{}",
                    format_ref(item.kind, item.namespace.as_deref(), &item.name)
                );
            }
        }
        OutputFormat::Json => {
            let output = if sort {
                let mut output = build_output_items(items, sources);
                output.sort_by(|a, b| a.name.cmp(&b.name));
                output
            } else {
                build_output_items(items, sources)
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
        }
        OutputFormat::Toml => {
            return Err("toml format is not supported for this command".into());
        }
    }
    Ok(())
}

fn build_output_items(items: &[ItemKey], sources: &[SourceFile]) -> Vec<OutputItem> {
    let mut output = Vec::new();
    for item in items {
        let source = sources
            .get(item.file_index)
            .map(|source| source.path.display().to_string())
            .unwrap_or_else(|| "<unknown>".to_string());
        output.push(OutputItem {
            kind: kind_label(item.kind).to_string(),
            name: format_ref(item.kind, item.namespace.as_deref(), &item.name),
            source,
        });
    }
    output
}

fn kind_label(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Schema => "schema",
        SymbolKind::Func => "func",
        SymbolKind::Mod => "mod",
    }
}

fn render_ir_fragment(file: &SurvFile, items: &[ItemKey]) -> Result<String, Box<dyn Error>> {
    let mut output = String::new();

    if let Some(package) = &file.package {
        output.push_str(&format!("package = \"{}\"\n", escape_toml_string(package)));
    }
    if let Some(namespace) = &file.namespace {
        output.push_str(&format!(
            "namespace = \"{}\"\n",
            escape_toml_string(namespace)
        ));
    }
    if !file.imports.is_empty() {
        let imports: Vec<String> = file
            .imports
            .iter()
            .map(|imp| match &imp.alias {
                Some(alias) => format!("\"{} as {}\"", imp.target, alias),
                None => format!("\"{}\"", imp.target),
            })
            .collect();
        output.push_str(&format!("import = [{}]\n", imports.join(", ")));
    }
    if !file.requires.is_empty() {
        let requires: Vec<String> = file
            .requires
            .iter()
            .map(|req| format!("\"{}\"", req.target))
            .collect();
        output.push_str(&format!("require = [{}]\n", requires.join(", ")));
    }
    if !output.is_empty() {
        output.push('\n');
    }

    let mut wanted: HashSet<(SymbolKind, String)> = HashSet::new();
    for item in items {
        wanted.insert((item.kind, item.name.clone()));
    }

    for section in &file.sections {
        match section {
            Section::Schema(schema) => {
                if wanted.contains(&(SymbolKind::Schema, schema.name.clone())) {
                    output.push_str(&format!("[schema.{}]\n", schema.name));
                    output.push_str(&format!(
                        "kind = \"{}\"\n",
                        escape_toml_string(&schema.kind)
                    ));
                    if !schema.role.is_empty() {
                        output.push_str(&format!(
                            "role = \"{}\"\n",
                            escape_toml_string(&schema.role)
                        ));
                    }
                    if !schema.r#type.is_empty() {
                        output.push_str(&format!(
                            "type = \"{}\"\n",
                            escape_toml_string(&schema.r#type)
                        ));
                    }
                    if !schema.from.is_empty() {
                        output.push_str(&format!(
                            "from = \"{}\"\n",
                            escape_toml_string(&schema.from)
                        ));
                    }
                    if !schema.to.is_empty() {
                        output.push_str(&format!("to = \"{}\"\n", escape_toml_string(&schema.to)));
                    }
                    if !schema.base.is_empty() {
                        output.push_str(&format!(
                            "base = \"{}\"\n",
                            escape_toml_string(&schema.base)
                        ));
                    }
                    if !schema.label.is_empty() {
                        output.push_str(&format!(
                            "label = \"{}\"\n",
                            escape_toml_string(&schema.label)
                        ));
                    }
                    if !schema.fields.is_empty() {
                        let fields: Vec<String> = schema
                            .fields
                            .iter()
                            .map(|(key, value)| {
                                format!("{} = \"{}\"", key, escape_toml_string(value))
                            })
                            .collect();
                        output.push_str(&format!("fields = {{{}}}\n", fields.join(", ")));
                    }
                    if !schema.over.is_empty() {
                        output.push_str(&format!("over = {:?}\n", schema.over));
                    }
                    if let Some(bind) = &schema.impl_bind {
                        output.push_str(&format!("impl.bind = \"{}\"\n", escape_toml_string(bind)));
                    }
                    if let Some(lang) = &schema.impl_lang {
                        output.push_str(&format!("impl.lang = \"{}\"\n", escape_toml_string(lang)));
                    }
                    if let Some(path) = &schema.impl_path {
                        output.push_str(&format!("impl.path = \"{}\"\n", escape_toml_string(path)));
                    }
                    output.push('\n');
                }
            }
            Section::Func(func) => {
                if wanted.contains(&(SymbolKind::Func, func.name.clone())) {
                    output.push_str(&format!("[func.{}]\n", func.name));
                    if !func.intent.is_empty() {
                        output.push_str(&format!(
                            "intent = \"{}\"\n",
                            escape_toml_string(&func.intent)
                        ));
                    }
                    if !func.input.is_empty() {
                        output.push_str(&format!("input = {:?}\n", func.input));
                    }
                    if !func.output.is_empty() {
                        output.push_str(&format!("output = {:?}\n", func.output));
                    }
                    if !func.design_notes.is_empty() {
                        output.push_str(&format!(
                            "design_notes = \"{}\"\n",
                            escape_toml_string(&func.design_notes)
                        ));
                    }
                    if let Some(bind) = &func.impl_bind {
                        output.push_str(&format!("impl.bind = \"{}\"\n", escape_toml_string(bind)));
                    }
                    if let Some(lang) = &func.impl_lang {
                        output.push_str(&format!("impl.lang = \"{}\"\n", escape_toml_string(lang)));
                    }
                    if let Some(path) = &func.impl_path {
                        output.push_str(&format!("impl.path = \"{}\"\n", escape_toml_string(path)));
                    }
                    output.push('\n');
                }
            }
            Section::Mod(module) => {
                if wanted.contains(&(SymbolKind::Mod, module.name.clone())) {
                    output.push_str(&format!("[mod.{}]\n", module.name));
                    if !module.purpose.is_empty() {
                        output.push_str(&format!(
                            "purpose = \"{}\"\n",
                            escape_toml_string(&module.purpose)
                        ));
                    }
                    if !module.schemas.is_empty() {
                        output.push_str(&format!("schemas = {:?}\n", module.schemas));
                    }
                    if !module.funcs.is_empty() {
                        output.push_str(&format!("funcs = {:?}\n", module.funcs));
                    }
                    if !module.pipeline.is_empty() {
                        output.push_str(&format!("pipeline = {:?}\n", module.pipeline));
                    }
                    output.push('\n');
                }
            }
            Section::Meta(_) | Section::Status(_) => {}
        }
    }

    Ok(output)
}

fn escape_toml_string(raw: &str) -> String {
    raw.replace('\\', "\\\\").replace('"', "\\\"")
}

fn print_slice_usage() {
    eprintln!("Usage: surc slice <target> <file> [options]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --include schemas,funcs,mods   Select which kinds to include (default: all)");
    eprintln!("  --with-defs                    Include definitions in output (implies toml)");
    eprintln!("  --closure                      Include transitive dependencies");
    eprintln!("  --mod <mod>                    Resolve pipeline adjacency within module context");
    eprintln!("  --format list|json|toml        Output format (default: list)");
}

fn print_refs_usage() {
    eprintln!("Usage: surc refs <target> <file> [options]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --format list|json             Output format (default: list)");
    eprintln!("  --kind mod|func|schema         Filter referrer kind");
    eprintln!("  --include import,impl,boundary|all  Opt-in extra reference sites");
}

fn print_trace_usage() {
    eprintln!("Usage: surc trace <target> <file> [options]");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  --up                           Trace upstream only");
    eprintln!("  --down                         Trace downstream only");
    eprintln!("  --both                         Trace both directions (default)");
    eprintln!("  --mod <mod>                    Resolve pipeline within module context");
    eprintln!("  --depth N                      Limit traversal depth");
    eprintln!("  --format list|json             Output format (default: list)");
}

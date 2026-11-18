use crate::simple_toml::legacy_status_ranges;
use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::ops::Range;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use toml_edit::{DocumentMut, Item, Table, TableLike, Value};

pub struct StatusDocument<'a> {
    original: &'a str,
    document: DocumentMut,
    legacy_ranges: Option<Vec<Range<usize>>>,
}

impl<'a> StatusDocument<'a> {
    pub fn parse(original: &'a str) -> Result<Self, Box<dyn Error>> {
        let (document, legacy_ranges) = match original.parse() {
            Ok(document) => (document, None),
            Err(_) => {
                let ranges = legacy_status_ranges(original)?;
                let status: String = ranges
                    .iter()
                    .map(|range| &original[range.clone()])
                    .collect();
                let status = quote_legacy_status_values(&status);
                (status.parse()?, Some(ranges))
            }
        };
        Ok(Self {
            original,
            document,
            legacy_ranges,
        })
    }

    pub fn status(&mut self, create: bool) -> Option<&mut dyn TableLike> {
        table_mut(self.document.as_table_mut(), "status", create)
    }

    pub fn finish(self) -> String {
        let updated = self.document.to_string();
        match self.legacy_ranges {
            None => updated,
            Some(ranges) if ranges.is_empty() => format!("{}\n\n{updated}", self.original),
            Some(ranges) => {
                let mut result = self.original.to_string();
                for range in ranges.iter().skip(1).rev() {
                    result.replace_range(range.clone(), "");
                }
                result.replace_range(ranges[0].clone(), &updated);
                result
            }
        }
    }
}

fn quote_legacy_status_values(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for line in input.lines() {
        let trimmed = line.trim_start();
        let Some((key, raw_value)) = trimmed.split_once('=') else {
            out.push_str(line);
            out.push('\n');
            continue;
        };
        let key = key.trim();
        if !matches!(key, "state" | "notes") {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let raw_value = raw_value.trim();
        if raw_value.is_empty() || raw_value.starts_with('"') || raw_value.starts_with('\'') {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        let (value, comment) = match raw_value.split_once('#') {
            Some((value, comment)) => (value.trim(), Some(comment.trim())),
            None => (raw_value, None),
        };
        let indent = &line[..line.len() - trimmed.len()];
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        out.push_str(indent);
        out.push_str(key);
        out.push_str(" = \"");
        out.push_str(&escaped);
        out.push('"');
        if let Some(comment) = comment {
            out.push_str(" # ");
            out.push_str(comment);
        }
        out.push('\n');
    }
    out
}

pub fn status_module_mut<'a>(
    status: &'a mut dyn TableLike,
    name: &str,
    create: bool,
) -> Option<&'a mut dyn TableLike> {
    let mut modules = table_mut(status, "mod", create)?;
    let mut remaining = name;
    loop {
        if modules.contains_key(remaining) || !remaining.contains('.') {
            return table_mut(modules, remaining, create);
        }
        let (parent, child) = remaining.split_once('.')?;
        modules = table_mut(modules, parent, create)?;
        remaining = child;
    }
}

fn table_mut<'a>(
    table: &'a mut dyn TableLike,
    key: &str,
    create: bool,
) -> Option<&'a mut dyn TableLike> {
    if create && !table.contains_key(key) {
        let mut child = Table::new();
        child.set_implicit(true);
        table.insert(key, Item::Table(child));
    }
    table.get_mut(key)?.as_table_like_mut()
}

pub fn set_status_value(table: &mut dyn TableLike, key: &str, value: impl Into<Value>) {
    let mut value = value.into();
    if let Some(current) = table.get_mut(key) {
        if let Some(old) = current.as_value() {
            *value.decor_mut() = old.decor().clone();
        }
        *current = Item::Value(value);
    } else {
        table.insert(key, Item::Value(value));
    }
}

/// Replace `path` with `updated` only if it still contains `original`.
///
/// The write is atomic: a temporary file in the same directory is fsynced and
/// renamed over the target, so a crash cannot leave a truncated status block.
pub fn write_atomic(path: &Path, original: &str, updated: &str) -> io::Result<()> {
    let path = path.canonicalize()?;
    let permissions = fs::metadata(&path)?.permissions();
    if permissions.readonly() {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "IR file is read-only",
        ));
    }
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let parent = path.parent().expect("canonical files have a parent");
    let (temporary, mut file) = loop {
        let ordinal = NEXT.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(".surc-status-{}-{ordinal}.tmp", std::process::id()));
        match OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
        {
            Ok(file) => break (temporary, file),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    };
    let result = (|| {
        file.write_all(updated.as_bytes())?;
        file.set_permissions(permissions)?;
        file.sync_all()?;
        drop(file);
        if fs::read_to_string(&path)? != original {
            return Err(io::Error::other("IR file changed during status update"));
        }
        fs::rename(&temporary, &path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

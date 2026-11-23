use std::error::Error;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use toml_edit::{DocumentMut, Item, Table, TableLike, Value};

pub struct StatusDocument {
    document: DocumentMut,
}

impl StatusDocument {
    pub fn parse(original: &str) -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            document: original.parse()?,
        })
    }

    pub fn status(&mut self, create: bool) -> Option<&mut dyn TableLike> {
        table_mut(self.document.as_table_mut(), "status", create)
    }

    pub fn finish(self) -> String {
        self.document.to_string()
    }
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

use std::path::Path;

use ql_ast::LanguageAdapter;

pub mod go;
pub mod python;
pub mod rust;
pub mod typescript;

pub use go::GoAdapter;
pub use python::PythonAdapter;
pub use rust::RustAdapter;
pub use typescript::TypeScriptAdapter;

static GO: GoAdapter = GoAdapter;
static RUST: RustAdapter = RustAdapter;
static TYPESCRIPT: TypeScriptAdapter = TypeScriptAdapter;
static PYTHON: PythonAdapter = PythonAdapter;
static ADAPTERS: [&'static dyn LanguageAdapter; 4] = [&GO, &RUST, &TYPESCRIPT, &PYTHON];

pub fn adapters() -> &'static [&'static dyn LanguageAdapter] {
    &ADAPTERS
}

pub fn adapter_for_path(path: &Path) -> Option<&'static dyn LanguageAdapter> {
    let ext = path.extension()?.to_str()?;
    let ext = format!(".{ext}");
    adapters()
        .iter()
        .copied()
        .find(|adapter| adapter.extensions().iter().any(|candidate| *candidate == ext))
}

pub fn supported_languages() -> Vec<String> {
    adapters()
        .iter()
        .map(|adapter| format!("{} ({})", adapter.language_name(), adapter.extensions().join(", ")))
        .collect()
}

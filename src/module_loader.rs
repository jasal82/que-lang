/// Module loader for the Que language.
///
/// Resolves import paths to files, loads/parses/executes them, caches
/// results, and detects circular imports. Implements the spec §30 module
/// system:
///
///   - Bare identifier paths  → external (std or que_packages/)
///   - Dot-prefixed paths     → local (package-root-relative)
///   - `mod.que` convention  → directory modules
///   - Single evaluation      → each module loaded once, cached by absolute path

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::ast::{ImportDecl, Item, Module, Pattern};
use crate::error::{QueError, ErrorKind, Signal};
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::value::{Value, FieldDef, MethodDef};

/// Shared module cache and loading state. Passed through all interpreter
/// instances so that each module is loaded and executed exactly once.
#[derive(Debug, Clone)]
pub struct ModuleLoader {
    /// Resolved absolute path → cached pub exports (as a Map value).
    cache: HashMap<PathBuf, Value>,
    /// Set of modules currently being loaded (cycle detection).
    loading: HashSet<PathBuf>,
    /// The package root directory (contains que.toml, or the script dir).
    package_root: PathBuf,
    /// Accumulated output from module loading (println in sub-modules).
    /// Only populated when direct_output is false.
    pub pending_output: Vec<String>,
    /// When true, sub-interpreters write output directly to stdout.
    pub direct_output: bool,
    /// Struct field definitions from exported types (pending merge into parent interpreter).
    pub pending_struct_defs: HashMap<String, Vec<FieldDef>>,
    /// Impl methods from exported types (pending merge into parent interpreter).
    pub pending_impl_methods: HashMap<String, Vec<MethodDef>>,
    /// Trait implementations from exported types (pending merge into parent interpreter).
    pub pending_trait_impls: HashMap<(String, String), Vec<MethodDef>>,
    /// Enum definitions from exported enum types (pending merge into parent interpreter).
    pub pending_enum_defs: HashMap<String, Vec<(String, Vec<String>)>>,
    /// Reverse mapping for imported enum variants: variant_name -> enum_name.
    pub pending_enum_variant_to_enum: HashMap<String, String>,
}

impl ModuleLoader {
    pub fn new(package_root: PathBuf) -> Self {
        Self {
            cache: HashMap::new(),
            loading: HashSet::new(),
            package_root,
            pending_output: Vec::new(),
            direct_output: false,
            pending_struct_defs: HashMap::new(),
            pending_impl_methods: HashMap::new(),
            pending_trait_impls: HashMap::new(),
            pending_enum_defs: HashMap::new(),
            pending_enum_variant_to_enum: HashMap::new(),
        }
    }

    /// Return a reference to the package root.
    pub fn package_root(&self) -> &Path {
        &self.package_root
    }

    /// Resolve an import declaration to a file path, load the module,
    /// and return its public exports as a `Value::Map`.
    ///
    /// `caller_dir` is the directory of the file containing the import
    /// statement. Dot-prefixed (local) imports are resolved relative to
    /// this directory, so `import .cursor` inside `escapes/mod.que` finds
    /// `escapes/cursor.que` rather than anchoring at the project root.
    ///
    /// For multi-module shorthand (`import std.{fs, path}`) and selective
    /// imports (`import std.fs { readText }`), the interpreter handles
    /// binding — this function always returns the full module map.
    pub fn load_import(
        &mut self,
        decl: &ImportDecl,
        caller_dir: &Path,
    ) -> Result<Vec<(String, Value)>, QueError> {
        // Multi-module shorthand with empty path: `import .{a, b}` or `import std.{a, b}`
        // The parser puts the trailing `{...}` into `items` and the prefix into `path`.
        // For `import std.{fs, path}`, path=["std"], items=Some(["fs","path"])
        // For `import .{utils, config}`, path=[], is_local=true, items=Some(["utils","config"])
        //
        // Each item in the shorthand is a separate module that gets loaded.
        if let Some(ref items) = decl.items {
            // Check if this is a multi-module shorthand (items come from `parent.{a, b}`)
            // vs selective imports (items come from `import mod { fn1, fn2 }`)
            //
            // Heuristic: if the items were parsed from `path.{items}`, we treat
            // them as sub-module names to load individually.  If from `path { items }`,
            // they are selective imports from a single module.
            //
            // The parser signals multi-module shorthand by producing items via
            // the `.{` branch. We need to distinguish:
            //   import std.{fs, path}       →  path=["std"], items=["fs","path"]  → multi-module
            //   import std.fs { readText }  →  path=["std","fs"], items=["readText"] → selective
            //
            // In the parser, multi-module `parent.{a,b}` returns early with
            // path = prefix segments (before the `{`). Selective `mod { a, b }`
            // finishes parsing the full path, then parses `{ items }`.
            //
            // So: if the last parsed path segment is NOT a known module but the
            // items look like module names, it's multi-module.  But this requires
            // filesystem probing which is fragile.
            //
            // Better approach: we always try to load path as a module first.
            // If path resolves to a file, treat items as selective imports.
            // If path does not resolve to a file, treat items as sub-module names.
            
            let base_path = if decl.is_local {
                self.resolve_local_path(&decl.path, caller_dir)
            } else {
                self.resolve_external_path(&decl.path)
            };
            
            match base_path {
                Some(file_path) if decl.path.len() >= 1 => {
                    // Path resolves to a file → selective imports from that module
                    let module_val = self.load_module_file(&file_path)?;
                    return Ok(vec![("__selective__".to_string(), module_val)]);
                }
                _ => {
                    // Path doesn't resolve to a file, or empty path (`.{a,b}`) 
                    // → multi-module shorthand: each item is a separate module
                    let mut results = Vec::new();
                    for item_name in items {
                        let mut sub_path = decl.path.clone();
                        sub_path.push(item_name.clone());
                        let file_path = if decl.is_local {
                            self.resolve_local_path(&sub_path, caller_dir)
                        } else {
                            self.resolve_external_path(&sub_path)
                        };
                        match file_path {
                            Some(fp) => {
                                let module_val = self.load_module_file(&fp)?;
                                results.push((item_name.clone(), module_val));
                            }
                            None => {
                                let full = if decl.is_local {
                                    format!(".{}", sub_path.join("."))
                                } else {
                                    sub_path.join(".")
                                };
                                return Err(QueError::new(
                                    ErrorKind::IoError,
                                    format!("module not found: {}", full),
                                ));
                            }
                        }
                    }
                    return Ok(results);
                }
            }
        }

        // Single module import (no `{ ... }`)
        let file_path = if decl.is_local {
            self.resolve_local_path(&decl.path, caller_dir)
        } else {
            self.resolve_external_path(&decl.path)
        };

        match file_path {
            Some(fp) => {
                let module_val = self.load_module_file(&fp)?;
                // Determine the binding name
                let name = if let Some(ref alias) = decl.alias {
                    alias.clone()
                } else {
                    // Last segment of path
                    decl.path.last().cloned().unwrap_or_else(|| "module".to_string())
                };
                Ok(vec![(name, module_val)])
            }
            None => {
                let full = if decl.is_local {
                    format!(".{}", decl.path.join("."))
                } else {
                    decl.path.join(".")
                };
                let hint = if !decl.is_local && !decl.path.is_empty() && decl.path[0] != "std" {
                    self.external_import_hint(&decl.path[0])
                } else {
                    String::new()
                };
                Err(QueError::new(
                    ErrorKind::IoError,
                    format!("module not found: {}{}", full, hint),
                ))
            }
        }
    }

    /// Say what to do about a bare import that did not resolve.
    ///
    /// The two situations need different advice: a package the manifest
    /// already declares just has not been fetched, while one it does not
    /// declare has to be declared first.
    fn external_import_hint(&self, pkg: &str) -> String {
        let declared = crate::manifest::load(&self.package_root)
            .ok()
            .flatten()
            .map(|m| m.dependencies.iter().any(|d| d.dir_name == pkg.replace('-', "_")))
            .unwrap_or(false);
        if declared {
            format!("\nHint: '{}' is in que.toml but not installed. Run `que install`.", pkg)
        } else {
            format!(
                "\nHint: to use an external package, add it under [dependencies] in que.toml \
                 and run `que install`:\n  {} = {{ git = \"<url>\", tag = \"<version>\" }}",
                pkg
            )
        }
    }

    /// Resolve a local (dot-prefixed) import path to an absolute file path,
    /// relative to the directory of the importing file.    /// `import .utils` from `lib/mod.que`    → `lib/utils.que` or `lib/utils/mod.que`
    /// `import .lib.build` from `main.que`   → `lib/build.que` or `lib/build/mod.que`
    fn resolve_local_path(&self, segments: &[String], caller_dir: &Path) -> Option<PathBuf> {
        let mut dir = caller_dir.to_path_buf();
        for seg in segments {
            dir = dir.join(seg);
        }
        try_resolve_file(&dir)
    }

    /// Resolve an external (bare identifier) import path.
    /// First segment is the package name. Resolution order:
    ///   1. `std` → built-in (currently no std files, returns None)
    ///   2. `que_packages/<pkg>/` + remaining path
    fn resolve_external_path(&self, segments: &[String]) -> Option<PathBuf> {
        if segments.is_empty() {
            return None;
        }

        let pkg_name = &segments[0];

        // std is reserved for the standard library. In v0.1, std modules
        // don't exist as files — they're built-in. Return None so the
        // interpreter can handle std specially.
        if pkg_name == "std" {
            return None;
        }

        // Look in que_packages/ (with hyphen→underscore normalization)
        let normalized = pkg_name.replace('-', "_");
        let pkg_dir = self.package_root.join("que_packages").join(&normalized);

        if !pkg_dir.is_dir() {
            return None;
        }

        if segments.len() == 1 {
            // `import deploy_tools` → `que_packages/deploy_tools/mod.que`
            try_resolve_file(&pkg_dir.join("mod"))
        } else {
            // `import deploy_tools.k8s` → que_packages/deploy_tools/k8s.que
            let mut dir = pkg_dir;
            for seg in &segments[1..] {
                dir = dir.join(seg);
            }
            try_resolve_file(&dir)
        }
    }

    /// Load a module from a resolved file path, returning its pub exports
    /// as a `Value::Map`. Uses the cache to ensure single evaluation.
    fn load_module_file(&mut self, path: &Path) -> Result<Value, QueError> {
        let abs = std::fs::canonicalize(path).map_err(|e| {
            QueError::new(
                ErrorKind::IoError,
                format!("cannot resolve module path '{}': {}", path.display(), e),
            )
        })?;

        // Return cached result if already loaded
        if let Some(cached) = self.cache.get(&abs) {
            return Ok(cached.clone());
        }

        // Cycle detection
        if self.loading.contains(&abs) {
            let cycle_path = self
                .loading
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(" → ");
            return Err(QueError::new(
                ErrorKind::Runtime,
                format!(
                    "circular import detected: {} → {}",
                    cycle_path,
                    abs.display()
                ),
            ));
        }

        self.loading.insert(abs.clone());

        // Read source
        let source = std::fs::read_to_string(&abs).map_err(|e| {
            QueError::new(
                ErrorKind::IoError,
                format!("cannot read module '{}': {}", abs.display(), e),
            )
        })?;

        // Lex & parse
        let mut lexer = Lexer::new(&source);
        let tokens = lexer.tokenize()?;
        let mut parser = Parser::new(tokens);
        let module = parser.parse_module()?;

        // Execute in a sub-interpreter that shares our module cache.
        // The sub-interpreter gets a fresh environment but the same loader.
        let exports = self.execute_module(&module, &abs)?;

        self.loading.remove(&abs);
        self.cache.insert(abs, exports.clone());

        Ok(exports)
    }

    /// Execute a parsed module, collecting its `pub` declarations into a Map.
    fn execute_module(&mut self, module: &Module, module_path: &Path) -> Result<Value, QueError> {
        use crate::interpreter::Interpreter;

        let mut interp = Interpreter::new();
        interp.direct_output = self.direct_output;
        // Share the module loader (pass self in)
        interp.set_script_path(module_path.to_path_buf());

        // We need to swap loaders: the sub-interpreter uses OUR loader
        // (with the shared cache and loading set).
        let _ = interp.take_module_loader();
        interp.set_module_loader(self.clone());

        // Execute all items
        match interp.exec_module(module) {
            Ok(_) => {}
            Err(Signal::Error(e)) => return Err(e),
            Err(Signal::Return(_)) => {}
            Err(signal) => {
                return Err(QueError::new(
                    ErrorKind::Runtime,
                    format!("unexpected signal in module: {:?}", signal),
                ));
            }
        }

        // Get the updated loader back (with new cache entries)
        *self = interp.take_module_loader();

        // Propagate the sub-interpreter's output (println in modules)
        interp.flush_partial();
        self.pending_output.append(&mut interp.output);

        // Collect pub exports from the module's items
        let mut exports = BTreeMap::new();
        let mut exported_type_names: Vec<String> = Vec::new();
        let mut exported_enum_names: Vec<String> = Vec::new();
        for (_, item) in &module.items {
            match item {
                Item::FnDecl(decl) if decl.is_pub => {
                    if let Some(val) = interp.env.get(&decl.name) {
                        exports.insert(decl.name.clone(), val);
                    }
                }
                Item::StructDecl(decl) if decl.is_pub => {
                    // Export the TypeRef so the importer can reference the type
                    if let Some(val) = interp.env.get(&decl.name) {
                        exports.insert(decl.name.clone(), val);
                    }
                    exported_type_names.push(decl.name.clone());
                }
                Item::PubLet { pattern, .. } => {
                    // Extract all names bound by the pattern and export them
                    let names = pattern_names(pattern);
                    for name in names {
                        if let Some(val) = interp.env.get(&name) {
                            exports.insert(name, val);
                        }
                    }
                }
                Item::TypeDecl(decl) if decl.is_pub => {
                    // Type declarations are compile-time only in v0.1
                    let _ = decl;
                }
                Item::EnumDecl(decl) if decl.is_pub => {
                    // Export the enum TypeRef so callers can do EnumName.Variant(...)
                    if let Some(val) = interp.env.get(&decl.name) {
                        exports.insert(decl.name.clone(), val);
                    }
                    exported_enum_names.push(decl.name.clone());
                    // Export unit variants as direct values
                    for variant in &decl.variants {
                        if variant.fields.is_empty() {
                            if let Some(val) = interp.env.get(&variant.name) {
                                exports.insert(variant.name.clone(), val);
                            }
                        }
                    }
                }
                Item::Import(decl) if decl.is_pub => {
                    // Re-exports: the imported names become part of this module's exports
                    // They were already bound in the interpreter's env during exec_module
                    if let Some(ref items) = decl.items {
                        for item_name in items {
                            if let Some(val) = interp.env.get(item_name) {
                                exports.insert(item_name.clone(), val);
                            }
                        }
                    } else if let Some(ref alias) = decl.alias {
                        if let Some(val) = interp.env.get(alias) {
                            exports.insert(alias.clone(), val);
                        }
                    } else if let Some(name) = decl.path.last() {
                        if let Some(val) = interp.env.get(name) {
                            exports.insert(name.clone(), val);
                        }
                    }
                }
                Item::TaskDecl(decl) => {
                    // Tasks are always exported (they're the primary CI/CD artifact)
                    if let Some(val) = interp.env.get(&decl.name) {
                        exports.insert(decl.name.clone(), val);
                    }
                }
                _ => {}
            }
        }

        // Propagate struct metadata for exported types into pending fields.
        // This allows the parent interpreter to construct instances of imported structs.
        for type_name in &exported_type_names {
            if let Some(fields) = interp.struct_defs.get(type_name) {
                self.pending_struct_defs.insert(type_name.clone(), fields.clone());
            }
            if let Some(methods) = interp.impl_methods.get(type_name) {
                self.pending_impl_methods.insert(type_name.clone(), methods.clone());
            }
            // Copy all trait impls for this type
            for ((tname, trait_name), methods) in &interp.trait_impls {
                if tname == type_name {
                    self.pending_trait_impls
                        .entry((tname.clone(), trait_name.clone()))
                        .or_default()
                        .extend(methods.clone());
                }
            }
        }

        // Propagate enum metadata and methods for exported enum types.
        for enum_name in &exported_enum_names {
            if let Some(variants) = interp.enum_defs.get(enum_name) {
                self.pending_enum_defs.insert(enum_name.clone(), variants.clone());
                for (variant_name, _) in variants {
                    self.pending_enum_variant_to_enum
                        .insert(variant_name.clone(), enum_name.clone());
                }
            }
            if let Some(methods) = interp.impl_methods.get(enum_name) {
                self.pending_impl_methods.insert(enum_name.clone(), methods.clone());
            }
            for ((tname, trait_name), methods) in &interp.trait_impls {
                if tname == enum_name {
                    self.pending_trait_impls
                        .entry((tname.clone(), trait_name.clone()))
                        .or_default()
                        .extend(methods.clone());
                }
            }
        }

        Ok(Value::Map(exports))
    }
}

/// Extract all identifier names bound by a pattern (recursively).
fn pattern_names(pattern: &Pattern) -> Vec<String> {
    match pattern {
        Pattern::Ident(name) => vec![name.clone()],
        Pattern::Tuple(pats) => pats.iter().flat_map(pattern_names).collect(),
        Pattern::List(pats, rest) => {
            let mut names: Vec<String> = pats.iter().flat_map(pattern_names).collect();
            if let Some(rest_pat) = rest {
                names.extend(pattern_names(rest_pat));
            }
            names
        }
        Pattern::Struct(fields, rest) => {
            let mut names: Vec<String> = fields.iter().map(|(name, _)| name.clone()).collect();
            if let Some(rest_name) = rest {
                names.push(rest_name.clone());
            }
            names
        }
        Pattern::Binding(name, _) => vec![name.clone()],
        _ => vec![],
    }
}

/// Try `<base>.que` first, then `<base>/mod.que`.
fn try_resolve_file(base: &Path) -> Option<PathBuf> {
    let with_ext = base.with_extension("que");
    if with_ext.is_file() {
        return Some(with_ext);
    }
    let mod_file = base.join("mod.que");
    if mod_file.is_file() {
        return Some(mod_file);
    }
    None
}

/// Walk up from `start_dir` looking for a directory containing `que.toml`.
/// If none is found, return `start_dir` itself (scripts without manifests).
pub fn find_package_root(start_dir: &Path) -> PathBuf {
    let mut dir = start_dir.to_path_buf();
    loop {
        if dir.join("que.toml").is_file() {
            return dir;
        }
        if !dir.pop() {
            return start_dir.to_path_buf();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_package_root_fallback() {
        // When no que.toml exists, returns the start dir
        let tmp = std::env::temp_dir().join("que_test_no_manifest");
        let _ = std::fs::create_dir_all(&tmp);
        assert_eq!(find_package_root(&tmp), tmp);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_try_resolve_file() {
        let tmp = std::env::temp_dir().join("que_test_resolve");
        let _ = std::fs::create_dir_all(&tmp);

        // Create utils.que
        let utils = tmp.join("utils.que");
        std::fs::write(&utils, "pub fn hello() { \"hi\" }").unwrap();

        assert_eq!(try_resolve_file(&tmp.join("utils")), Some(utils.clone()));
        assert_eq!(try_resolve_file(&tmp.join("nonexistent")), None);

        // Create lib/mod.que (directory module)
        let lib_dir = tmp.join("lib");
        std::fs::create_dir_all(&lib_dir).unwrap();
        let mod_file = lib_dir.join("mod.que");
        std::fs::write(&mod_file, "pub fn greet() { \"hello\" }").unwrap();

        assert_eq!(try_resolve_file(&tmp.join("lib")), Some(mod_file));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}

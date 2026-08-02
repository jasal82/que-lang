/// Variable environment with lexical scoping for the Que interpreter.
///
/// Scopes are reference-counted (`Arc<Mutex<…>>`), so cloning an
/// `Environment` (e.g. for closure capture) **shares** the underlying
/// scope data.  Mutations made through one handle (inside the closure)
/// are visible through every other handle that points to the same scope.
///
/// `Arc<Mutex<…>>` rather than `Rc<RefCell<…>>` because `parallel`
/// branches run on real threads: each branch gets its own handle, so the
/// scopes it pushes are private, while the outer scopes it shares with its
/// siblings are serialised by the lock. Every lock here is taken and
/// released inside a single method, so no two are ever held at once.

use crate::value::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Debug, Clone)]
struct Binding {
    value: Value,
    mutable: bool,
}

type Scope = HashMap<String, Binding>;

/// Take a scope lock, recovering from poisoning.
///
/// A panic in one `parallel` branch should not make every variable in the
/// program permanently unreadable: the map behind the lock is a plain
/// `HashMap` that no half-finished write can leave inconsistent.
fn lock(scope: &Mutex<Scope>) -> MutexGuard<'_, Scope> {
    scope.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Debug, Clone)]
pub struct Environment {
    scopes: Vec<Arc<Mutex<Scope>>>,
}

impl Environment {
    pub fn new() -> Self {
        Self {
            scopes: vec![Arc::new(Mutex::new(HashMap::new()))],
        }
    }

    /// Push a new scope.
    pub fn push_scope(&mut self) {
        self.scopes.push(Arc::new(Mutex::new(HashMap::new())));
    }

    /// Pop the current scope.
    pub fn pop_scope(&mut self) {
        if self.scopes.len() > 1 {
            self.scopes.pop();
        }
    }

    /// Define a new variable in the current scope.
    pub fn define(&mut self, name: &str, value: Value, mutable: bool) {
        if let Some(scope) = self.scopes.last() {
            lock(scope).insert(
                name.to_string(),
                Binding {
                    value,
                    mutable,
                },
            );
        }
    }

    /// Get the value of a variable by searching scopes inside-out.
    ///
    /// Returns an owned clone because the underlying storage is behind
    /// a lock and we cannot hand out long-lived references.
    pub fn get(&self, name: &str) -> Option<Value> {
        for scope in self.scopes.iter().rev() {
            let scope_ref = lock(scope);
            if let Some(binding) = scope_ref.get(name) {
                return Some(binding.value.clone());
            }
        }
        None
    }

    /// Set a variable's value (must already exist and be mutable).
    pub fn set(&mut self, name: &str, value: Value) -> Result<(), String> {
        for scope in self.scopes.iter().rev() {
            let mut scope_ref = lock(scope);
            if let Some(binding) = scope_ref.get_mut(name) {
                if !binding.mutable {
                    return Err(format!("cannot assign to immutable variable '{}'", name));
                }
                binding.value = value;
                return Ok(());
            }
        }
        Err(format!("undefined variable '{}'", name))
    }

    /// Check if a variable exists.
    pub fn contains(&self, name: &str) -> bool {
        self.scopes.iter().rev().any(|s| lock(s).contains_key(name))
    }

    /// Check if a variable is mutable.
    pub fn is_mutable(&self, name: &str) -> Option<bool> {
        for scope in self.scopes.iter().rev() {
            let scope_ref = lock(scope);
            if let Some(binding) = scope_ref.get(name) {
                return Some(binding.mutable);
            }
        }
        None
    }

    /// List all variable names visible in the current scope chain,
    /// with their values and mutability. Variables in inner scopes
    /// shadow those in outer scopes (only the innermost is returned).
    pub fn list_vars(&self) -> Vec<(String, Value, bool)> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        for scope in self.scopes.iter().rev() {
            let scope_ref = lock(scope);
            for (name, binding) in scope_ref.iter() {
                if seen.insert(name.clone()) {
                    result.push((name.clone(), binding.value.clone(), binding.mutable));
                }
            }
        }
        result.sort_by(|a, b| a.0.cmp(&b.0));
        result
    }

    /// Return the current scope depth (0 = global).
    pub fn scope_depth(&self) -> usize {
        self.scopes.len().saturating_sub(1)
    }
}

impl Default for Environment {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_define_and_get() {
        let mut env = Environment::new();
        env.define("x", Value::Int(42), false);
        assert_eq!(env.get("x"), Some(Value::Int(42)));
    }

    #[test]
    fn test_undefined_variable() {
        let env = Environment::new();
        assert_eq!(env.get("x"), None);
    }

    #[test]
    fn test_nested_scopes() {
        let mut env = Environment::new();
        env.define("x", Value::Int(1), false);
        env.push_scope();
        env.define("y", Value::Int(2), false);
        assert_eq!(env.get("x"), Some(Value::Int(1))); // visible from parent
        assert_eq!(env.get("y"), Some(Value::Int(2)));
        env.pop_scope();
        assert_eq!(env.get("y"), None); // no longer visible
        assert_eq!(env.get("x"), Some(Value::Int(1)));
    }

    #[test]
    fn test_shadowing() {
        let mut env = Environment::new();
        env.define("x", Value::Int(1), false);
        env.push_scope();
        env.define("x", Value::Int(2), false);
        assert_eq!(env.get("x"), Some(Value::Int(2)));
        env.pop_scope();
        assert_eq!(env.get("x"), Some(Value::Int(1)));
    }

    #[test]
    fn test_mutable_set() {
        let mut env = Environment::new();
        env.define("x", Value::Int(1), true);
        assert!(env.set("x", Value::Int(2)).is_ok());
        assert_eq!(env.get("x"), Some(Value::Int(2)));
    }

    #[test]
    fn test_immutable_set_fails() {
        let mut env = Environment::new();
        env.define("x", Value::Int(1), false);
        assert!(env.set("x", Value::Int(2)).is_err());
    }

    #[test]
    fn test_set_undefined_fails() {
        let mut env = Environment::new();
        assert!(env.set("x", Value::Int(1)).is_err());
    }

    #[test]
    fn test_contains() {
        let mut env = Environment::new();
        env.define("x", Value::Int(1), false);
        assert!(env.contains("x"));
        assert!(!env.contains("y"));
    }

    #[test]
    fn test_shared_scope_mutation() {
        // Cloning an environment shares scopes via Rc.
        // Mutations through one handle are visible through the other.
        let mut env = Environment::new();
        env.define("x", Value::Int(0), true);

        let mut clone = env.clone(); // shares the same scope Rc

        clone.set("x", Value::Int(42)).unwrap();
        assert_eq!(env.get("x"), Some(Value::Int(42)));
    }
}

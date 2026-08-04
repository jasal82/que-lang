//! Expression evaluation for the Que interpreter.

use super::Interpreter;
use super::helpers::{duration_to_ms, cmp_semver};
use crate::ast::*;
use crate::error::*;
use crate::ast::AstStringPart;
use crate::token::DurationUnit;
use crate::value::{CmdModifiers, CmdPart, Value};

use std::collections::BTreeMap;

impl Interpreter {
    // ── Expression evaluation ────────────────────────────────────────

    pub(crate) fn eval_expr(&mut self, expr: &Expr) -> IResult {
        match expr {
            // ── Literals ──
            Expr::IntLit(n) => Ok(Value::Int(*n)),
            Expr::FloatLit(n) => Ok(Value::Float(*n)),
            Expr::StringLit(s) => Ok(Value::String(s.clone())),
            Expr::InterpolatedString(parts) => self.interpolate_string(parts),
            Expr::BoolLit(b) => Ok(Value::Bool(*b)),
            Expr::NullLit => Ok(Value::Null),
            Expr::ListLit(elems) => {
                let mut items = Vec::new();
                for e in elems {
                    if let Expr::Spread(inner) = e {
                        if let Value::List(list) = self.eval_expr(inner)? {
                            items.extend(list);
                        } else {
                            return Err(Signal::Error(QueError::new(
                                ErrorKind::TypeMismatch,
                                "spread operator requires a list",
                            )));
                        }
                    } else {
                        items.push(self.eval_expr(e)?);
                    }
                }
                Ok(Value::List(items))
            }
            Expr::MapLit(entries) => {
                let mut map = BTreeMap::new();
                for entry in entries {
                    match entry {
                        MapEntry::Pair(k, v) => {
                            let key = match self.eval_expr(k)? {
                                Value::String(s) => s,
                                other => other.display_string(),
                            };
                            let val = self.eval_expr(v)?;
                            map.insert(key, val);
                        }
                        MapEntry::Spread(expr) => {
                            let val = self.eval_expr(expr)?;
                            if let Value::Map(other) = val {
                                for (k, v) in other {
                                    map.insert(k, v);
                                }
                            } else {
                                return Err(Signal::Error(QueError::new(
                                    ErrorKind::TypeMismatch,
                                    "spread in map requires a map value",
                                )));
                            }
                        }
                    }
                }
                Ok(Value::Map(map))
            }
            Expr::SetLit(elems) => {
                let mut items = Vec::new();
                for e in elems {
                    let val = self.eval_expr(e)?;
                    if !self.set_contains(&items, &val)? {
                        items.push(val);
                    }
                }
                Ok(Value::Set(items))
            }
            Expr::TupleLit(elems) => {
                let items: Result<Vec<_>, _> =
                    elems.iter().map(|e| self.eval_expr(e)).collect();
                Ok(Value::Tuple(items?))
            }
            Expr::CmdLit(parts) => {
                // Commands are lazy — produce a Cmd value, don't execute yet.
                // Evaluate interpolations now, storing the resolved string parts.
                let mut cmd_parts = Vec::new();
                for part in parts {
                    match part {
                        AstStringPart::Literal(s) => cmd_parts.push(CmdPart::Literal(s.clone())),
                        AstStringPart::Expr(expr) => {
                            let val = self.eval_expr(expr)?;
                            match val {
                                // The shell needs the real token; every
                                // human-facing rendering of this command
                                // will show `<redacted>` instead.
                                Value::Secret(s) => cmd_parts.push(CmdPart::Secret(s)),
                                other => cmd_parts.push(CmdPart::Interpolated(other.display_string())),
                            }
                        }
                        AstStringPart::RawExpr(expr) => {
                            let val = self.eval_expr(expr)?;
                            match val {
                                // A secret is a single opaque token, never a
                                // fragment of shell syntax, so `!{}` still
                                // quotes it rather than splicing it raw.
                                Value::Secret(s) => cmd_parts.push(CmdPart::Secret(s)),
                                other => cmd_parts.push(CmdPart::Raw(other.display_string())),
                            }
                        }
                    }
                }
                Ok(Value::Cmd(cmd_parts, Box::new(CmdModifiers::default())))
            }
            Expr::DurationLit(val, unit) => Ok(Value::Duration(*val, *unit)),
            Expr::RegexLit(r) => Ok(Value::Regex(r.clone())),
            Expr::SemverLit(v) => Ok(Value::Semver(v.clone())),
            Expr::PathLit(parts) => self.eval_path_lit(parts),
            Expr::GlobLit(parts) => self.eval_glob_lit(parts),

            // ── Variable ──
            Expr::Ident(name) => {
                let value = self.env.get(name).ok_or_else(|| {
                    Signal::Error(QueError::new(
                        ErrorKind::UndefinedVariable,
                        format!("undefined variable '{}'", name),
                    ))
                })?;

                // `Some`/`None` are kept registered only to produce a migration
                // error; a bare `None` expression must fail too.
                if matches!(name.as_str(), "Some" | "None")
                    && matches!(value, Value::BuiltinFn(ref b) if b == name)
                {
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::Runtime,
                        "`Option`/`Some`/`None` were removed; use `null`, `??` and `?.` instead",
                    )));
                }

                Ok(value)
            }

            // ── Binary / Unary ──
            Expr::BinaryOp { left, op, right } => {
                let l = self.eval_expr(left)?;
                // Short-circuit for logical operators.
                match op {
                    BinOp::And => {
                        return if !l.is_truthy() {
                            Ok(l)
                        } else {
                            self.eval_expr(right)
                        };
                    }
                    BinOp::Or => {
                        return if l.is_truthy() {
                            Ok(l)
                        } else {
                            self.eval_expr(right)
                        };
                    }
                    _ => {}
                }
                let r = self.eval_expr(right)?;
                self.eval_binary_op(*op, &l, &r)
            }
            Expr::UnaryOp { op, expr } => {
                let val = self.eval_expr(expr)?;
                self.eval_unary_op(*op, &val)
            }

            // ── Calls ──
            Expr::Call { callee, args } => {
                // Check for partial application: if any arg is `_`, create a closure
                let has_placeholder = args.iter().any(|a| matches!(&a.value, Expr::Ident(n) if n == "_"));
                if has_placeholder {
                    let callee_val = self.eval_expr(callee)?;
                    // Build a partially applied function:
                    // Evaluate non-placeholder args now, create params for placeholders
                    let mut captured_args: Vec<Option<Value>> = Vec::new();
                    let mut param_names = Vec::new();
                    let mut param_idx = 0;
                    for arg in args {
                        if matches!(&arg.value, Expr::Ident(n) if n == "_") {
                            let pname = format!("__partial_{}", param_idx);
                            param_names.push(pname);
                            captured_args.push(None);
                            param_idx += 1;
                        } else {
                            let val = self.eval_expr(&arg.value)?;
                            captured_args.push(Some(val));
                        }
                    }
                    // Create a closure that fills in the placeholders
                    let params: Vec<Param> = param_names
                        .iter()
                        .map(|name| Param {
                            name: name.clone(),
                            type_ann: None,
                            default: None,
                        })
                        .collect();
                    // Build call expression for the body
                    let mut body_args = Vec::new();
                    let mut pi = 0;
                    for cap in &captured_args {
                        match cap {
                            Some(_val) => {
                                // Embed captured value as a string/int literal (simplified)
                                // Store captured values in the closure environment instead
                                let cap_name = format!("__cap_{}", body_args.len());
                                body_args.push(CallArg {
                                    name: None,
                                    value: Expr::Ident(cap_name),
                                });
                            }
                            None => {
                                body_args.push(CallArg {
                                    name: None,
                                    value: Expr::Ident(param_names[pi].clone()),
                                });
                                pi += 1;
                            }
                        }
                    }
                    // Store captured values and callee in closure environment
                    let mut closure_env = self.env.clone();
                    closure_env.push_scope();
                    closure_env.define("__partial_callee__", callee_val, false);
                    let mut cap_idx = 0;
                    for cap in &captured_args {
                        if let Some(val) = cap {
                            let cap_name = format!("__cap_{}", cap_idx);
                            closure_env.define(&cap_name, val.clone(), false);
                        }
                        cap_idx += 1;
                    }
                    let body_expr = Expr::Call {
                        callee: Box::new(Expr::Ident("__partial_callee__".to_string())),
                        args: body_args,
                    };
                    return Ok(Value::Function {
                        name: None,
                        params,
                        return_type: None,
                        body: Block {
                            stmts: vec![],
                            expr: Some(Box::new(body_expr)),
                        },
                        closure_env,
                    });
                }

                let callee_val = self.eval_expr(callee)?;
                // `assert` is the one builtin that wants its argument unevaluated:
                // a bare `false` says nothing, the expression that produced it says
                // everything. Dispatching on the resolved value rather than on the
                // syntax means a shadowed or aliased `assert` still behaves right.
                if matches!(&callee_val, Value::BuiltinFn(name) if name == "assert") {
                    return self.eval_assert(args);
                }
                // Collect evaluated args with their optional names
                let mut named_args: Vec<(Option<String>, Value)> = Vec::new();
                for arg in args {
                    let val = self.eval_expr(&arg.value)?;
                    named_args.push((arg.name.clone(), val));
                }
                self.call_value_named(callee_val, named_args)
            }
            Expr::MethodCall {
                object,
                method,
                args,
            } => {
                let obj = self.eval_expr(object)?;
                // Whatever an earlier call parked here is not ours; clearing
                // first means a stale value can never land on this receiver.
                self.pending_self_writeback = None;
                let result = self.dispatch_method(obj, method, args);
                let written = self.apply_self_writeback(object, method);
                // A failed call still wrote back what it changed, but its own
                // error is the one worth reporting.
                result.and_then(|v| written.map(|_| v))
            }
            Expr::FieldAccess { object, field } => {
                let obj = self.eval_expr(object)?;
                self.access_field(&obj, field)
            }
            Expr::OptionalAccess {
                object,
                field,
                args,
            } => {
                let obj = self.eval_expr(object)?;
                if matches!(obj, Value::Null) {
                    return Ok(Value::Null);
                }
                // `?.` is `?` and then `.`: the lexer takes the two characters
                // together, so this is the only place `res?.field` can mean the
                // field of the value rather than of the `Ok` wrapping it.
                let obj = try_unwrap(obj)?;
                match args {
                    Some(args) => self.dispatch_method(obj, field, args),
                    None => self.access_field(&obj, field),
                }
            }

            Expr::StructLit { name, fields } => {
                self.eval_struct_lit(name, fields)
            }
            Expr::Index { object, index } => {
                let obj = self.eval_expr(object)?;
                let idx = self.eval_expr(index)?;
                self.index_into(&obj, &idx)
            }

            // ── Lambda ──
            Expr::Lambda { params, body } => Ok(Value::Function {
                name: None,
                params: params.clone(),
                return_type: None,
                body: Block {
                    stmts: vec![],
                    expr: Some(body.clone()),
                },
                closure_env: self.env.clone(),
            }),

            // ── Control flow expressions ──
            Expr::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond = self.eval_expr(condition)?;
                if cond.is_truthy() {
                    self.eval_block_scoped(then_branch)
                } else if let Some(else_expr) = else_branch {
                    self.eval_expr(else_expr)
                } else {
                    Ok(Value::Null)
                }
            }
            Expr::IfLet {
                pattern,
                value,
                then_branch,
                else_branch,
            } => {
                let val = self.eval_expr(value)?;
                if let Some(bindings) = self.match_pattern(pattern, &val) {
                    self.env.push_scope();
                    for (name, v) in bindings {
                        self.env.define(&name, v, false);
                    }
                    let result = self.eval_block(then_branch);
                    self.env.pop_scope();
                    result
                } else if let Some(else_expr) = else_branch {
                    self.eval_expr(else_expr)
                } else {
                    Ok(Value::Null)
                }
            }
            Expr::Match { subject, arms } => {
                let val = self.eval_expr(subject)?;
                for arm in arms {
                    if let Some(bindings) = self.match_pattern(&arm.pattern, &val) {
                        self.env.push_scope();
                        for (name, v) in bindings {
                            self.env.define(&name, v, false);
                        }
                        // Check guard.
                        if let Some(guard) = &arm.guard {
                            let guard_val = self.eval_expr(guard)?;
                            if !guard_val.is_truthy() {
                                self.env.pop_scope();
                                continue;
                            }
                        }
                        let result = self.eval_expr(&arm.body);
                        self.env.pop_scope();
                        return result;
                    }
                }
                Err(Signal::Error(QueError::new(
                    ErrorKind::Runtime,
                    "non-exhaustive match",
                )))
            }
            Expr::Block(block) => self.eval_block_scoped(block),

            // ── With expressions ──
            Expr::WithContext { manager, name, body } => {
                self.eval_with_context(manager, name, body)
            }

            // ── Loop expression (returns value via break) ──
            Expr::Loop { body } => {
                let result = loop {
                    match self.eval_block(body) {
                        Ok(_) => {}
                        Err(Signal::Break(val)) => break val.unwrap_or(Value::Null),
                        Err(Signal::Continue) => continue,
                        Err(signal) => return Err(signal),
                    }
                };
                Ok(result)
            }

            // ── Pipe ──
            Expr::Pipe { left, right } => {
                let left_val = self.eval_expr(left)?;
                self.eval_pipe(left_val, right)
            }

            // ── Try (?) ──
            Expr::Try(inner) => {
                let val = self.eval_expr(inner)?;
                try_unwrap(val)
            }

            // ── Null coalesce ──
            Expr::NullCoalesce { left, right } => {
                let val = self.eval_expr(left)?;
                match val {
                    Value::Null => self.eval_expr(right),
                    other => Ok(other),
                }
            }

            // ── Range ──
            Expr::Range {
                start,
                end,
                inclusive,
            } => {
                let s = start
                    .as_ref()
                    .map(|e| self.eval_expr(e))
                    .transpose()?
                    .unwrap_or(Value::Int(0));
                let e = end.as_ref().map(|e| self.eval_expr(e)).transpose()?;

                match (&s, &e) {
                    (Value::Int(a), Some(Value::Int(b))) => {
                        let items: Vec<Value> = if *inclusive {
                            (*a..=*b).map(Value::Int).collect()
                        } else {
                            (*a..*b).map(Value::Int).collect()
                        };
                        Ok(Value::List(items))
                    }
                    (Value::Int(_), None) => {
                        // Unbounded ranges are not materialised.
                        Err(Signal::Error(QueError::new(
                            ErrorKind::Runtime,
                            "unbounded range cannot be materialised",
                        )))
                    }
                    _ => Err(Signal::Error(QueError::new(
                        ErrorKind::TypeMismatch,
                        "range requires integer bounds",
                    ))),
                }
            }

            // ── Spread (only valid in specific contexts — handled above) ──
            Expr::Spread(_) => Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                "spread operator used outside of valid context",
            ))),

            // ── spawn expr — launch a command in the background ──
            Expr::Spawn(inner) => {
                let val = self.eval_expr(inner)?;
                self.eval_spawn(val)
            }

            // ── parallel { branches } — evaluate branches (concurrently when safe) ──
            Expr::Parallel(branches) => {
                self.eval_parallel(branches)
            }
        }
    }


    // ── Pipe operator ────────────────────────────────────────────────

    fn eval_pipe(&mut self, left: Value, right: &Expr) -> IResult {
        match right {
            Expr::Call { callee, args } => {
                let callee_val = self.eval_expr(callee)?;
                let mut named_args: Vec<(Option<String>, Value)> = vec![(None, left)];
                for arg in args {
                    let val = self.eval_expr(&arg.value)?;
                    named_args.push((arg.name.clone(), val));
                }
                self.call_value_named(callee_val, named_args)
            }
            Expr::Lambda { params, body } => {
                let func = Value::Function {
                    name: None,
                    params: params.clone(),
                    return_type: None,
                    body: Block {
                        stmts: vec![],
                        expr: Some(body.clone()),
                    },
                    closure_env: self.env.clone(),
                };
                self.call_value(func, vec![left])
            }
            _ => {
                // Treat right side as a function expression.
                let func = self.eval_expr(right)?;
                self.call_value(func, vec![left])
            }
        }
    }

    // ── Assertions ───────────────────────────────────────────────────

    /// Evaluate `assert(condition, message?)`.
    ///
    /// The condition arrives as an expression rather than a `Bool` so that a
    /// failure can report the code and the values it produced side by side:
    ///
    /// ```text
    /// assertion failed: len(users) >= min_count  (2 >= 5)
    /// ```
    ///
    /// This is why there is no `assert_eq`, `assert_lt` and so on: the family
    /// existed only to recover operand values that a collapsed `Bool` had
    /// already thrown away.
    fn eval_assert(&mut self, args: &[CallArg]) -> IResult {
        let condition = match args.first() {
            Some(arg) => &arg.value,
            None => {
                return Err(Signal::Error(QueError::new(
                    ErrorKind::ArityMismatch,
                    "assert requires a condition",
                )))
            }
        };

        let explained = self.explain_assert(condition)?;
        let Some(detail) = explained else {
            return Ok(Value::Null);
        };

        // An explicit message replaces the generic headline but keeps the
        // detail — the whole point is not having to restate the values.
        let headline = match args.get(1) {
            Some(arg) => self.eval_expr(&arg.value)?.display_string(),
            None => "assertion failed".to_string(),
        };
        Err(Signal::Error(QueError::runtime(format!(
            "{}: {}",
            headline, detail
        ))))
    }

    /// Evaluate `expr` for truthiness, returning `None` when it holds and a
    /// rendered explanation when it does not.
    ///
    /// Every sub-expression is evaluated at most once, and `&&` / `||` keep
    /// their short-circuit order, so an assertion costs exactly what the same
    /// condition would cost anywhere else.
    fn explain_assert(&mut self, expr: &Expr) -> Result<Option<String>, Signal> {
        let source = crate::formatter::Formatter::expr_to_source(expr);
        match expr {
            Expr::BinaryOp { left, op, right }
                if matches!(
                    op,
                    BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq
                ) =>
            {
                let l = self.eval_expr(left)?;
                let r = self.eval_expr(right)?;
                if self.eval_binary_op(*op, &l, &r)?.is_truthy() {
                    return Ok(None);
                }
                let values = format!(
                    "{} {} {}",
                    l.debug_string(),
                    crate::formatter::bin_op_str(*op),
                    r.debug_string()
                );
                Ok(Some(annotate(&source, &values)))
            }
            // Report the operand that decided the result, not the whole chain.
            Expr::BinaryOp { left, op: BinOp::And, right } => {
                if let Some(detail) = self.explain_assert(left)? {
                    return Ok(Some(detail));
                }
                self.explain_assert(right)
            }
            Expr::BinaryOp { left, op: BinOp::Or, right } => {
                let l = self.eval_expr(left)?;
                if l.is_truthy() {
                    return Ok(None);
                }
                match self.explain_assert(right)? {
                    None => Ok(None),
                    Some(_) => Ok(Some(format!("{}  (both sides false)", source))),
                }
            }
            Expr::UnaryOp { op: UnaryOp::Not, expr: inner } => {
                let val = self.eval_expr(inner)?;
                if !val.is_truthy() {
                    return Ok(None);
                }
                Ok(Some(annotate(&source, &val.debug_string())))
            }
            _ => {
                let val = self.eval_expr(expr)?;
                if val.is_truthy() {
                    return Ok(None);
                }
                Ok(Some(annotate(&source, &val.debug_string())))
            }
        }
    }

    // ── Binary operators ─────────────────────────────────────────────

    pub(crate) fn eval_binary_op(&mut self, op: BinOp, left: &Value, right: &Value) -> IResult {
        match op {
            BinOp::Add => self.eval_add(left, right),
            BinOp::Sub => {
                // Set - Set → difference
                if let (Value::Set(a), Value::Set(b)) = (left, right) {
                    let result: Vec<Value> = a.iter()
                        .filter(|item| !b.contains(item))
                        .cloned()
                        .collect();
                    return Ok(Value::Set(result));
                }
                // Int - Duration → Int (timestamp arithmetic: now() - 7d)
                if let (Value::Int(a), Value::Duration(b, bu)) = (left, right) {
                    let b_ms = duration_to_ms(*b, *bu) as i64;
                    return Ok(Value::Int(a - b_ms));
                }
                self.eval_arith(left, right, "subtract", |a, b| a - b, |a, b| a - b)
            }
            BinOp::Mul => self.eval_arith(left, right, "multiply", |a, b| a * b, |a, b| a * b),
            BinOp::Div => self.eval_div(left, right),
            BinOp::Mod => self.eval_mod(left, right),
            BinOp::Pow => self.eval_pow(left, right),

            BinOp::Eq => {
                if let (Value::Instance { type_name: ta, .. }, Value::Instance { type_name: tb, .. }) = (left, right) {
                    if ta == tb {
                        let ta = ta.clone();
                        if let Some(m) = self.find_instance_method(&ta, "equals") {
                            return self.call_method_def(m, Some(left.clone()), vec![right.clone()]);
                        }
                    }
                }
                Ok(Value::Bool(left == right))
            }
            BinOp::NotEq => {
                if let (Value::Instance { type_name: ta, .. }, Value::Instance { type_name: tb, .. }) = (left, right) {
                    if ta == tb {
                        let ta = ta.clone();
                        if let Some(m) = self.find_instance_method(&ta, "equals") {
                            let result = self.call_method_def(m, Some(left.clone()), vec![right.clone()])?;
                            return Ok(Value::Bool(!result.is_truthy()));
                        }
                    }
                }
                Ok(Value::Bool(left != right))
            }
            BinOp::Lt => {
                if let Some(ord) = self.try_instance_compare(left, right)? {
                    return Ok(Value::Bool(ord.is_lt()));
                }
                self.eval_cmp(left, right, |o| o.is_lt())
            }
            BinOp::Gt => {
                if let Some(ord) = self.try_instance_compare(left, right)? {
                    return Ok(Value::Bool(ord.is_gt()));
                }
                self.eval_cmp(left, right, |o| o.is_gt())
            }
            BinOp::LtEq => {
                if let Some(ord) = self.try_instance_compare(left, right)? {
                    return Ok(Value::Bool(!ord.is_gt()));
                }
                self.eval_cmp(left, right, |o| !o.is_gt())
            }
            BinOp::GtEq => {
                if let Some(ord) = self.try_instance_compare(left, right)? {
                    return Ok(Value::Bool(!ord.is_lt()));
                }
                self.eval_cmp(left, right, |o| !o.is_lt())
            }

            // And/Or are handled by short-circuit in eval_expr.
            BinOp::And | BinOp::Or => unreachable!(),

            BinOp::BitAnd => {
                // Set & Set → intersection
                if let (Value::Set(a), Value::Set(b)) = (left, right) {
                    let result: Vec<Value> = a.iter()
                        .filter(|item| b.contains(item))
                        .cloned()
                        .collect();
                    return Ok(Value::Set(result));
                }
                self.eval_bitop(left, right, "bitand", |a, b| a & b)
            }
            BinOp::BitOr => {
                // Set | Set → union
                if let (Value::Set(a), Value::Set(b)) = (left, right) {
                    let mut result = a.clone();
                    for val in b {
                        if !result.contains(val) {
                            result.push(val.clone());
                        }
                    }
                    return Ok(Value::Set(result));
                }
                // Cmd | Cmd → a shell pipeline. `|` needs no new token here
                // because the operand types already decide the meaning, the
                // same way `/` is division on numbers and join on paths.
                if let Value::Cmd(left_parts, left_mods) = left {
                    let Value::Cmd(right_parts, right_mods) = right else {
                        return Err(Signal::Error(QueError::new(
                            ErrorKind::TypeMismatch,
                            format!(
                                "the right side of a command pipe must be a command, got {}",
                                right.type_name()
                            ),
                        )));
                    };
                    let mut mods = right_mods.as_ref().clone();
                    // An attached stage owns the terminal, which is exactly
                    // what a pipeline needs to take over on both ends.
                    if mods.attach || left_mods.attach {
                        return Err(Signal::Error(QueError::new(
                            ErrorKind::Runtime,
                            "`.attach()` cannot be used in a pipeline: the stages need each other's streams",
                        )));
                    }
                    // Flatten: `a | b | c` parses as `(a | b) | c`, so the
                    // left side may already carry upstream stages.
                    let mut upstream = left_mods.stdin_from.clone();
                    let mut head = left_mods.as_ref().clone();
                    head.stdin_from = Vec::new();
                    upstream.push(crate::value::CmdStage {
                        parts: left_parts.clone(),
                        mods: head,
                    });
                    upstream.extend(mods.stdin_from);
                    mods.stdin_from = upstream;
                    return Ok(Value::Cmd(right_parts.clone(), Box::new(mods)));
                }
                self.eval_bitop(left, right, "bitor", |a, b| a | b)
            }
            BinOp::BitXor => {
                // Set ^ Set → symmetric difference
                if let (Value::Set(a), Value::Set(b)) = (left, right) {
                    let mut result: Vec<Value> = a.iter()
                        .filter(|item| !b.contains(item))
                        .cloned()
                        .collect();
                    for val in b {
                        if !a.contains(val) && !result.contains(val) {
                            result.push(val.clone());
                        }
                    }
                    return Ok(Value::Set(result));
                }
                self.eval_bitop(left, right, "bitxor", |a, b| a ^ b)
            }
            BinOp::Shl => self.eval_bitop(left, right, "shl", |a, b| a << b),
            BinOp::Shr => self.eval_bitop(left, right, "shr", |a, b| a >> b),
        }
    }

    fn eval_add(&self, left: &Value, right: &Value) -> IResult {
        match (left, right) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 + b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + *b as f64)),
            (Value::String(a), Value::String(b)) => {
                Ok(Value::String(format!("{}{}", a, b)))
            }
            (Value::String(a), b) => Ok(Value::String(format!("{}{}", a, b))),
            (Value::List(a), Value::List(b)) => {
                let mut result = a.clone();
                result.extend(b.iter().cloned());
                Ok(Value::List(result))
            }
            // Set + Set → union
            (Value::Set(a), Value::Set(b)) => {
                let mut result = a.clone();
                for val in b {
                    if !result.contains(val) {
                        result.push(val.clone());
                    }
                }
                Ok(Value::Set(result))
            }
            (Value::Duration(a, au), Value::Duration(b, bu)) => {
                let a_ms = duration_to_ms(*a, *au);
                let b_ms = duration_to_ms(*b, *bu);
                Ok(Value::Duration(a_ms + b_ms, DurationUnit::Milliseconds))
            }
            // Int + Duration → Int (timestamp arithmetic: now() + 24h)
            (Value::Int(a), Value::Duration(b, bu)) => {
                let b_ms = duration_to_ms(*b, *bu) as i64;
                Ok(Value::Int(a + b_ms))
            }
            (Value::Duration(a, au), Value::Int(b)) => {
                let a_ms = duration_to_ms(*a, *au) as i64;
                Ok(Value::Int(a_ms + b))
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::TypeMismatch,
                format!(
                    "cannot add {} and {}",
                    left.type_name(),
                    right.type_name()
                ),
            ))),
        }
    }

    fn eval_arith(
        &self,
        left: &Value,
        right: &Value,
        name: &str,
        int_op: fn(i64, i64) -> i64,
        float_op: fn(f64, f64) -> f64,
    ) -> IResult {
        match (left, right) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(int_op(*a, *b))),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(float_op(*a, *b))),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(float_op(*a as f64, *b))),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(float_op(*a, *b as f64))),
            // Duration - Duration
            (Value::Duration(a, au), Value::Duration(b, bu)) => {
                let a_ms = duration_to_ms(*a, *au);
                let b_ms = duration_to_ms(*b, *bu);
                Ok(Value::Duration(float_op(a_ms, b_ms), DurationUnit::Milliseconds))
            }
            // Duration * number / number * Duration
            (Value::Duration(a, au), Value::Int(b)) => {
                let a_ms = duration_to_ms(*a, *au);
                Ok(Value::Duration(float_op(a_ms, *b as f64), DurationUnit::Milliseconds))
            }
            (Value::Duration(a, au), Value::Float(b)) => {
                let a_ms = duration_to_ms(*a, *au);
                Ok(Value::Duration(float_op(a_ms, *b), DurationUnit::Milliseconds))
            }
            (Value::Int(a), Value::Duration(b, bu)) => {
                let b_ms = duration_to_ms(*b, *bu);
                Ok(Value::Duration(float_op(*a as f64, b_ms), DurationUnit::Milliseconds))
            }
            (Value::Float(a), Value::Duration(b, bu)) => {
                let b_ms = duration_to_ms(*b, *bu);
                Ok(Value::Duration(float_op(*a, b_ms), DurationUnit::Milliseconds))
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::TypeMismatch,
                format!(
                    "cannot {} {} and {}",
                    name,
                    left.type_name(),
                    right.type_name()
                ),
            ))),
        }
    }

    fn eval_div(&self, left: &Value, right: &Value) -> IResult {
        match (left, right) {
            (Value::Int(a), Value::Int(b)) => {
                if *b == 0 {
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::DivisionByZero,
                        "division by zero",
                    )));
                }
                Ok(Value::Int(a / b))
            }
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a / b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 / b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a / *b as f64)),
            // Path / Path: absolute right side wins (via Rust's PathBuf::join)
            (Value::Path(p), Value::Path(s)) => {
                let joined = std::path::PathBuf::from(p)
                    .join(s)
                    .to_string_lossy()
                    .to_string();
                Ok(Value::Path(joined))
            }
            // Path / String: string is NEVER absolute — strip leading slashes
            (Value::Path(p), Value::String(s)) => {
                let seg = s.trim_start_matches('/');
                let joined = std::path::PathBuf::from(p)
                    .join(seg)
                    .to_string_lossy()
                    .to_string();
                Ok(Value::Path(joined))
            }
            // Path / Glob is a type error — use Glob(path) / "pattern" instead
            (Value::Path(_), Value::Glob(_)) => Err(Signal::Error(QueError::new(
                ErrorKind::TypeMismatch,
                "cannot compose Path with Glob using /; convert to Glob first: Glob(path) / pattern",
            ))),
            // Glob / String: string is NEVER absolute
            (Value::Glob(g), Value::String(s)) => {
                let seg = s.trim_start_matches('/');
                let joined = if g.is_empty() {
                    seg.to_string()
                } else if g.ends_with('/') {
                    format!("{}{}", g, seg)
                } else {
                    format!("{}/{}", g, seg)
                };
                Ok(Value::Glob(joined))
            }
            // Glob / Path: absolute path wins
            (Value::Glob(g), Value::Path(s)) => {
                let joined = std::path::PathBuf::from(g)
                    .join(s)
                    .to_string_lossy()
                    .to_string();
                Ok(Value::Glob(joined))
            }
            // Glob / Glob: absolute right side wins
            (Value::Glob(g), Value::Glob(h)) => {
                let joined = std::path::PathBuf::from(g)
                    .join(h)
                    .to_string_lossy()
                    .to_string();
                Ok(Value::Glob(joined))
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::TypeMismatch,
                format!(
                    "cannot divide {} by {}",
                    left.type_name(),
                    right.type_name()
                ),
            ))),
        }
    }

    fn eval_mod(&self, left: &Value, right: &Value) -> IResult {
        match (left, right) {
            (Value::Int(a), Value::Int(b)) => {
                if *b == 0 {
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::DivisionByZero,
                        "modulo by zero",
                    )));
                }
                Ok(Value::Int(a % b))
            }
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a % b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(*a as f64 % b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a % *b as f64)),
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::TypeMismatch,
                format!(
                    "cannot modulo {} and {}",
                    left.type_name(),
                    right.type_name()
                ),
            ))),
        }
    }

    fn eval_pow(&self, left: &Value, right: &Value) -> IResult {
        match (left, right) {
            (Value::Int(a), Value::Int(b)) => {
                if *b >= 0 {
                    Ok(Value::Int(a.pow(*b as u32)))
                } else {
                    Ok(Value::Float((*a as f64).powi(*b as i32)))
                }
            }
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a.powf(*b))),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float((*a as f64).powf(*b))),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a.powi(*b as i32))),
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::TypeMismatch,
                format!(
                    "cannot exponentiate {} by {}",
                    left.type_name(),
                    right.type_name()
                ),
            ))),
        }
    }

    pub(crate) fn eval_cmp(
        &self,
        left: &Value,
        right: &Value,
        pred: fn(std::cmp::Ordering) -> bool,
    ) -> IResult {
        let ord = match (left, right) {
            (Value::Int(a), Value::Int(b)) => a.cmp(b),
            (Value::Float(a), Value::Float(b)) => a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal),
            (Value::Int(a), Value::Float(b)) => (*a as f64).partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal),
            (Value::Float(a), Value::Int(b)) => a.partial_cmp(&(*b as f64)).unwrap_or(std::cmp::Ordering::Equal),
            (Value::String(a), Value::String(b)) => a.cmp(b),
            (Value::Semver(a), Value::Semver(b)) => cmp_semver(a, b),
            (Value::Duration(a, au), Value::Duration(b, bu)) => {
                let a_ms = duration_to_ms(*a, *au);
                let b_ms = duration_to_ms(*b, *bu);
                a_ms.partial_cmp(&b_ms).unwrap_or(std::cmp::Ordering::Equal)
            }
            (Value::Instance { type_name: ta, fields: fa },
             Value::Instance { type_name: tb, fields: fb })
                if ta == "DateTime" && tb == "DateTime" =>
            {
                let a_ms = match fa.get("_timestamp_ms") {
                    Some(Value::Int(ms)) => *ms,
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::Runtime,
                        "DateTime missing _timestamp_ms".to_string(),
                    ))),
                };
                let b_ms = match fb.get("_timestamp_ms") {
                    Some(Value::Int(ms)) => *ms,
                    _ => return Err(Signal::Error(QueError::new(
                        ErrorKind::Runtime,
                        "DateTime missing _timestamp_ms".to_string(),
                    ))),
                };
                a_ms.cmp(&b_ms)
            }
            _ => {
                return Err(Signal::Error(QueError::new(
                    ErrorKind::TypeMismatch,
                    format!(
                        "cannot compare {} and {}",
                        left.type_name(),
                        right.type_name()
                    ),
                )))
            }
        };
        Ok(Value::Bool(pred(ord)))
    }

    fn eval_bitop(
        &self,
        left: &Value,
        right: &Value,
        name: &str,
        op: fn(i64, i64) -> i64,
    ) -> IResult {
        match (left, right) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(op(*a, *b))),
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::TypeMismatch,
                format!(
                    "cannot {} on {} and {}",
                    name,
                    left.type_name(),
                    right.type_name()
                ),
            ))),
        }
    }

    // ── Unary operators ──────────────────────────────────────────────

    fn eval_unary_op(&self, op: UnaryOp, val: &Value) -> IResult {
        match op {
            UnaryOp::Neg => match val {
                Value::Int(n) => Ok(Value::Int(-n)),
                Value::Float(n) => Ok(Value::Float(-n)),
                _ => Err(Signal::Error(QueError::new(
                    ErrorKind::TypeMismatch,
                    format!("cannot negate {}", val.type_name()),
                ))),
            },
            UnaryOp::Not => Ok(Value::Bool(!val.is_truthy())),
            UnaryOp::BitNot => match val {
                Value::Int(n) => Ok(Value::Int(!n)),
                _ => Err(Signal::Error(QueError::new(
                    ErrorKind::TypeMismatch,
                    format!("cannot bitwise-not {}", val.type_name()),
                ))),
            },
        }
    }


    // ── Function / method calling ────────────────────────────────────

    /// Call a zero-argument function from the `que test` runner.
    ///
    /// `Signal::Return` is normalised to the returned value: a test that ends
    /// with an early `return` passed just as surely as one that fell off the
    /// end, and the runner should not have to know the difference.
    pub fn call_test(&mut self, callee: Value) -> Result<Value, Signal> {
        match self.call_value(callee, Vec::new()) {
            Err(Signal::Return(v)) => Ok(v),
            other => other,
        }
    }

    pub(crate) fn call_value(&mut self, callee: Value, args: Vec<Value>) -> IResult {
        let named_args: Vec<(Option<String>, Value)> = args.into_iter().map(|v| (None, v)).collect();
        self.call_value_named(callee, named_args)
    }

    pub(crate) fn call_value_named(&mut self, callee: Value, named_args: Vec<(Option<String>, Value)>) -> IResult {
        match callee {
            Value::Function {
                name,
                params,
                return_type,
                body,
                closure_env,
            } => {
                // Save call-site span so that after the function returns, current_span
                // reflects where the call was made (not a stale line inside the callee).
                let saved_span = self.current_span;
                let saved_file = self.current_file.clone();
                self.call_stack.push(crate::error::CallFrame {
                    name: name.clone().unwrap_or_else(|| "<anonymous>".to_string()),
                    call_file: self.current_file.clone(),
                    call_span: self.current_span,
                });
                let saved = std::mem::replace(&mut self.env, closure_env);
                // For recursion: if function has a name, define it in its own scope
                // so it can call itself. This works because we capture the environment
                // after the function is defined in exec_item.
                if let Some(ref func_name) = name {
                    if self.env.contains(func_name) {
                        // Function is already in scope (defined after us), use it.
                        // This handles mutual recursion as well.
                    } else {
                        // Function not in closure env; check saved env
                        if let Some(func_val) = saved.get(func_name) {
                            self.env.define(func_name, func_val, false);
                        }
                    }
                }
                self.env.push_scope();
                // Resolve named arguments: split into positional and named
                let mut positional: Vec<Value> = Vec::new();
                let mut named: Vec<(String, Value)> = Vec::new();
                for (name_opt, val) in named_args.iter() {
                    match name_opt {
                        Some(n) => named.push((n.clone(), val.clone())),
                        None => positional.push(val.clone()),
                    }
                }
                // Bind parameters: positional first, then named by name, then defaults
                let mut pos_idx = 0;
                let strict = self.strict;
                for param in params.iter() {
                    let val = if let Some(idx) = named.iter().position(|(n, _)| n == &param.name) {
                        // Named argument matches this parameter
                        named.remove(idx).1
                    } else if pos_idx < positional.len() {
                        // Use next positional argument
                        let v = positional[pos_idx].clone();
                        pos_idx += 1;
                        v
                    } else if let Some(default) = &param.default {
                        // Evaluate default in the function's scope.
                        self.eval_expr(default)?
                    } else {
                        self.call_stack.pop();
                        self.current_span = saved_span;
                        self.current_file = saved_file;
                        return Err(Signal::Error(QueError::new(
                            ErrorKind::ArityMismatch,
                            format!(
                                "expected {} args, got {}",
                                params.len(),
                                positional.len() + named.len()
                            ),
                        )));
                    };
                    // Type check parameter if strict mode is on and annotation exists
                    if strict {
                        if let Some(ref ty) = param.type_ann {
                            check_type(&val, ty, &param.name, &name)?;
                        }
                    }
                    self.env.define(&param.name, val, true);
                }
                let result = self.eval_block(&body);
                self.env.pop_scope();
                self.env = saved;
                // Restore call-site span so callers see correct location tracking.
                self.current_span = saved_span;
                self.current_file = saved_file;
                self.call_stack.pop();
                match result {
                    Ok(v) => {
                        // Type check return value if strict mode is on
                        if strict {
                            if let Some(ref ret_ty) = return_type {
                                check_return_type(&v, ret_ty, &name)?;
                            }
                        }
                        Ok(v)
                    }
                    Err(Signal::Return(v)) => {
                        if strict {
                            if let Some(ref ret_ty) = return_type {
                                check_return_type(&v, ret_ty, &name)?;
                            }
                        }
                        Ok(v)
                    }
                    Err(other) => Err(other),
                }
            }
            Value::BuiltinFn(name) => {
                let args: Vec<Value> = named_args.into_iter().map(|(_, v)| v).collect();
                self.call_builtin(&name, args)
            }
            // Handle tasks — calling a task triggers dependency resolution + execution
            Value::Task(t) => {
                self.execute_task(&t, named_args)
            }
            // Handle composed functions (created by compose())
            Value::Tuple(ref items)
                if items.first() == Some(&Value::String("__composed__".to_string())) =>
            {
                let funcs = &items[1..];
                let mut result = named_args.into_iter().next().map(|(_, v)| v).unwrap_or(Value::Null);
                for func in funcs {
                    result = self.call_value(func.clone(), vec![result])?;
                }
                Ok(result)
            }
            // TypeRef called as a function: MyStruct(args) → MyStruct.new(args)
            Value::TypeRef(type_name) => {
                if let Some(m) = self.find_static_method(&type_name, "new") {
                    self.call_method_def(m, None, named_args.into_iter().map(|(_, v)| v).collect())
                } else {
                    Err(Signal::Error(QueError::new(
                        ErrorKind::NotCallable,
                        format!("type `{}` has no `new` constructor", type_name),
                    )))
                }
            }
            other => Err(Signal::Error(QueError::new(
                ErrorKind::NotCallable,
                format!("{} is not callable", other.type_name()),
            ))),
        }
    }


    // ── Field access ─────────────────────────────────────────────────

    /// Dispatch `obj.method(args)` on an already-evaluated receiver.
    ///
    /// Shared by `Expr::MethodCall` and the method form of `Expr::OptionalAccess`.
    fn dispatch_method(&mut self, obj: Value, method: &str, args: &[CallArg]) -> IResult {
            if let Value::TypeRef(type_name) = &obj {
                let type_name = type_name.clone();
                if let Some(variants) = self.enum_defs.get(&type_name).cloned() {
                    if let Some((_, field_names)) = variants.iter().find(|(n, _)| n == method) {
                        let field_names = field_names.clone();
                        let has_names = args.iter().any(|a| a.name.is_some());
                        let mut fields = BTreeMap::new();
                        for (i, arg) in args.iter().enumerate() {
                            let val = self.eval_expr(&arg.value)?;
                            let name = if has_names {
                                arg.name.clone().ok_or_else(|| Signal::Error(QueError::new(
                                    ErrorKind::Runtime,
                                    format!("enum variant constructor '{}' mixes named and positional arguments", method),
                                )))?
                            } else {
                                field_names.get(i).cloned().ok_or_else(|| Signal::Error(QueError::new(
                                    ErrorKind::Runtime,
                                    format!("enum variant '{}' has {} field(s) but {} argument(s) provided", method, field_names.len(), args.len()),
                                )))?
                            };
                            fields.insert(name, val);
                        }
                        return Ok(Value::Enum {
                            enum_name: type_name,
                            variant: method.to_string(),
                            fields,
                        });
                    }
                }
            }

            // Also dispatch instance methods on Enum values: e.method(args)
            if let Value::Enum { enum_name, .. } = &obj {
                let enum_name = enum_name.clone();
                let mut arg_vals = Vec::new();
                for arg in args {
                    arg_vals.push(self.eval_expr(&arg.value)?);
                }
                if let Some(m) = self.find_instance_method(&enum_name, method) {
                    return self.call_method_def(m, Some(obj), arg_vals);
                }
                return self.call_method(&obj, method, arg_vals);
            }

            let mut arg_vals = Vec::new();
            for arg in args {
                arg_vals.push(self.eval_expr(&arg.value)?);
            }
            // Static method dispatch on TypeRef: TypeName.method(args)
            if let Value::TypeRef(type_name) = &obj {
                let type_name = type_name.clone();
                if let Some(m) = self.find_static_method(&type_name, method) {
                    return self.call_method_def(m, None, arg_vals);
                }
                // If not found as a static method, fall through to call_method
            }
            // Instance method dispatch on Instance: instance.method(args)
            if let Value::Instance { type_name, fields } = &obj {
                let type_name = type_name.clone();
                if let Some(m) = self.find_instance_method(&type_name, method) {
                    return self.call_method_def(m, Some(obj), arg_vals);
                }
                // If no impl method found, check if the field itself is callable
                // (e.g. os.exit where exit is a BuiltinFn stored in the struct field)
                if let Some(callable) = fields.get(method).cloned() {
                    if matches!(callable, Value::Function { .. } | Value::BuiltinFn(_)) {
                        return self.call_value(callable, arg_vals);
                    }
                }
                // If not found as user method or callable field, fall through to builtin call_method
            }
            // Module dispatch: modules have no built-in methods, only callable entries
            if let Value::Module { ref entries, ref name } = obj {
                if let Some(callable) = entries.get(method).cloned() {
                    return self.call_value(callable, arg_vals);
                }
                return Err(Signal::Error(QueError::new(
                    ErrorKind::KeyNotFound,
                    format!("'{}' is not a function in module '{}'", method, name),
                )));
            }
            self.call_method(&obj, method, arg_vals)
    }

    fn access_field(&self, obj: &Value, field: &str) -> IResult {
        match obj {
            Value::Map(map) => Ok(map.get(field).cloned().unwrap_or(Value::Null)),
            Value::ProcessResult {
                exit_code,
                stdout,
                stderr,
            } => match field {
                "exit_code" | "code" => Ok(Value::Int(*exit_code)),
                "stdout" => Ok(Value::String(stdout.clone())),
                "stderr" => Ok(Value::String(stderr.clone())),
                _ => Err(Signal::Error(QueError::new(
                    ErrorKind::KeyNotFound,
                    format!("ProcessResult has no field '{}'", field),
                ))),
            },
            Value::Tuple(items) => {
                // Support .0, .1, .2, etc.
                if let Ok(idx) = field.parse::<usize>() {
                    items.get(idx).cloned().ok_or_else(|| {
                        Signal::Error(QueError::new(
                            ErrorKind::IndexOutOfBounds,
                            format!("tuple index {} out of bounds (len {})", idx, items.len()),
                        ))
                    })
                } else {
                    Err(Signal::Error(QueError::new(
                        ErrorKind::KeyNotFound,
                        format!("Tuple has no field '{}'", field),
                    )))
                }
            }
            Value::Semver(s) => {
                let parse_ver = |ver: &str| -> (u64, u64, u64) {
                    let base = ver.split('-').next().unwrap_or(ver);
                    let parts: Vec<&str> = base.split('.').collect();
                    let major = parts.first().and_then(|p| p.parse().ok()).unwrap_or(0);
                    let minor = parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(0);
                    let patch = parts.get(2).and_then(|p| p.parse().ok()).unwrap_or(0);
                    (major, minor, patch)
                };
                let (major, minor, patch) = parse_ver(s);
                match field {
                    "major" => Ok(Value::Int(major as i64)),
                    "minor" => Ok(Value::Int(minor as i64)),
                    "patch" => Ok(Value::Int(patch as i64)),
                    "prerelease" | "pre_release" => {
                        if let Some(idx) = s.find('-') {
                            Ok(Value::String(s[idx + 1..].to_string()))
                        } else {
                            Ok(Value::Null)
                        }
                    }
                    _ => Err(Signal::Error(QueError::new(
                        ErrorKind::KeyNotFound,
                        format!("Semver has no field '{}'", field),
                    ))),
                }
            }
            Value::Task(t) => {
                match field {
                    "name" => Ok(Value::String(t.name.clone())),
                    "description" | "desc" => Ok(t.description.as_ref()
                        .map(|d| Value::String(d.clone()))
                        .unwrap_or(Value::Null)),
                    "deps" | "depends_on" => Ok(Value::List(
                        t.depends_on.iter().map(|d| Value::String(d.clone())).collect(),
                    )),
                    "params" => Ok(Value::List(
                        t.params.iter().map(|p| Value::String(p.name.clone())).collect(),
                    )),
                    "status" => {
                        match self.task_status.get(t.name.as_str()) {
                            Some((status, _)) => Ok(Value::String(status.clone())),
                            None => Ok(Value::String("pending".to_string())),
                        }
                    }
                    "env" | "env_keys" => Ok(Value::List(
                        t.env_keys.iter().map(|k| Value::String(k.clone())).collect(),
                    )),
                    _ => Err(Signal::Error(QueError::new(
                        ErrorKind::KeyNotFound,
                        format!("Task has no field '{}'", field),
                    ))),
                }
            }
            Value::Module { entries, .. } => {
                Ok(entries.get(field).cloned().unwrap_or(Value::Null))
            }
            Value::Instance { fields, .. } => {
                Ok(fields.get(field).cloned().unwrap_or(Value::Null))
            }
            // Enum field access: EnumType.VariantName
            Value::TypeRef(type_name) => {
                if let Some(variants) = self.enum_defs.get(type_name) {
                    if let Some((_, field_names)) = variants.iter().find(|(n, _)| n == field) {
                        if field_names.is_empty() {
                            // Unit variant - return the value directly
                            return Ok(Value::Enum {
                                enum_name: type_name.clone(),
                                variant: field.to_string(),
                                fields: BTreeMap::new(),
                            });
                        }
                        // Data variant — must be constructed with field values.
                        return Err(Signal::Error(QueError::new(
                            ErrorKind::Runtime,
                            format!(
                                "enum variant '{}' has fields; construct with {}.{} {{ field: value, ... }} or {}.{}(field: value, ...)",
                                field, type_name, field, type_name, field
                            ),
                        )));
                    }
                }
                // Fall through: may be a static field on a struct type (not typical but handle gracefully)
                Err(Signal::Error(QueError::new(
                    ErrorKind::KeyNotFound,
                    format!("type '{}' has no variant or field '{}'", type_name, field),
                )))
            }
            // Enum instance field access: enum_val.field_name
            Value::Enum { fields, .. } => {
                Ok(fields.get(field).cloned().unwrap_or(Value::Null))
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::TypeMismatch,
                format!(
                    "cannot access field '{}' on {}",
                    field,
                    obj.type_name()
                ),
            ))),
        }
    }

    /// Call a user-defined MethodDef, optionally passing `self_val` as the first argument.
    pub(crate) fn call_method_def(
        &mut self,
        method: crate::value::MethodDef,
        self_val: Option<Value>,
        args: Vec<Value>,
    ) -> IResult {
        let saved_span = self.current_span;
        let saved_file = self.current_file.clone();
        self.call_stack.push(crate::error::CallFrame {
            name: method.name.clone(),
            call_file: self.current_file.clone(),
            call_span: self.current_span,
        });
        // Build a child environment from the method's closure env.
        let saved_env = self.env.clone();
        self.env = method.closure_env.clone();
        self.env.push_scope();

        // Bind parameters: `self` first if instance method, then positional args.
        let mut arg_iter = args.into_iter();
        for param in &method.params {
            if param.name == "self" {
                if let Some(sv) = &self_val {
                    self.env.define("self", sv.clone(), method.mutates_self);
                } else {
                    self.env.define("self", Value::Null, method.mutates_self);
                }
            } else {
                let val = arg_iter.next()
                    .or_else(|| param.default.as_ref().and_then(|_| None)) // evaluated below
                    .unwrap_or(Value::Null);
                self.env.define(&param.name, val, true);
            }
        }

        let result = self.eval_block(&method.body);
        // Read `self` back before the frame goes away. Even a failed body has
        // to hand back what it managed to change: the alternative is a method
        // that half-updated a struct and then discarded the evidence.
        let updated_self = if method.mutates_self {
            self.env.get("self")
        } else {
            None
        };
        self.env.pop_scope();
        self.env = saved_env;
        self.current_span = saved_span;
        self.current_file = saved_file;
        self.call_stack.pop();
        if method.mutates_self {
            self.pending_self_writeback = updated_self;
        }

        match result {
            Ok(v) => Ok(v),
            Err(Signal::Return(v)) => Ok(v),
            Err(e) => Err(e),
        }
    }

    /// Store what a `mut self` method left in `self` back over the expression
    /// it was called on.
    ///
    /// `c.bump()` has to mean `c = <c after bump>`, and the only expressions
    /// that can express is a variable or a path into one. Anything else —
    /// `Counter().bump()`, `list_of()[0].bump()` — would mutate a value that
    /// is discarded on the next line, so it is an error rather than a no-op
    /// nobody can see.
    fn apply_self_writeback(&mut self, receiver: &Expr, method: &str) -> Result<(), Signal> {
        let Some(new_self) = self.pending_self_writeback.take() else {
            return Ok(());
        };
        match receiver {
            Expr::Ident(_) | Expr::FieldAccess { .. } | Expr::Index { .. } => {
                self.assign_target(receiver, new_self).map_err(|e| match e {
                    // The generic "cannot assign to X" is true but hides why
                    // an ordinary-looking call needs a mutable binding.
                    Signal::Error(err) if err.kind == ErrorKind::ImmutableVariable => {
                        Signal::Error(QueError::new(
                            ErrorKind::ImmutableVariable,
                            format!(
                                "{}() takes `mut self`, so what it is called on has to be declared with `mut` rather than `let`",
                                method
                            ),
                        ))
                    }
                    other => other,
                })
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::InvalidAssignmentTarget,
                format!(
                    "{}() takes `mut self`, so it needs a receiver it can write back to: call it on a variable, not on a temporary",
                    method
                ),
            ))),
        }
    }

    // ── Indexing ─────────────────────────────────────────────────────

    // ── Indexing ─────────────────────────────────────────────────────

    fn index_into(&self, obj: &Value, idx: &Value) -> IResult {
        match (obj, idx) {
            (Value::List(items), Value::Int(i)) => {
                let index = if *i < 0 {
                    (items.len() as i64 + i) as usize
                } else {
                    *i as usize
                };
                items.get(index).cloned().ok_or_else(|| {
                    Signal::Error(QueError::new(
                        ErrorKind::IndexOutOfBounds,
                        format!(
                            "index {} out of bounds (len {})",
                            i,
                            items.len()
                        ),
                    ))
                })
            }
            (Value::Map(map), Value::String(key)) => {
                map.get(key).cloned().ok_or_else(|| {
                    Signal::Error(QueError::new(
                        ErrorKind::KeyNotFound,
                        format!(
                            "key '{}' not found in map (use .get(\"{}\") to get null instead)",
                            key, key
                        ),
                    ))
                })
            }
            (Value::String(s), Value::Int(i)) => {
                let index = if *i < 0 {
                    (s.len() as i64 + i) as usize
                } else {
                    *i as usize
                };
                s.chars()
                    .nth(index)
                    .map(|c| Value::String(c.to_string()))
                    .ok_or_else(|| {
                        Signal::Error(QueError::new(
                            ErrorKind::IndexOutOfBounds,
                            format!(
                                "string index {} out of bounds (len {})",
                                i,
                                s.len()
                            ),
                        ))
                    })
            }
            (Value::Tuple(items), Value::Int(i)) => {
                let index = if *i < 0 {
                    (items.len() as i64 + i) as usize
                } else {
                    *i as usize
                };
                items.get(index).cloned().ok_or_else(|| {
                    Signal::Error(QueError::new(
                        ErrorKind::IndexOutOfBounds,
                        format!(
                            "tuple index {} out of bounds (len {})",
                            i,
                            items.len()
                        ),
                    ))
                })
            }
            _ => Err(Signal::Error(QueError::new(
                ErrorKind::TypeMismatch,
                format!(
                    "cannot index {} with {}",
                    obj.type_name(),
                    idx.type_name()
                ),
            ))),
        }
    }


    // ── Path / Glob literal evaluation ──────────────────────────────

    fn eval_path_lit(&mut self, parts: &[AstStringPart]) -> IResult {
        let mut result = String::new();
        for part in parts {
            match part {
                AstStringPart::Literal(s) => result.push_str(s),
                AstStringPart::Expr(expr) => {
                    let val = self.eval_expr(expr)?;
                    let seg = match &val {
                        Value::Path(p) => p.clone(),
                        Value::String(s) => s.clone(),
                        _ => val.display_string(),
                    };
                    // Avoid double-slash at join point: if both sides have a slash boundary
                    if result.ends_with('/') {
                        result.push_str(seg.trim_start_matches('/'));
                    } else if seg.starts_with('/') && !result.is_empty() {
                        // Interpolated path segment starting with / — strip to avoid double slash
                        // (The literal already contains the leading slash before ${...})
                        result.push_str(seg.trim_start_matches('/'));
                    } else {
                        result.push_str(&seg);
                    }
                }
                AstStringPart::RawExpr(expr) => {
                    let val = self.eval_expr(expr)?;
                    result.push_str(&val.display_string());
                }
            }
        }
        // `~` means the home directory here exactly as it does in `path("~/…")`
        // and in a glob. A literal that expands in one construction form and
        // not in another is a trap, and the shell it borrows the notation from
        // does not have that seam either.
        Ok(Value::Path(crate::interpreter::helpers::expand_tilde(&result)))
    }

    fn eval_glob_lit(&mut self, parts: &[AstStringPart]) -> IResult {
        let mut result = String::new();
        for part in parts {
            match part {
                AstStringPart::Literal(s) => result.push_str(s),
                AstStringPart::Expr(expr) => {
                    let val = self.eval_expr(expr)?;
                    let seg = match &val {
                        Value::Path(p) => p.clone(),
                        Value::Glob(g) => g.clone(),
                        Value::String(s) => s.clone(),
                        _ => val.display_string(),
                    };
                    if result.ends_with('/') {
                        result.push_str(seg.trim_start_matches('/'));
                    } else if seg.starts_with('/') && !result.is_empty() {
                        result.push_str(seg.trim_start_matches('/'));
                    } else {
                        result.push_str(&seg);
                    }
                }
                AstStringPart::RawExpr(expr) => {
                    let val = self.eval_expr(expr)?;
                    result.push_str(&val.display_string());
                }
            }
        }
        Ok(Value::Glob(result))
    }

    // ── String interpolation ─────────────────────────────────────────

    fn interpolate_string(&mut self, parts: &[AstStringPart]) -> IResult {
        let mut result = String::new();
        for part in parts {
            match part {
                AstStringPart::Literal(s) => result.push_str(s),
                AstStringPart::Expr(expr) => {
                    let val = self.eval_expr(expr)?;
                    result.push_str(&self.display_value(val)?);
                }
                AstStringPart::RawExpr(expr) => {
                    let val = self.eval_expr(expr)?;
                    result.push_str(&self.display_value(val)?);
                }
            }
        }
        Ok(Value::String(result))
    }

    // ── Helpers ──────────────────────────────────────────────────────

    // ── spawn implementation ──────────────────────────────────────────

    /// Evaluate a `spawn` expression: launch a command in the background.
    /// Returns a ProcessHandle value. Only works with Cmd values.
    pub(crate) fn eval_spawn(&mut self, val: Value) -> IResult {
        use crate::value::ProcessHandle;
        use std::sync::{Arc, Mutex};

        match val {
            Value::Cmd(parts, mods) => {
                if !mods.stdin_from.is_empty() {
                    // A handle tracks one process. Rather than pick a stage to
                    // hand back and leak the rest, say so.
                    return Err(Signal::Error(QueError::new(
                        ErrorKind::Runtime,
                        "spawn cannot take a `|` pipeline; write the pipe inside one command literal, e.g. `tail -f log | grep ERROR`",
                    )));
                }
                let cmd_str = crate::interpreter::methods::render_cmd(&parts);
                self.check_permission(
                    crate::permissions::Capability::Exec,
                    &crate::interpreter::methods::render_cmd_display(&parts),
                )?;

                // A `spawn` must hand back a real handle: the script will
                // `.wait()` or `.kill()` it. In a dry run we start the shell's
                // no-op instead, so the handle behaves like a process that
                // started and did nothing.
                let cmd_str = if self.dry_run_skip(format!("spawn {}", cmd_str)) {
                    crate::interpreter::helpers::shell_noop().to_string()
                } else {
                    cmd_str
                };

                let mut cmd = crate::interpreter::helpers::shell_command(&cmd_str);
                cmd.stdin(std::process::Stdio::null());
                cmd.stdout(std::process::Stdio::piped());
                cmd.stderr(std::process::Stdio::piped());

                if let Some(dir) = &mods.dir {
                    cmd.current_dir(dir);
                }
                for (key, val) in &mods.env_vars {
                    cmd.env(key, val);
                }

                let child = cmd.spawn().map_err(|e| {
                    Signal::Error(QueError::new(
                        ErrorKind::CommandFailed,
                        format!("spawn failed: {}", e),
                    ))
                })?;

                let pid = child.id();
                let handle = ProcessHandle {
                    pid,
                    child: Arc::new(Mutex::new(child)),
                };
                Ok(Value::ProcessHandle(handle))
            }
            other => Err(Signal::Error(QueError::new(
                ErrorKind::TypeMismatch,
                format!("spawn expects a command (Cmd), got {}", other.type_name()),
            ))),
        }
    }

    // ── struct literal construction ───────────────────────────────────

    /// Separated from eval_expr to keep its stack frame small enough for deep recursion.
    #[inline(never)]
    fn eval_struct_lit(&mut self, name: &str, fields: &[(String, Expr)]) -> IResult {
        let field_defs = self.struct_defs.get(name).cloned().ok_or_else(|| {
            Signal::Error(QueError::new(
                ErrorKind::UndefinedVariable,
                format!("unknown struct type '{}'", name),
            ))
        })?;
        let mut instance_fields = BTreeMap::new();
        // Fill in defaults first
        for fd in &field_defs {
            if let Some(default) = &fd.default {
                instance_fields.insert(fd.name.clone(), default.clone());
            }
        }
        // Apply provided fields
        for (field_name, field_expr) in fields {
            let field_known = field_defs.iter().any(|fd| &fd.name == field_name);
            if !field_known {
                return Err(Signal::Error(QueError::new(
                    ErrorKind::KeyNotFound,
                    format!("struct '{}' has no field '{}'", name, field_name),
                )));
            }
            let val = self.eval_expr(field_expr)?;
            instance_fields.insert(field_name.clone(), val);
        }
        // Check all required fields (no default) are present
        for fd in &field_defs {
            if fd.default.is_none() && !instance_fields.contains_key(&fd.name) {
                return Err(Signal::Error(QueError::new(
                    ErrorKind::Runtime,
                    format!("struct '{}': missing required field '{}'", name, fd.name),
                )));
            }
        }
        Ok(Value::Instance {
            type_name: name.to_string(),
            fields: instance_fields,
        })
    }

    // ── with context manager ─────────────────────────────────────────

    /// Separated from eval_expr to keep its stack frame small enough for deep recursion.
    #[inline(never)]
    fn eval_with_context(&mut self, manager: &Expr, name: &str, body: &Block) -> IResult {
        let mgr = self.eval_expr(manager)?;
        // mgr must be a Value::Instance implementing Contextual
        let type_name = match &mgr {
            Value::Instance { type_name, .. } => type_name.clone(),
            other => return Err(Signal::Error(QueError::new(
                ErrorKind::TypeMismatch,
                format!(
                    "with ... as requires a struct instance implementing Contextual, got {}",
                    other.type_name()
                ),
            ))),
        };
        if !self.implements_trait(&type_name, "Contextual") {
            return Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("type '{}' does not implement Contextual", type_name),
            )));
        }
        // Call enter()
        let enter_method = self.find_instance_method(&type_name, "enter")
            .ok_or_else(|| Signal::Error(QueError::new(
                ErrorKind::Runtime,
                format!("Contextual impl for '{}' missing 'enter'", type_name),
            )))?;
        let resource = self.call_method_def(enter_method, Some(mgr.clone()), vec![])?;
        // A `mut self` enter() has nowhere to write itself back to — the
        // manager is usually a temporary like `with Dir("/tmp")` — so the
        // block owns it, and exit() sees what enter() changed.
        let mgr = self.pending_self_writeback.take().unwrap_or(mgr);

        // Run body in new scope with resource bound to name
        self.env.push_scope();
        self.env.define(name, resource.clone(), false);
        let body_result = self.eval_block(body);
        self.env.pop_scope();

        // Always call exit(resource), even on error
        let exit_method = self.find_instance_method(&type_name, "exit");
        if let Some(exit_m) = exit_method {
            let _ = self.call_method_def(exit_m, Some(mgr), vec![resource]);
            self.pending_self_writeback = None;
        }

        body_result
    }

    // ── parallel implementation ──────────────────────────────────────

    /// Evaluate a `parallel { ... }` block.
    ///
    /// Every branch runs on its own OS thread. A branch gets its own
    /// `Interpreter` (a clone of this one) that *shares* the caller's variable
    /// scopes, so it can read outer variables and every scope it pushes of its
    /// own stays private to it.
    ///
    /// Output is buffered per branch and replayed in source order after the
    /// join, so a parallel block reads the same way every run even though the
    /// work inside it did not happen in that order.
    ///
    /// If several branches fail, the first one in source order is the error
    /// that propagates — the same rule as a pipeline, and the only one that
    /// does not depend on scheduling.
    pub(crate) fn eval_parallel(&mut self, branches: &[crate::ast::ParallelBranch]) -> IResult {
        let all_named = branches.iter().all(|b| b.label.is_some());
        let all_unnamed = branches.iter().all(|b| b.label.is_none());

        if !all_named && !all_unnamed {
            return Err(Signal::Error(QueError::new(
                ErrorKind::Runtime,
                "parallel block must have either all named or all unnamed branches",
            )));
        }

        let outcomes: Vec<(Vec<String>, IResult)> = std::thread::scope(|scope| {
            let handles: Vec<_> = branches
                .iter()
                .map(|branch| {
                    let mut sub = self.clone();
                    // Buffer: branches finish in arbitrary order, and letting
                    // them all write to the terminal at once would interleave
                    // half-lines from different jobs.
                    sub.direct_output = false;
                    sub.output.clear();
                    sub.partial_line.clear();
                    scope.spawn(move || {
                        let result = sub.eval_expr(&branch.body);
                        sub.flush_partial();
                        (sub.output, result)
                    })
                })
                .collect();

            handles
                .into_iter()
                .map(|h| {
                    h.join().unwrap_or_else(|_| {
                        (
                            Vec::new(),
                            Err(Signal::Error(QueError::new(
                                ErrorKind::Runtime,
                                "a parallel branch panicked",
                            ))),
                        )
                    })
                })
                .collect()
        });

        let mut values = Vec::with_capacity(outcomes.len());
        let mut first_error = None;
        for (output, result) in outcomes {
            for line in output {
                self.emit(line);
            }
            match result {
                Ok(v) => values.push(v),
                Err(sig) => {
                    values.push(Value::Null);
                    if first_error.is_none() {
                        first_error = Some(sig);
                    }
                }
            }
        }
        if let Some(sig) = first_error {
            return Err(sig);
        }

        if all_named {
            let mut map = std::collections::BTreeMap::new();
            for (branch, value) in branches.iter().zip(values) {
                map.insert(branch.label.clone().unwrap(), value);
            }
            Ok(Value::Map(map))
        } else {
            Ok(Value::Tuple(values))
        }
    }
}

/// The meaning of `?`: take the value out of a success, turn a failure into a
/// raised error, and leave anything else alone.
///
/// Shared with `?.`, which is `?` followed by an access, so that
/// `res?.field` reads the field of the value rather than of the `Ok` around it.
pub(crate) fn try_unwrap(val: Value) -> Result<Value, Signal> {
    match val {
        Value::Ok(v) => Ok(*v),
        Value::Err(e) => Err(Signal::Error(crate::interpreter::err_value_to_error(&e))),
        Value::ProcessResult {
            exit_code,
            stderr,
            stdout,
        } => {
            if exit_code == 0 {
                Ok(Value::String(stdout))
            } else {
                Err(Signal::Error(QueError::new(
                    ErrorKind::CommandFailed,
                    format!("command failed (exit {}): {}", exit_code, stderr.trim()),
                )))
            }
        }
        // Non-Result/Option values pass through.
        other => Ok(other),
    }
}

/// Check that a value matches a type annotation (for strict mode).
fn check_type(val: &Value, ty: &TypeExpr, param_name: &str, func_name: &Option<String>) -> Result<(), Signal> {
    if value_matches_type(val, ty) {
        Ok(())
    } else {
        let fn_label = func_name
            .as_deref()
            .unwrap_or("<anonymous>");
        Err(Signal::Error(QueError::new(
            ErrorKind::TypeMismatch,
            format!(
                "parameter '{}' of {} expected {}, got {} ({})",
                param_name,
                fn_label,
                ty,
                val.type_name(),
                val.display_string(),
            ),
        )))
    }
}

/// Check that a return value matches the declared return type (for strict mode).
fn check_return_type(val: &Value, ty: &TypeExpr, func_name: &Option<String>) -> Result<(), Signal> {
    if value_matches_type(val, ty) {
        Ok(())
    } else {
        let fn_label = func_name
            .as_deref()
            .unwrap_or("<anonymous>");
        Err(Signal::Error(QueError::new(
            ErrorKind::TypeMismatch,
            format!(
                "{} return type expected {}, got {} ({})",
                fn_label,
                ty,
                val.type_name(),
                val.display_string(),
            ),
        )))
    }
}

/// Check if a runtime value matches a type annotation.
fn value_matches_type(val: &Value, ty: &TypeExpr) -> bool {
    match ty {
        TypeExpr::Named(name) => match name.as_str() {
            "Int" => matches!(val, Value::Int(_)),
            "Float" => matches!(val, Value::Float(_)),
            "Bool" => matches!(val, Value::Bool(_)),
            "String" => matches!(val, Value::String(_)),
            "Path" => matches!(val, Value::Path(_)),
            "Duration" => matches!(val, Value::Duration(_, _)),
            "Semver" => matches!(val, Value::Semver(_)),
            "Regex" => matches!(val, Value::Regex(_)),
            "Secret" => matches!(val, Value::Secret(_)),
            "Null" => matches!(val, Value::Null),
            "Cmd" => matches!(val, Value::Cmd(_, _)),
            "ProcessResult" => matches!(val, Value::ProcessResult { .. }),
            "ProcessHandle" => matches!(val, Value::ProcessHandle(_)),
            "List" => matches!(val, Value::List(_)),
            "Map" => matches!(val, Value::Map(_)),
            "Set" => matches!(val, Value::Set(_)),
            "Tuple" => matches!(val, Value::Tuple(_)),
            "Function" => matches!(val, Value::Function { .. } | Value::BuiltinFn(_)),
            "Stream" => matches!(val, Value::Stream(_)),
            "Bytes" => val.type_name() == "Bytes",
            "FileHandle" => matches!(val, Value::FileHandle(_)),
            "Any" => true,
            // Check for struct instances
            _ => {
                if let Value::Instance { type_name: ref inst_name, .. } = val {
                    inst_name == name
                } else if let Value::Ok(_) = val {
                    name == "Ok"
                } else if let Value::Err(_) = val {
                    name == "Err"
                } else {
                    false
                }
            }
        },
        TypeExpr::Generic(name, _args) => {
            // For generics like List<Int>, Result<T, E>:
            // we check the outer type only (no inner type checking yet)
            match name.as_str() {
                "List" => matches!(val, Value::List(_)),
                "Map" => matches!(val, Value::Map(_)),
                "Set" => matches!(val, Value::Set(_)),
                "Result" => matches!(val, Value::Ok(_) | Value::Err(_)),
                _ => true,
            }
        }
    }
}

/// Append the values an assertion produced to the source it was written as.
///
/// When the condition is made of literals the two read the same, and repeating
/// `"prod" == "dev"  ("prod" == "dev")` is noise rather than information.
fn annotate(source: &str, values: &str) -> String {
    if source == values {
        source.to_string()
    } else {
        format!("{}  ({})", source, values)
    }
}

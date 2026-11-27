//! Names a file reaches through its `import:` items (spec 18, the Go model).
//!
//! * `import: utils` -> `utils.add()` and `utils.count`; the short names are
//!   not in scope (a bare `add()` is SEM007).
//! * `import: utils as U` -> only `U.add()`.
//! * `import: data.models` (data/models.cln) -> `models.fn()`, the last
//!   segment.
//! * `import: utils.add` -> only `add()`; `utils.add as plus` -> `plus()`.
//!
//! Files joined by `shared:` folders or by file-path imports share one
//! namespace and keep short names; classes stay unqualified.
//!
//! The rest of the pipeline addresses functions and state variables by name
//! in one namespace. This pass runs per file before the files merge: it
//! rewrites every qualified or aliased reference to the internal name of the
//! member it reaches, and reports a bare reference to a name that the file
//! only imported by name.

use std::collections::{HashMap, HashSet};

use super::private_scope::PrivateRenames;
use super::{
    HirBlock, HirClass, HirExpression, HirFunction, HirLValue, HirProgram, HirStateBlock,
    HirStatement,
};
use crate::ast::SourceLocation;
use crate::error::CompilerError;

/// What one imported module offers.
pub struct ModuleMembers {
    /// Module name as written in `import:`.
    pub name: String,
    /// Top-level functions: `(name, is_private)`.
    pub functions: Vec<(String, bool)>,
    /// State variables: `(name, is_private)`.
    pub state: Vec<(String, bool)>,
    /// Internal names the merge gave the module's private members.
    pub renames: PrivateRenames,
}

impl ModuleMembers {
    fn function(&self, name: &str) -> Option<String> {
        self.functions
            .iter()
            .any(|(n, _)| n == name)
            .then(|| internal_name(&self.renames.functions, name))
    }

    fn state_var(&self, name: &str) -> Option<String> {
        self.state
            .iter()
            .any(|(n, _)| n == name)
            .then(|| internal_name(&self.renames.state, name))
    }
}

fn internal_name(renames: &HashMap<String, String>, name: &str) -> String {
    renames
        .get(name)
        .cloned()
        .unwrap_or_else(|| name.to_string())
}

/// One by-name import item of the file being scoped.
pub enum ImportedModule<'a> {
    /// `utils`, `utils as U`, `data.models`: reached through `namespace`.
    Module {
        namespace: String,
        members: &'a ModuleMembers,
        /// The module's names are also visible unqualified (a `shared:`
        /// folder file, or joined by a file-path import).
        joined: bool,
    },
    /// `utils.add`, `utils.add as plus`: one symbol, in scope as `local`.
    Symbol {
        symbol: String,
        local: String,
        members: &'a ModuleMembers,
        joined: bool,
    },
}

/// How the names of one file resolve.
pub struct ImportScope<'a> {
    /// Names the file declares itself; they shadow imported ones.
    own: HashSet<String>,
    namespaces: HashMap<String, &'a ModuleMembers>,
    /// Modules named by a symbol import only (`import: utils.add`): not
    /// usable as a namespace.
    symbol_only_modules: HashSet<String>,
    /// Imported symbol (as written locally) -> internal function name.
    function_symbols: HashMap<String, String>,
    /// Imported symbol (as written locally) -> internal state name.
    state_symbols: HashMap<String, String>,
    /// Unqualified function name -> help for the SEM007 it raises.
    hidden_functions: HashMap<String, String>,
    hidden_state: HashMap<String, String>,
}

impl<'a> ImportScope<'a> {
    pub fn new(own: HashSet<String>, imports: &[ImportedModule<'a>]) -> Self {
        let mut scope = ImportScope {
            own,
            namespaces: HashMap::new(),
            symbol_only_modules: HashSet::new(),
            function_symbols: HashMap::new(),
            state_symbols: HashMap::new(),
            hidden_functions: HashMap::new(),
            hidden_state: HashMap::new(),
        };
        // Names a symbol import brings in under their own name.
        let mut granted: HashSet<String> = HashSet::new();
        for import in imports {
            if let ImportedModule::Symbol {
                symbol,
                local,
                members,
                ..
            } = import
            {
                if let Some(internal) = members.function(symbol) {
                    scope.function_symbols.insert(local.clone(), internal);
                }
                if let Some(internal) = members.state_var(symbol) {
                    scope.state_symbols.insert(local.clone(), internal);
                }
                if local == symbol {
                    granted.insert(symbol.clone());
                }
            }
        }
        for import in imports {
            let (members, joined, namespace, symbol) = match import {
                ImportedModule::Module {
                    namespace,
                    members,
                    joined,
                } => (*members, *joined, Some(namespace.as_str()), None),
                ImportedModule::Symbol {
                    symbol,
                    members,
                    joined,
                    ..
                } => (*members, *joined, None, Some(symbol.as_str())),
            };
            if let Some(namespace) = namespace {
                scope.namespaces.insert(namespace.to_string(), members);
            }
            if symbol.is_some() {
                scope.symbol_only_modules.insert(members.name.clone());
            }
            if joined {
                continue;
            }
            let via = namespace.unwrap_or(members.name.as_str());
            for (name, is_private) in &members.functions {
                if !is_private && !granted.contains(name) {
                    scope
                        .hidden_functions
                        .entry(name.clone())
                        .or_insert_with(|| {
                            format!(
                                "write {via}.{name}(...) or import {module}.{name}",
                                module = members.name
                            )
                        });
                }
            }
            for (name, is_private) in &members.state {
                if !is_private && !granted.contains(name) {
                    scope.hidden_state.entry(name.clone()).or_insert_with(|| {
                        format!(
                            "write {via}.{name} or import {module}.{name}",
                            module = members.name
                        )
                    });
                }
            }
        }
        // A module imported as a namespace is not "symbol only".
        let namespaced: HashSet<String> =
            scope.namespaces.values().map(|m| m.name.clone()).collect();
        scope
            .symbol_only_modules
            .retain(|m| !namespaced.contains(m));
        scope
    }

    fn is_empty(&self) -> bool {
        self.namespaces.is_empty()
            && self.function_symbols.is_empty()
            && self.state_symbols.is_empty()
            && self.symbol_only_modules.is_empty()
            && self.hidden_functions.is_empty()
            && self.hidden_state.is_empty()
    }
}

/// Rewrite one file's references; returns the errors found.
pub fn apply_import_scope(hir: &mut HirProgram, scope: &ImportScope<'_>) -> Vec<CompilerError> {
    if scope.is_empty() {
        return Vec::new();
    }
    let mut walker = Walker {
        scope,
        scopes: Vec::new(),
        members: HashSet::new(),
        errors: Vec::new(),
    };
    for function in &mut hir.functions {
        walker.function(function);
    }
    if let Some(start) = &mut hir.start_function {
        walker.function(start);
    }
    for class in &mut hir.classes {
        walker.class(class);
    }
    if let Some(block) = &mut hir.state {
        walker.state_block(block);
    }
    for watch in &mut hir.watch_blocks {
        walker.block(&mut watch.body);
    }
    for test in &mut hir.tests {
        walker.block(&mut test.body);
    }
    walker.errors
}

struct Walker<'s, 'a> {
    scope: &'s ImportScope<'a>,
    scopes: Vec<HashSet<String>>,
    members: HashSet<String>,
    errors: Vec<CompilerError>,
}

impl<'a> Walker<'_, 'a> {
    fn is_local(&self, name: &str) -> bool {
        self.scopes.iter().any(|s| s.contains(name))
    }

    fn declare(&mut self, name: &str) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string());
        }
    }

    /// A name that stands for a module in `ns.member`, not a local variable.
    fn namespace(&self, name: &str) -> Option<&'a ModuleMembers> {
        if self.is_local(name) || self.members.contains(name) {
            return None;
        }
        self.scope.namespaces.get(name).copied()
    }

    fn report(&mut self, message: String, help: String, location: &SourceLocation) {
        if !crate::ast::is_user_source_location(location) {
            return;
        }
        self.errors.push(CompilerError::semantic_error_with_code(
            message,
            Some(help),
            Some(location.clone()),
            "SEM007",
        ));
    }

    /// `ns.name(...)` where `ns` is a module namespace of this file: the
    /// internal function name, or an error.
    fn qualified_function(
        &mut self,
        ns: &str,
        name: &str,
        location: &SourceLocation,
    ) -> Option<Option<String>> {
        if let Some(members) = self.namespace(ns) {
            let found = members.function(name);
            let module = members.name.clone();
            if found.is_none() {
                self.report(
                    format!("Function '{}' not found in module '{}'", name, module),
                    format!("Module '{}' declares no function '{}'", module, name),
                    location,
                );
            }
            return Some(found);
        }
        self.not_a_namespace(ns, name, location);
        None
    }

    /// `ns.name` read or written, where `ns` is a module namespace.
    fn qualified_state(
        &mut self,
        ns: &str,
        name: &str,
        location: &SourceLocation,
    ) -> Option<Option<String>> {
        if let Some(members) = self.namespace(ns) {
            let found = members.state_var(name);
            let module = members.name.clone();
            if found.is_none() {
                self.report(
                    format!("'{}' not found in module '{}'", name, module),
                    format!("Module '{}' declares no state variable '{}'", module, name),
                    location,
                );
            }
            return Some(found);
        }
        self.not_a_namespace(ns, name, location);
        None
    }

    /// A module the file imported only by symbol (`import: utils.add`).
    fn is_symbol_only(&self, name: &str) -> bool {
        !self.is_local(name)
            && !self.members.contains(name)
            && self.scope.symbol_only_modules.contains(name)
    }

    /// `utils.mul()` after `import: utils.add`: `utils` was not imported as
    /// a module.
    fn not_a_namespace(&mut self, ns: &str, name: &str, location: &SourceLocation) {
        if self.is_local(ns)
            || self.members.contains(ns)
            || self.scope.namespaces.contains_key(ns)
            || !self.scope.symbol_only_modules.contains(ns)
        {
            return;
        }
        self.report(
            format!("'{ns}' is not imported as a module, so '{ns}.{name}' is not in scope"),
            format!("write import: {ns} to use {ns}.{name}, or import: {ns}.{name}"),
            location,
        );
    }

    /// Rewrite a bare function name; reports a hidden one.
    fn function_name(&mut self, name: &mut String, location: &SourceLocation) {
        if self.members.contains(name.as_str()) || self.is_local(name) {
            return;
        }
        if let Some(internal) = self.scope.function_symbols.get(name.as_str()) {
            *name = internal.clone();
            return;
        }
        if self.scope.own.contains(name.as_str()) {
            return;
        }
        if let Some(help) = self.scope.hidden_functions.get(name.as_str()) {
            let message = format!("Function '{}' is not in scope", name);
            let help = help.clone();
            self.report(message, help, location);
        }
    }

    /// Rewrite a bare state variable name; reports a hidden one.
    fn state_name(&mut self, name: &mut String, location: &SourceLocation) {
        if self.members.contains(name.as_str()) || self.is_local(name) {
            return;
        }
        if let Some(internal) = self.scope.state_symbols.get(name.as_str()) {
            *name = internal.clone();
            return;
        }
        if self.scope.own.contains(name.as_str()) {
            return;
        }
        if let Some(help) = self.scope.hidden_state.get(name.as_str()) {
            let message = format!("Variable '{}' is not in scope", name);
            let help = help.clone();
            self.report(message, help, location);
        }
    }

    /// A bare name used as a value: a local, a state variable or a function
    /// reference.
    fn variable(&mut self, name: &mut String, location: &SourceLocation) {
        if let Some((ns, member)) = name.split_once('.') {
            let (ns, member) = (ns.to_string(), member.to_string());
            if let Some(Some(internal)) = self.qualified_state(&ns, &member, location) {
                *name = internal;
            }
            return;
        }
        if self.members.contains(name.as_str()) || self.is_local(name) {
            return;
        }
        if let Some(internal) = self.scope.state_symbols.get(name.as_str()) {
            *name = internal.clone();
        } else if let Some(internal) = self.scope.function_symbols.get(name.as_str()) {
            *name = internal.clone();
        } else if self.scope.own.contains(name.as_str()) {
        } else if self.scope.hidden_state.contains_key(name.as_str()) {
            self.state_name(name, location);
        } else {
            self.function_name(name, location);
        }
    }

    fn function(&mut self, function: &mut HirFunction) {
        self.scopes.push(HashSet::new());
        for param in &function.parameters {
            self.declare(&param.name);
        }
        for param in &mut function.parameters {
            if let Some(default) = &mut param.default_value {
                self.expression(default);
            }
        }
        self.block(&mut function.body);
        self.scopes.pop();
    }

    fn class(&mut self, class: &mut HirClass) {
        let outer = std::mem::take(&mut self.members);
        self.members
            .extend(class.fields.iter().map(|f| f.name.clone()));
        self.members
            .extend(class.methods.iter().map(|m| m.name.clone()));

        for field in &mut class.fields {
            if let Some(init) = &mut field.initializer {
                self.expression(init);
            }
        }
        if let Some(ctor) = &mut class.constructor {
            self.scopes.push(HashSet::new());
            for param in &ctor.parameters {
                self.declare(&param.name);
            }
            self.block(&mut ctor.body);
            self.scopes.pop();
        }
        for method in &mut class.methods {
            self.scopes.push(HashSet::new());
            for param in &method.parameters {
                self.declare(&param.name);
            }
            for param in &mut method.parameters {
                if let Some(default) = &mut param.default_value {
                    self.expression(default);
                }
            }
            self.block(&mut method.body);
            self.scopes.pop();
        }
        for invariant in &mut class.invariants {
            self.expression(&mut invariant.condition);
        }
        self.members = outer;
    }

    fn state_block(&mut self, block: &mut HirStateBlock) {
        for decl in &mut block.declarations {
            self.expression(&mut decl.initializer);
            if let Some(guard) = &mut decl.guard {
                self.scopes.push(HashSet::from(["value".to_string()]));
                self.expression(&mut guard.condition);
                self.scopes.pop();
            }
        }
        for computed in &mut block.computed {
            self.block(&mut computed.body);
        }
        for rule in &mut block.rules {
            self.expression(&mut rule.condition);
        }
    }

    fn block(&mut self, block: &mut HirBlock) {
        self.scopes.push(HashSet::new());
        for statement in &mut block.statements {
            self.statement(statement);
        }
        self.scopes.pop();
    }

    fn statement(&mut self, statement: &mut HirStatement) {
        match statement {
            HirStatement::VariableDeclaration {
                name, initializer, ..
            } => {
                if let Some(init) = initializer {
                    self.expression(init);
                }
                self.declare(name);
            }
            HirStatement::Assignment { target, value, .. } => {
                self.expression(value);
                self.lvalue(target);
            }
            HirStatement::Expression { expression, .. }
            | HirStatement::Background { expression, .. }
            | HirStatement::Print { expression, .. } => self.expression(expression),
            HirStatement::Return { value, .. } => {
                if let Some(value) = value {
                    self.expression(value);
                }
            }
            HirStatement::If {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                self.expression(condition);
                self.block(then_branch);
                if let Some(else_branch) = else_branch {
                    self.block(else_branch);
                }
            }
            HirStatement::For {
                variable,
                iterable,
                body,
                ..
            } => {
                self.expression(iterable);
                self.scopes.push(HashSet::from([variable.clone()]));
                self.block(body);
                self.scopes.pop();
            }
            HirStatement::While {
                condition, body, ..
            } => {
                self.expression(condition);
                self.block(body);
            }
            HirStatement::LaterAssignment {
                variable,
                expression,
                ..
            } => {
                self.expression(expression);
                self.declare(variable);
            }
            HirStatement::Require { condition, .. } | HirStatement::Ensure { condition, .. } => {
                self.expression(condition)
            }
            HirStatement::Break { .. } | HirStatement::Continue { .. } => {}
        }
    }

    fn lvalue(&mut self, target: &mut HirLValue) {
        match target {
            HirLValue::Variable { name, location } => {
                let location = location.clone();
                self.variable(name, &location);
            }
            HirLValue::FieldAccess {
                object,
                field,
                location,
            } => {
                if let HirExpression::Variable { name: ns, .. } = object.as_ref() {
                    let ns = ns.clone();
                    if let Some(found) = self.qualified_state(&ns, field, location) {
                        if let Some(internal) = found {
                            *target = HirLValue::Variable {
                                name: internal,
                                location: location.clone(),
                            };
                        }
                        return;
                    }
                }
                self.expression(object);
            }
            HirLValue::Index { array, index, .. } => {
                self.expression(array);
                self.expression(index);
            }
        }
    }

    /// Replace `ns.fn(args)` / `ns.state` with the member it names.
    /// `Some(())` when `expression` was a qualified reference (handled,
    /// whether or not it resolved).
    fn qualified(&mut self, expression: &mut HirExpression) -> Option<()> {
        let (ns, name, arguments, location) = match expression {
            HirExpression::NamespaceCall {
                namespace,
                function,
                arguments,
                location,
            } => (
                namespace.clone(),
                function.clone(),
                Some(std::mem::take(arguments)),
                location.clone(),
            ),
            HirExpression::StaticMethodCall {
                namespace,
                class_name,
                method,
                arguments,
                location,
            } if namespace.is_empty() => (
                class_name.clone(),
                method.clone(),
                Some(std::mem::take(arguments)),
                location.clone(),
            ),
            HirExpression::MethodCall {
                receiver,
                method,
                arguments,
                argument_labels,
                location,
            } if argument_labels.is_empty()
                && matches!(receiver.as_ref(), HirExpression::Variable { name, .. } if self.namespace(name).is_some() || self.is_symbol_only(name)) =>
            {
                let HirExpression::Variable { name: ns, .. } = receiver.as_ref() else {
                    return None;
                };
                (
                    ns.clone(),
                    method.clone(),
                    Some(std::mem::take(arguments)),
                    location.clone(),
                )
            }
            HirExpression::FieldAccess {
                object,
                field,
                location,
            } => {
                let HirExpression::Variable { name: ns, .. } = object.as_ref() else {
                    return None;
                };
                let (ns, field, location) = (ns.clone(), field.clone(), location.clone());
                let found = self.qualified_state(&ns, &field, &location)?;
                if let Some(internal) = found {
                    *expression = HirExpression::Variable {
                        name: internal,
                        location,
                    };
                }
                return Some(());
            }
            _ => return None,
        };
        let mut arguments = arguments.unwrap_or_default();
        let call = self.qualified_function(&ns, &name, &location);
        match call {
            Some(Some(internal)) => {
                for arg in &mut arguments {
                    self.expression(arg);
                }
                *expression = HirExpression::Call {
                    function: internal,
                    arguments,
                    location,
                };
                Some(())
            }
            Some(None) => Some(()),
            None => {
                // Not a module of this file: put the arguments back untouched
                // and let the later stages resolve the call.
                match expression {
                    HirExpression::NamespaceCall { arguments: a, .. }
                    | HirExpression::StaticMethodCall { arguments: a, .. }
                    | HirExpression::MethodCall { arguments: a, .. } => *a = arguments,
                    _ => {}
                }
                None
            }
        }
    }

    fn expression(&mut self, expression: &mut HirExpression) {
        if self.qualified(expression).is_some() {
            return;
        }
        match expression {
            HirExpression::Literal { .. } => {}
            HirExpression::Variable { name, location } => {
                let location = location.clone();
                self.variable(name, &location);
            }
            HirExpression::BinaryOp { left, right, .. } => {
                self.expression(left);
                self.expression(right);
            }
            HirExpression::UnaryOp { operand, .. } => self.expression(operand),
            HirExpression::Call {
                function,
                arguments,
                location,
            } => {
                let location = location.clone();
                if let Some((ns, member)) = function.split_once('.') {
                    let (ns, member) = (ns.to_string(), member.to_string());
                    if let Some(Some(internal)) = self.qualified_function(&ns, &member, &location) {
                        *function = internal;
                    }
                } else {
                    self.function_name(function, &location);
                }
                for arg in arguments {
                    self.expression(arg);
                }
            }
            HirExpression::MethodCall {
                receiver,
                arguments,
                ..
            } => {
                self.expression(receiver);
                for arg in arguments {
                    self.expression(arg);
                }
            }
            HirExpression::FieldAccess { object, .. } => self.expression(object),
            HirExpression::Index { array, index, .. } => {
                self.expression(array);
                self.expression(index);
            }
            HirExpression::Array { elements, .. } => {
                for element in elements {
                    self.expression(element);
                }
            }
            HirExpression::Constructor { arguments, .. }
            | HirExpression::BaseCall { arguments, .. }
            | HirExpression::NamespaceCall { arguments, .. }
            | HirExpression::StaticMethodCall { arguments, .. } => {
                for arg in arguments {
                    self.expression(arg);
                }
            }
            HirExpression::Cast { expression, .. } => self.expression(expression),
            HirExpression::OnError {
                expression,
                fallback,
                ..
            } => {
                self.expression(expression);
                self.expression(fallback);
            }
            HirExpression::Conditional {
                condition,
                then_expr,
                else_expr,
                ..
            } => {
                self.expression(condition);
                self.expression(then_expr);
                self.expression(else_expr);
            }
            HirExpression::Range {
                start, end, step, ..
            } => {
                self.expression(start);
                self.expression(end);
                if let Some(step) = step {
                    self.expression(step);
                }
            }
            HirExpression::ObjectLiteral { fields, .. } => {
                for (_, value) in fields {
                    self.expression(value);
                }
            }
        }
    }
}

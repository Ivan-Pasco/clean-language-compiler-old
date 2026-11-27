//! File-private names for multi-file programs.
//!
//! Spec 18-modules-and-imports "Visibility Model": everything a file declares
//! is private to that file unless it sits under `public:`. Two files may
//! therefore each declare a private `helper()` or `state: calls`, and the
//! importing file may declare its own.
//!
//! The rest of the pipeline (resolver, type checker, MIR, WASM) addresses
//! functions and state variables by name in one global namespace, so the
//! merge step gives every private name that another file also declares a
//! file-qualified internal name before the modules are combined. Names
//! that no other file declares are left alone, so diagnostics keep the
//! names the user wrote.

use std::collections::{HashMap, HashSet};

use super::{
    HirBlock, HirClass, HirExpression, HirFunction, HirLValue, HirProgram, HirStateBlock,
    HirStatement,
};

/// Internal-name substitutions for one module.
#[derive(Debug, Default, Clone)]
pub struct PrivateRenames {
    /// Private function name -> internal name.
    pub functions: HashMap<String, String>,
    /// Private state variable name -> internal name.
    pub state: HashMap<String, String>,
}

impl PrivateRenames {
    pub fn is_empty(&self) -> bool {
        self.functions.is_empty() && self.state.is_empty()
    }
}

/// One module's declared top-level function and state names.
pub struct ModuleDeclarations {
    /// Suffix that makes internal names unique to this module.
    pub qualifier: String,
    /// Entry-module privates keep their names; other modules are renamed
    /// around them.
    pub is_entry: bool,
    /// The module is reached only through by-name imports (not a `shared:`
    /// folder file, not a file-path import): its public names are its own
    /// namespace, so a public name another file also declares gets an
    /// internal name too (spec decision 2026-11-27).
    pub namespaced: bool,
    pub functions: Vec<(String, bool)>,
    pub state: Vec<(String, bool)>,
}

impl ModuleDeclarations {
    /// Collect the names a module declares. `is_user_source` filters out
    /// functions that a plugin injected into the module.
    pub fn collect(
        hir: &HirProgram,
        qualifier: &str,
        is_entry: bool,
        namespaced: bool,
        is_user_source: impl Fn(&HirFunction) -> bool,
    ) -> Self {
        let functions = hir
            .functions
            .iter()
            .filter(|f| is_user_source(f) && f.owner_screen.is_none())
            .map(|f| (f.name.clone(), f.is_private))
            .collect();
        let mut state = Vec::new();
        if let Some(block) = &hir.state {
            for decl in &block.declarations {
                state.push((decl.name.clone(), decl.is_private));
            }
            for computed in &block.computed {
                state.push((computed.name.clone(), false));
            }
        }
        Self {
            qualifier: qualifier.to_string(),
            is_entry,
            namespaced,
            functions,
            state,
        }
    }
}

/// Decide, per module, which names need an internal name: a private name of
/// a non-entry module that any other module also declares, and a public name
/// of a by-name-imported module (`namespaced`) that any other module also
/// declares. Public names of files joined into one namespace are never
/// renamed: a clash there stays a SEM003.
pub fn plan_renames(modules: &[ModuleDeclarations]) -> Vec<PrivateRenames> {
    // name -> number of modules declaring it (function or state).
    let mut declared_in: HashMap<&str, usize> = HashMap::new();
    for module in modules {
        let mut names: HashSet<&str> = HashSet::new();
        names.extend(module.functions.iter().map(|(n, _)| n.as_str()));
        names.extend(module.state.iter().map(|(n, _)| n.as_str()));
        for name in names {
            *declared_in.entry(name).or_insert(0) += 1;
        }
    }

    modules
        .iter()
        .map(|module| {
            let mut renames = PrivateRenames::default();
            if module.is_entry {
                return renames;
            }
            let internal = |name: &str| format!("{}__{}", name, module.qualifier);
            for (name, is_private) in &module.functions {
                if (*is_private || module.namespaced)
                    && declared_in.get(name.as_str()).copied().unwrap_or(0) > 1
                {
                    renames.functions.insert(name.clone(), internal(name));
                }
            }
            for (name, is_private) in &module.state {
                if (*is_private || module.namespaced)
                    && declared_in.get(name.as_str()).copied().unwrap_or(0) > 1
                {
                    renames.state.insert(name.clone(), internal(name));
                }
            }
            renames
        })
        .collect()
}

/// Apply a module's renames to its HIR: declarations and every reference
/// that is not shadowed by a local variable, parameter or class member.
pub fn apply_renames(hir: &mut HirProgram, renames: &PrivateRenames) {
    if renames.is_empty() {
        return;
    }
    let mut walker = Walker::new(renames);

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
        for target in &mut watch.targets {
            if let Some(new) = renames.state.get(target) {
                *target = new.clone();
            }
        }
        walker.block(&mut watch.body);
    }
    for test in &mut hir.tests {
        walker.block(&mut test.body);
    }
}

struct Walker<'a> {
    renames: &'a PrivateRenames,
    /// Local scopes (parameters, declared variables, loop variables).
    scopes: Vec<HashSet<String>>,
    /// Names the enclosing class declares as fields or methods; a bare use
    /// inside a method refers to the member, not the file-level name.
    members: HashSet<String>,
}

impl<'a> Walker<'a> {
    fn new(renames: &'a PrivateRenames) -> Self {
        Self {
            renames,
            scopes: Vec::new(),
            members: HashSet::new(),
        }
    }

    fn is_local(&self, name: &str) -> bool {
        self.scopes.iter().any(|s| s.contains(name))
    }

    fn declare(&mut self, name: &str) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string());
        }
    }

    fn function(&mut self, function: &mut HirFunction) {
        if let Some(new) = self.renames.functions.get(&function.name) {
            if (function.is_private || crate::ast::is_user_source_location(&function.location))
                && function.owner_screen.is_none()
            {
                function.name = new.clone();
            }
        }
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
            if let Some(new) = self.renames.state.get(&decl.name) {
                decl.name = new.clone();
            }
            self.expression(&mut decl.initializer);
            if let Some(guard) = &mut decl.guard {
                self.scopes.push(HashSet::from(["value".to_string()]));
                self.expression(&mut guard.condition);
                self.scopes.pop();
            }
        }
        for computed in &mut block.computed {
            if let Some(new) = self.renames.state.get(&computed.name) {
                computed.name = new.clone();
            }
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
            HirLValue::Variable { name, .. } => self.variable(name),
            HirLValue::FieldAccess { object, .. } => self.expression(object),
            HirLValue::Index { array, index, .. } => {
                self.expression(array);
                self.expression(index);
            }
        }
    }

    /// A bare name used as a value: a local, a class member, a state
    /// variable or a function reference. Only the last two are renamed.
    fn variable(&self, name: &mut String) {
        if self.is_local(name) || self.members.contains(name.as_str()) {
            return;
        }
        if let Some(new) = self
            .renames
            .state
            .get(name.as_str())
            .or_else(|| self.renames.functions.get(name.as_str()))
        {
            *name = new.clone();
        }
    }

    fn expression(&mut self, expression: &mut HirExpression) {
        match expression {
            HirExpression::Literal { .. } => {}
            HirExpression::Variable { name, .. } => self.variable(name),
            HirExpression::BinaryOp { left, right, .. } => {
                self.expression(left);
                self.expression(right);
            }
            HirExpression::UnaryOp { operand, .. } => self.expression(operand),
            HirExpression::Call {
                function,
                arguments,
                ..
            } => {
                if !self.members.contains(function.as_str()) {
                    if let Some(new) = self.renames.functions.get(function.as_str()) {
                        *function = new.clone();
                    }
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

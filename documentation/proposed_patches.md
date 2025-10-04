# Proposed Patches (for Evaluation)

This document presents concrete patch options to align the compiler with the Clean Language Specification and recent decisions. These are intended for review and staged adoption.

## 1) Grammar: Tabs-Only Indentation (Strict)

Rationale: The spec mandates tabs-only indentation (one tab per level). The grammar currently accepts spaces. This patch enforces tabs-only at the grammar level. Optionally, we can also add a pre-parse linter for friendlier errors when spaces are used.

Patch:

```diff
--- a/src/parser/grammar.pest
+++ b/src/parser/grammar.pest
@@
-INDENT = _{ " " | "\t" }
+INDENT = _{ "\t" }
```

Notes:
- Consider adding a small pre-parse scan to emit a clearer message when a line starts with spaces (instead of letting the grammar fail ambiguously).

## 2) Lexer: Keyword Additions (background)

Rationale: The grammar and spec use `background`. The specification lexer’s keyword table lacks this value. We add it to `TokenKind` and `Keywords::lookup`.

Patch:

```diff
--- a/src/lexer/specification_token.rs
+++ b/src/lexer/specification_token.rs
@@
 pub enum TokenKind {
@@
     Or,           // or
     Print,        // print
     Println,      // println
     Return,       // return
     Start,        // start
     Step,         // step
+    Background,   // background
     Test,         // test
     Tests,        // tests
     This,         // this
@@
 impl Keywords {
     /// Check if identifier is a keyword, return appropriate token kind
     pub fn lookup(identifier: &str) -> Option<TokenKind> {
         match identifier {
@@
             "start" => Some(TokenKind::Start),
             "step" => Some(TokenKind::Step),
+            "background" => Some(TokenKind::Background),
             "test" => Some(TokenKind::Test),
             "tests" => Some(TokenKind::Tests),
             "this" => Some(TokenKind::This),
             "to" => Some(TokenKind::To),
             "true" => Some(TokenKind::True),
             "while" => Some(TokenKind::While),
```

Notes:
- We deliberately do not add `base` as a lexer keyword; the grammar handles `base` in expressions, and keeping it as an identifier avoids over-tokenization given the current parsing strategy.

## 3) Lexer: Number Literal Enhancements (0x/0b/0o)

Rationale: The spec supports hex (`0x`), binary (`0b`), and octal (`0o`) integers. The specification lexer currently only parses decimal/floats. This patch enhances `read_number_literal` to detect and parse base-prefixed integers.

Patch snippet (core changes only):

```diff
--- a/src/lexer/specification_lexer.rs
+++ b/src/lexer/specification_lexer.rs
@@
     /// Read number literal with optional precision modifier
     fn read_number_literal(&mut self) -> Result<Token, LexError> {
         let start_location = self.current_location();
         let start_pos = self.current_pos;

         let mut number_text = String::new();
         let mut is_float = false;

+        // Base-prefixed integer detection: 0x..., 0b..., 0o...
+        if let Some(&'0') = self.peek() {
+            if let Some(chars) = self.peek_chars(2) {
+                match chars.as_slice() {
+                    ['0', 'x'] | ['0', 'X'] => {
+                        // consume 0x
+                        self.advance(); self.advance();
+                        let mut digits = String::new();
+                        while let Some(&ch) = self.peek() {
+                            if ch.is_ascii_hexdigit() { digits.push(ch); self.advance(); } else { break; }
+                        }
+                        let text = self.source_text_range(start_pos, self.current_pos);
+                        let value = i64::from_str_radix(&digits, 16).map_err(|_| LexError::InvalidNumber { text: text.clone(), location: start_location.clone() })?;
+                        return Ok(Token::new(TokenKind::IntegerLiteral(value), start_location, text));
+                    }
+                    ['0', 'b'] | ['0', 'B'] => {
+                        self.advance(); self.advance();
+                        let mut digits = String::new();
+                        while let Some(&ch) = self.peek() {
+                            if ch == '0' || ch == '1' { digits.push(ch); self.advance(); } else { break; }
+                        }
+                        let text = self.source_text_range(start_pos, self.current_pos);
+                        let value = i64::from_str_radix(&digits, 2).map_err(|_| LexError::InvalidNumber { text: text.clone(), location: start_location.clone() })?;
+                        return Ok(Token::new(TokenKind::IntegerLiteral(value), start_location, text));
+                    }
+                    ['0', 'o'] | ['0', 'O'] => {
+                        self.advance(); self.advance();
+                        let mut digits = String::new();
+                        while let Some(&ch) = self.peek() {
+                            if ch >= '0' && ch <= '7' { digits.push(ch); self.advance(); } else { break; }
+                        }
+                        let text = self.source_text_range(start_pos, self.current_pos);
+                        let value = i64::from_str_radix(&digits, 8).map_err(|_| LexError::InvalidNumber { text: text.clone(), location: start_location.clone() })?;
+                        return Ok(Token::new(TokenKind::IntegerLiteral(value), start_location, text));
+                    }
+                    _ => {}
+                }
+            }
+        }

         // Read integer part
         while let Some(&ch) = self.peek() {
             if ch.is_ascii_digit() {
                 number_text.push(ch);
                 self.advance();
             } else {
                 break;
             }
         }
         // ... rest of existing float/precision handling remains unchanged ...
```

Notes:
- Negative base-prefixed numbers (e.g., `-0xFF`) can be handled at the parser level as unary minus, which aligns with many language grammars.
- If you prefer `-0xFF` to lex as a single token, extend lookbehind for a leading `-` here as well.

## 4) Semantics: Method Parentheses Validation (helpers must use `()`)

Rationale: The spec requires helper methods to be called with parentheses (e.g., `x.toString()`, not `x.toString`). We add a semantic validation that flags property access using known helper names without `()`.

Patch (illustrative; hook placement may vary depending on your semantic traversal):

```diff
--- a/src/semantic/mod.rs
+++ b/src/semantic/mod.rs
@@
 impl SemanticAnalyzer {
@@
     pub fn new() -> Self { /* unchanged */ }

+    fn validate_method_parentheses(&mut self, expr: &crate::ast::Expression) {
+        use crate::ast::Expression;
+        // Known helper names that must be invoked as methods
+        const HELPERS: &[&str] = &[
+            "length", "toString", "toInteger", "toNumber", "toBoolean",
+            "isDefined", "isNotDefined", "isEmpty", "isNotEmpty",
+            "mustBeTrue", "mustBeFalse", "mustBeEqual", "keepBetween",
+        ];
+        match expr {
+            Expression::PropertyAccess { property, location, .. } => {
+                if HELPERS.contains(&property.as_str()) {
+                    let loc = location.clone();
+                    self.enhanced_error_collector.add_error(
+                        CompilerError::semantic_error(
+                            format!("Helper '{}' must be called with parentheses", property),
+                            loc.clone(),
+                            Some("Use 'obj.helper()' instead of 'obj.helper'".to_string()),
+                        )
+                    );
+                }
+            }
+            // Recurse where needed
+            Expression::Binary(l, _, r) => { self.validate_method_parentheses(l); self.validate_method_parentheses(r); }
+            Expression::Unary(_, e) => self.validate_method_parentheses(e),
+            Expression::MethodCall { object, arguments, .. } => {
+                self.validate_method_parentheses(object);
+                for a in arguments { self.validate_method_parentheses(a); }
+            }
+            Expression::PropertyAssignment { object, value, .. } => {
+                self.validate_method_parentheses(object);
+                self.validate_method_parentheses(value);
+            }
+            Expression::NamespaceCall { arguments, .. } => {
+                for a in arguments { self.validate_method_parentheses(a); }
+            }
+            Expression::ListAccess(arr, idx) => { self.validate_method_parentheses(arr); self.validate_method_parentheses(idx); }
+            Expression::MatrixAccess(a, r, c) => { self.validate_method_parentheses(a); self.validate_method_parentheses(r); self.validate_method_parentheses(c); }
+            Expression::OnError { expression, fallback, .. } => { self.validate_method_parentheses(expression); self.validate_method_parentheses(fallback); }
+            Expression::OnErrorBlock { expression, .. } => { self.validate_method_parentheses(expression); }
+            Expression::Conditional { condition, then_expr, else_expr, .. } => {
+                self.validate_method_parentheses(condition);
+                self.validate_method_parentheses(then_expr);
+                self.validate_method_parentheses(else_expr);
+            }
+            Expression::ObjectCreation { arguments, .. } => { for a in arguments { self.validate_method_parentheses(a); } }
+            Expression::StartExpression { expression, .. } => self.validate_method_parentheses(expression),
+            Expression::StringInterpolation(parts) => {
+                for p in parts { if let crate::ast::StringPart::Interpolation(e) = p { self.validate_method_parentheses(e); } }
+            }
+            _ => {}
+        }
+    }
+
+    // Example hook: call after/before existing expression analysis
+    fn analyze_expression(&mut self, expr: &crate::ast::Expression) {
+        self.validate_method_parentheses(expr);
+        // ... existing analysis logic ...
+    }
 }
```

Notes:
- The exact call site should match where expressions are already validated. If there’s a central expression visitor, integrate the call there. Otherwise, call `validate_method_parentheses` from statement/function validation paths.
- The helper list can live in a shared constants module if preferred.

---

These patches can be applied independently. Recommended order: (1) Grammar tabs-only, (2) Lexer keywords, (3) Lexer number bases, (4) Semantic validation.


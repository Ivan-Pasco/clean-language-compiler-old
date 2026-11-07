//! Source positions for code a plugin hands back as Clean source.
//!
//! `_emit_stmt_from_source` / `_emit_expr_from_source` parse a fragment the
//! plugin assembled from the block body it received (an endpoint handler, a
//! component event handler, a query). The parser can only number the
//! fragment's own lines, and plugins do not pass reliable offsets (they
//! re-indent, rewrite `return` lines and add their own lines), so the parsed
//! code carried `<plugin-fragment>` locations whose lines meant nothing in
//! the user's file (compiler-old #27).
//!
//! [`FragmentOrigin`] moves those locations back to the user's file. Each
//! fragment line is matched against the block's content lines: equal text
//! after trimming is an exact match (line and column are exact); a content
//! line whose words all appear, in order, in the fragment line is a rewrite
//! of it (the line is exact, the column is where the content line starts).
//! The matched lines are aligned in order, so a fragment maps to one region
//! of the block even when several handlers share lines. Fragment lines that
//! match nothing were written by the plugin and are marked as its code
//! (`<plugin:NAME>`), so they are never reported as the user's.
//!
//! Nothing here knows any plugin or DSL: it compares lines of text.

use crate::ast::{Expression, FrameworkBlock, SourceLocation, Statement};
use std::collections::HashMap;

/// File name the fragment parser gives the code it parses
/// (`<plugin-fragment>`, `<plugin-fragment-expr>`).
const FRAGMENT_FILE_PREFIX: &str = "<plugin-fragment";

/// Score of an exact line match and of a rewritten-line match.
const EXACT: usize = 3;
const REWRITE: usize = 1;

/// The user's block a plugin is expanding, as needed to place the code it
/// parses from fragments.
#[derive(Debug, Clone)]
struct BlockText {
    file: String,
    /// Source line and column of content line 0, column 1.
    first_line: usize,
    first_column: usize,
    lines: Vec<String>,
    /// Trimmed content line -> content line indices, ascending.
    by_text: HashMap<String, Vec<usize>>,
    words: Vec<Vec<String>>,
}

/// Where code parsed from a plugin's fragments came from.
#[derive(Debug, Clone)]
pub(crate) struct FragmentOrigin {
    /// `<plugin:NAME>` marker for code the plugin wrote itself.
    plugin_marker: String,
    block: Option<BlockText>,
    /// Content line after the last region a fragment mapped to. Plugins
    /// emit handlers in block order; among equally good regions the first
    /// one at or after this line wins.
    cursor: usize,
}

/// A fragment line's place in the user's block.
#[derive(Debug, Clone, Copy, PartialEq)]
struct LineOrigin {
    content_line: usize,
    exact: bool,
}

impl FragmentOrigin {
    /// Origin for code the plugin `plugin_name` emits while expanding
    /// `block`. Without a block, or when the block is not in a user file,
    /// fragment code is marked as the plugin's.
    pub(crate) fn new(plugin_name: &str, block: Option<&FrameworkBlock>) -> Self {
        Self {
            plugin_marker: format!(
                "{}{}{}",
                crate::ast::PLUGIN_ORIGIN_PREFIX,
                plugin_name,
                crate::ast::PLUGIN_ORIGIN_SUFFIX
            ),
            block: block.and_then(BlockText::from_block),
            cursor: 0,
        }
    }

    /// Move the locations of `stmt`, parsed from `fragment`, to where they
    /// came from (see the module docs).
    pub(crate) fn relocate_statement(&mut self, fragment: &str, stmt: &mut Statement) {
        let lines = self.map_fragment(fragment);
        stmt.for_each_location_mut(&mut |loc| self.relocate(fragment, &lines, loc));
    }

    /// [`Self::relocate_statement`] for an expression fragment.
    pub(crate) fn relocate_expression(&mut self, fragment: &str, expr: &mut Expression) {
        let lines = self.map_fragment(fragment);
        expr.for_each_location_mut(&mut |loc| self.relocate(fragment, &lines, loc));
    }

    fn relocate(&self, fragment: &str, lines: &[Option<LineOrigin>], loc: &mut SourceLocation) {
        if !loc.file.starts_with(FRAGMENT_FILE_PREFIX) {
            return;
        }
        let origin = loc
            .line
            .checked_sub(1)
            .and_then(|index| lines.get(index).copied().flatten());
        match (origin, &self.block) {
            (Some(origin), Some(block)) => {
                let fragment_line = fragment.split('\n').nth(loc.line - 1).unwrap_or("");
                let content_line = &block.lines[origin.content_line];
                let content_indent = leading_whitespace(content_line);
                let content_column = if origin.exact {
                    // Same text: keep the offset into it.
                    let offset = loc
                        .column
                        .saturating_sub(1 + leading_whitespace(fragment_line));
                    content_indent + 1 + offset
                } else {
                    content_indent + 1
                };
                *loc = SourceLocation {
                    file: block.file.clone(),
                    line: block.first_line + origin.content_line,
                    column: block.first_column + content_column - 1,
                    byte_start: None,
                    byte_end: None,
                };
            }
            _ => {
                *loc = SourceLocation {
                    file: self.plugin_marker.clone(),
                    line: 0,
                    column: 0,
                    byte_start: None,
                    byte_end: None,
                };
            }
        }
    }

    /// The block content line each fragment line (by index) came from.
    fn map_fragment(&mut self, fragment: &str) -> Vec<Option<LineOrigin>> {
        let fragment_lines: Vec<&str> = fragment.split('\n').collect();
        let mut result = vec![None; fragment_lines.len()];
        let Some(block) = &self.block else {
            return result;
        };

        // Candidate pairs (fragment line, content line, score).
        let mut pairs: Vec<(usize, usize, usize)> = Vec::new();
        for (j, line) in fragment_lines.iter().enumerate() {
            let text = line.trim();
            if text.is_empty() {
                continue;
            }
            if let Some(found) = block.by_text.get(text) {
                pairs.extend(found.iter().map(|&i| (j, i, EXACT)));
                continue;
            }
            let fragment_words = words(text);
            for (i, content_words) in block.words.iter().enumerate() {
                if content_words.len() >= 2 && is_subsequence(content_words, &fragment_words) {
                    pairs.push((j, i, REWRITE));
                }
            }
        }
        if pairs.is_empty() {
            return result;
        }
        pairs.sort_by_key(|&(j, i, _)| (i, j));

        // Best chain increasing in both fragment and content line, by score,
        // then by the smallest content span. For chains ending at the same
        // pair, the smallest span is the latest first line, so each pair
        // keeps the best (score, first) over the chains it can extend: a
        // prefix maximum over fragment lines, taken over the pairs with a
        // smaller content line.
        let mut chains: Vec<Chain> = Vec::with_capacity(pairs.len());
        let mut best_before = PrefixMax::new(fragment_lines.len());
        let mut group_start = 0;
        while group_start < pairs.len() {
            let content_line = pairs[group_start].1;
            let group_end = pairs[group_start..]
                .iter()
                .position(|&(_, i, _)| i != content_line)
                .map_or(pairs.len(), |n| group_start + n);
            for &(j, i, score) in &pairs[group_start..group_end] {
                chains.push(match best_before.query(j) {
                    Some((previous_score, first, q)) => Chain {
                        score: previous_score + score,
                        first,
                        previous: Some(q),
                    },
                    None => Chain {
                        score,
                        first: i,
                        previous: None,
                    },
                });
            }
            for (p, &(j, _, _)) in pairs.iter().enumerate().take(group_end).skip(group_start) {
                best_before.update(j, (chains[p].score, chains[p].first, p));
            }
            group_start = group_end;
        }

        let span = |p: usize| pairs[p].1 - chains[p].first;
        let mut end = None::<usize>;
        for p in 0..chains.len() {
            end = Some(match end {
                None => p,
                Some(e) => {
                    let (chain, current) = (&chains[p], &chains[e]);
                    let better = chain.score > current.score
                        || (chain.score == current.score && span(p) < span(e));
                    let tie = chain.score == current.score && span(p) == span(e);
                    // On a tie, the region at or after the cursor wins.
                    if better || (tie && current.first < self.cursor && chain.first >= self.cursor)
                    {
                        p
                    } else {
                        e
                    }
                }
            });
        }
        let mut at = end;
        if let Some(e) = end {
            self.cursor = pairs[e].1 + 1;
        }
        while let Some(p) = at {
            let (j, i, score) = pairs[p];
            result[j] = Some(LineOrigin {
                content_line: i,
                exact: score == EXACT,
            });
            at = chains[p].previous;
        }
        result
    }
}

#[derive(Clone, Copy)]
struct Chain {
    score: usize,
    /// Content line the chain starts at.
    first: usize,
    previous: Option<usize>,
}

/// Prefix maximum over fragment lines (a Fenwick tree) of
/// `(score, first content line, pair index)`.
struct PrefixMax {
    tree: Vec<Option<(usize, usize, usize)>>,
}

impl PrefixMax {
    fn new(len: usize) -> Self {
        Self {
            tree: vec![None; len + 1],
        }
    }

    /// Best entry over fragment lines `0..j`.
    fn query(&self, j: usize) -> Option<(usize, usize, usize)> {
        let mut best = None;
        let mut k = j;
        while k > 0 {
            best = best.max(self.tree[k]);
            k &= k - 1;
        }
        best
    }

    fn update(&mut self, j: usize, value: (usize, usize, usize)) {
        let mut k = j + 1;
        while k < self.tree.len() {
            if self.tree[k] < Some(value) {
                self.tree[k] = Some(value);
            }
            k += 1 << k.trailing_zeros();
        }
    }
}

impl BlockText {
    fn from_block(block: &FrameworkBlock) -> Option<Self> {
        // Only content the parser took from a source file has source lines;
        // blocks the compiler builds (ORM queries with a binding header)
        // carry no body location.
        let body = block.body_location.as_ref()?;
        let file = block
            .location
            .as_ref()
            .map_or(&body.file, |header| &header.file);
        if file.is_empty() || file.starts_with('<') || body.line == 0 {
            return None;
        }
        let (first_line, first_column) = (body.line, body.column.max(1));
        let lines: Vec<String> = block.content.split('\n').map(str::to_string).collect();
        let mut by_text: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, line) in lines.iter().enumerate() {
            let text = line.trim();
            if !text.is_empty() {
                by_text.entry(text.to_string()).or_default().push(i);
            }
        }
        let words = lines.iter().map(|line| words(line)).collect();
        Some(Self {
            file: file.clone(),
            first_line,
            first_column,
            lines,
            by_text,
            words,
        })
    }
}

fn leading_whitespace(line: &str) -> usize {
    line.chars().take_while(|c| c.is_whitespace()).count()
}

/// The identifier and number words of a line.
fn words(line: &str) -> Vec<String> {
    line.split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

fn is_subsequence(needle: &[String], haystack: &[String]) -> bool {
    let mut rest = haystack.iter();
    needle.iter().all(|word| rest.any(|w| w == word))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(content: &str) -> FrameworkBlock {
        FrameworkBlock {
            name: "endpoints".to_string(),
            content: content.to_string(),
            attributes: Vec::new(),
            location: Some(SourceLocation {
                file: "app/api.cln".to_string(),
                line: 3,
                column: 1,
                byte_start: None,
                byte_end: None,
            }),
            body_location: Some(SourceLocation {
                file: "app/api.cln".to_string(),
                line: 5,
                column: 2,
                byte_start: None,
                byte_end: None,
            }),
        }
    }

    fn fragment_location(line: usize, column: usize) -> SourceLocation {
        SourceLocation {
            file: "<plugin-fragment>".to_string(),
            line,
            column,
            byte_start: None,
            byte_end: None,
        }
    }

    fn place(
        origin: &mut FragmentOrigin,
        fragment: &str,
        line: usize,
        column: usize,
    ) -> (String, usize, usize) {
        let mut stmt = Statement::Break {
            location: Some(fragment_location(line, column)),
        };
        origin.relocate_statement(fragment, &mut stmt);
        let loc = stmt.location().unwrap();
        (loc.file.clone(), loc.line, loc.column)
    }

    const CONTENT: &str = "GET \"/a\":\n\tinteger a = 1\n\tinteger b = first(a)\n\treturn json(b)\n\nGET \"/b\":\n\tinteger a = 1\n\tinteger c = second(a)\n\treturn json(c)";

    #[test]
    fn exact_lines_keep_line_and_column() {
        let mut origin = FragmentOrigin::new("frame.server", Some(&block(CONTENT)));
        let fragment = "integer a = 1\ninteger b = first(a)\nreturn json(b)";
        // `first` is column 13 of fragment line 2; content line 2 is
        // `\tinteger b = first(a)`, source line 7, the content starts at
        // source column 2.
        assert_eq!(
            place(&mut origin, fragment, 2, 13),
            ("app/api.cln".to_string(), 7, 15)
        );
    }

    #[test]
    fn shared_first_line_maps_to_the_handler_it_belongs_to() {
        let mut origin = FragmentOrigin::new("frame.server", Some(&block(CONTENT)));
        let fragment = "integer a = 1\ninteger c = second(a)\nreturn json(c)";
        assert_eq!(place(&mut origin, fragment, 1, 1).1, 11);
        assert_eq!(place(&mut origin, fragment, 2, 1).1, 12);
    }

    #[test]
    fn identical_handlers_map_in_order() {
        let content = "GET \"/a\":\n\treturn json(1)\n\nGET \"/b\":\n\treturn json(1)";
        let mut origin = FragmentOrigin::new("frame.server", Some(&block(content)));
        assert_eq!(place(&mut origin, "return json(1)", 1, 1).1, 6);
        assert_eq!(place(&mut origin, "return json(1)", 1, 1).1, 9);
    }

    #[test]
    fn rewritten_line_keeps_its_line() {
        let mut origin = FragmentOrigin::new("frame.server", Some(&block(CONTENT)));
        let fragment = "integer a = 1\ninteger b = first(a)\nreturn __wrap(json(b))";
        assert_eq!(
            place(&mut origin, fragment, 3, 1),
            ("app/api.cln".to_string(), 8, 3)
        );
    }

    #[test]
    fn plugin_written_lines_are_the_plugins() {
        let mut origin = FragmentOrigin::new("frame.server", Some(&block(CONTENT)));
        let fragment = "string __route = req.path()\ninteger a = 1";
        assert_eq!(
            place(&mut origin, fragment, 1, 1),
            ("<plugin:frame.server>".to_string(), 0, 0)
        );
    }

    #[test]
    fn without_a_user_block_fragment_code_is_the_plugins() {
        let mut origin = FragmentOrigin::new("frame.data", None);
        assert_eq!(
            place(&mut origin, "integer a = 1", 1, 1),
            ("<plugin:frame.data>".to_string(), 0, 0)
        );
    }

    #[test]
    fn real_file_locations_are_left_alone() {
        let mut origin = FragmentOrigin::new("frame.server", Some(&block(CONTENT)));
        let mut stmt = Statement::Break {
            location: Some(SourceLocation {
                file: "app/other.cln".to_string(),
                line: 40,
                column: 3,
                byte_start: None,
                byte_end: None,
            }),
        };
        origin.relocate_statement("integer a = 1", &mut stmt);
        assert_eq!(stmt.location().unwrap().line, 40);
    }
}

#![allow(clippy::uninlined_format_args)]

use clean_language_compiler::parser::{CleanParser, Rule};
use pest::Parser;
use std::env;
use std::fs;

fn print_pairs(pair: pest::iterators::Pair<Rule>, indent: usize) {
    let indent_str = "  ".repeat(indent);
    println!(
        "{}Rule::{:?}: \"{}\"",
        indent_str,
        pair.as_rule(),
        pair.as_str()
    );

    for inner_pair in pair.into_inner() {
        print_pairs(inner_pair, indent + 1);
    }
}

fn test_rule(rule: Rule, input: &str, rule_name: &str) {
    println!("\n=== Testing {rule_name} ===");
    println!("Input: {:?}", input);

    match CleanParser::parse(rule, input) {
        Ok(pairs) => {
            println!("SUCCESS!");
            for pair in pairs {
                print_pairs(pair, 0);
            }
        }
        Err(e) => {
            println!("FAILED: {e}");
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        eprintln!("Usage: {program} <file>", program = args[0]);
        std::process::exit(1);
    }

    let source =
        fs::read_to_string(&args[1]).unwrap_or_else(|e| panic!("Failed to read file: {}", e));

    println!("Source code: {source}");
    println!("Hex dump:");
    for (i, byte) in source.bytes().enumerate() {
        print!("{:02x} ", byte);
        if (i + 1) % 16 == 0 {
            println!();
        }
    }
    println!();

    // Test basic elements first
    test_rule(Rule::integer, "42", "integer");
    test_rule(Rule::statement, "42", "statement");

    // Test NEWLINE and INDENT specifically (these are internal rules, may not be directly testable)
    // Let's test parts of indented_block manually

    // Test the exact sequence we expect
    let test_seq = "\n\t42";
    println!("\n=== Manual sequence analysis ===");
    println!("Testing sequence: {:?}", test_seq);

    // Try different combinations
    test_rule(Rule::indented_block, test_seq, "indented_block");

    // Try without the statement part
    println!("\n=== Testing partial sequences ===");

    // Test if the issue is the combination of elements
    test_rule(Rule::statement, "return 42", "return statement");
    test_rule(Rule::return_stmt, "return 42", "return_stmt");

    // Test the full sequence from our file
    test_rule(Rule::start_function, &source, "start_function");
    test_rule(Rule::program, &source, "program");
}

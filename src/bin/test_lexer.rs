use clean_language_compiler::lexer::{CleanLexer, Lexer};
use std::env;
use std::fs;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 2 {
        println!("Usage: {} <clean_file>", args[0]);
        return;
    }

    let filename = &args[1];
    let content = match fs::read_to_string(filename) {
        Ok(content) => content,
        Err(err) => {
            eprintln!("Error reading file {}: {}", filename, err);
            return;
        }
    };

    println!("🚀 Testing Clean Language Lexer");
    println!("📄 File: {}", filename);
    println!("📏 Length: {} characters", content.len());
    println!();

    let mut lexer = CleanLexer::new(&content);
    let mut token_count = 0;
    let mut error_count = 0;

    // Track token types for statistics
    let mut keywords = 0;
    let mut identifiers = 0;
    let mut literals = 0;
    let mut operators = 0;
    let mut punctuation = 0;
    let mut comments = 0;

    println!("🔍 Tokenizing...");
    println!("{:<6} {:<20} {:<15} {}", "Line", "Token", "Type", "Details");
    println!("{:-<60}", "");

    loop {
        match lexer.next_token() {
            Ok(token) => {
                token_count += 1;

                // Categorize token for statistics
                match &token.kind {
                    clean_language_compiler::lexer::TokenKind::Identifier(_) => identifiers += 1,
                    clean_language_compiler::lexer::TokenKind::Integer(_)
                    | clean_language_compiler::lexer::TokenKind::Number(_)
                    | clean_language_compiler::lexer::TokenKind::String(_)
                    | clean_language_compiler::lexer::TokenKind::Boolean(_) => literals += 1,
                    clean_language_compiler::lexer::TokenKind::Comment(_) => comments += 1,
                    clean_language_compiler::lexer::TokenKind::Plus
                    | clean_language_compiler::lexer::TokenKind::Minus
                    | clean_language_compiler::lexer::TokenKind::Multiply
                    | clean_language_compiler::lexer::TokenKind::Divide
                    | clean_language_compiler::lexer::TokenKind::Power
                    | clean_language_compiler::lexer::TokenKind::Modulo
                    | clean_language_compiler::lexer::TokenKind::Assign
                    | clean_language_compiler::lexer::TokenKind::Equal
                    | clean_language_compiler::lexer::TokenKind::NotEqual
                    | clean_language_compiler::lexer::TokenKind::Less
                    | clean_language_compiler::lexer::TokenKind::Greater
                    | clean_language_compiler::lexer::TokenKind::LessEqual
                    | clean_language_compiler::lexer::TokenKind::GreaterEqual => operators += 1,
                    clean_language_compiler::lexer::TokenKind::LeftParen
                    | clean_language_compiler::lexer::TokenKind::RightParen
                    | clean_language_compiler::lexer::TokenKind::LeftBrace
                    | clean_language_compiler::lexer::TokenKind::RightBrace
                    | clean_language_compiler::lexer::TokenKind::LeftBracket
                    | clean_language_compiler::lexer::TokenKind::RightBracket
                    | clean_language_compiler::lexer::TokenKind::Comma
                    | clean_language_compiler::lexer::TokenKind::Dot
                    | clean_language_compiler::lexer::TokenKind::Colon
                    | clean_language_compiler::lexer::TokenKind::Semicolon => punctuation += 1,
                    clean_language_compiler::lexer::TokenKind::Eof => break,
                    clean_language_compiler::lexer::TokenKind::Invalid(_) => error_count += 1,
                    _ => keywords += 1,
                }

                // Display token (limit output to prevent flooding)
                if token_count <= 100 {
                    println!(
                        "{:<6} {:<20} {:<15} {}",
                        token.span.start.line,
                        format!("{:.18}", format!("{:?}", token.kind)),
                        get_token_category(&token.kind),
                        if token_count == 100 { "..." } else { "" }
                    );
                }
            }
            Err(error) => {
                error_count += 1;
                eprintln!("❌ Lexer error: {}", error);
                if error_count > 10 {
                    eprintln!("❌ Too many errors, stopping...");
                    break;
                }
            }
        }
    }

    // Print statistics
    println!();
    println!("📊 Tokenization Statistics:");
    println!("  📝 Total tokens: {}", token_count);
    println!("  🔤 Keywords: {}", keywords);
    println!("  🏷️  Identifiers: {}", identifiers);
    println!("  💎 Literals: {}", literals);
    println!("  ⚡ Operators: {}", operators);
    println!("  🔣 Punctuation: {}", punctuation);
    println!("  💬 Comments: {}", comments);
    println!("  ❌ Errors: {}", error_count);

    if error_count == 0 {
        println!();
        println!("✅ Lexer test completed successfully!");
    } else {
        println!();
        println!("⚠️  Lexer test completed with {} errors", error_count);
    }
}

fn get_token_category(token: &clean_language_compiler::lexer::TokenKind) -> &'static str {
    match token {
        clean_language_compiler::lexer::TokenKind::Identifier(_) => "Identifier",
        clean_language_compiler::lexer::TokenKind::Integer(_)
        | clean_language_compiler::lexer::TokenKind::Number(_)
        | clean_language_compiler::lexer::TokenKind::String(_)
        | clean_language_compiler::lexer::TokenKind::Boolean(_) => "Literal",
        clean_language_compiler::lexer::TokenKind::Comment(_) => "Comment",
        clean_language_compiler::lexer::TokenKind::Eof => "EOF",
        clean_language_compiler::lexer::TokenKind::Invalid(_) => "Invalid",
        clean_language_compiler::lexer::TokenKind::Newline
        | clean_language_compiler::lexer::TokenKind::Tab => "Whitespace",
        _ => "Keyword/Operator",
    }
}

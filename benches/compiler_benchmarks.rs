//! Compiler Performance Benchmarks
//!
//! Run with: cargo bench
//!
//! These benchmarks measure the performance of various compiler stages:
//! - Lexing: Token generation from source code
//! - Parsing: AST construction from tokens
//! - Type checking: Type inference and validation
//! - Full compilation: End-to-end WASM generation

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};

// Sample programs of varying complexity
const SIMPLE_PROGRAM: &str = r#"
start()
	integer x = 10
	integer y = 20
	integer result = x + y
	println(result)
"#;

const FUNCTION_PROGRAM: &str = r#"
functions:
	integer add(integer a, integer b)
		return a + b

	integer multiply(integer a, integer b)
		return a * b

	integer factorial(integer n)
		if n <= 1
			return 1
		return n * factorial(n - 1)

start()
	integer sum = add(10, 20)
	integer product = multiply(5, 6)
	integer fact = factorial(5)
	println(sum)
	println(product)
	println(fact)
"#;

const CLASS_PROGRAM: &str = r#"
class Point
	number x
	number y

	constructor(number x, number y)
		this.x = x
		this.y = y

	number distance()
		return sqrt(this.x * this.x + this.y * this.y)

	Point add(Point other)
		return Point(this.x + other.x, this.y + other.y)

start()
	Point p1 = Point(3.0, 4.0)
	Point p2 = Point(1.0, 2.0)
	number dist = p1.distance()
	Point p3 = p1.add(p2)
	println(dist)
"#;

const COMPLEX_PROGRAM: &str = r#"
class Vector
	list<number> elements

	constructor(list<number> elements)
		this.elements = elements

	integer length()
		return this.elements.length()

	number sum()
		number total = 0.0
		for element in this.elements
			total = total + element
		return total

	number average()
		return this.sum() / this.length()

functions:
	list<integer> range(integer start, integer end)
		list<integer> result = []
		integer i = start
		while i < end
			result.push(i)
			i = i + 1
		return result

	integer sumList(list<integer> numbers)
		integer total = 0
		for num in numbers
			total = total + num
		return total

	boolean isPrime(integer n)
		if n < 2
			return false
		integer i = 2
		while i * i <= n
			if n % i == 0
				return false
			i = i + 1
		return true

start()
	list<integer> numbers = range(1, 100)
	integer sum = sumList(numbers)
	println(sum)

	integer primeCount = 0
	for n in numbers
		if isPrime(n)
			primeCount = primeCount + 1
	println(primeCount)
"#;

fn bench_lexing(c: &mut Criterion) {
    use clean_language_compiler::lexer::{SourceCode, SpecificationLexer};

    let mut group = c.benchmark_group("lexing");

    let programs = [
        ("simple", SIMPLE_PROGRAM),
        ("functions", FUNCTION_PROGRAM),
        ("class", CLASS_PROGRAM),
        ("complex", COMPLEX_PROGRAM),
    ];

    for (name, source) in &programs {
        group.throughput(Throughput::Bytes(source.len() as u64));
        group.bench_with_input(BenchmarkId::new("lex", name), source, |b, source| {
            b.iter(|| {
                let source_code = SourceCode::new(source.to_string(), "benchmark.cln".to_string());
                let mut lexer = SpecificationLexer::new(&source_code);
                let tokens = lexer.tokenize();
                black_box(tokens)
            });
        });
    }

    group.finish();
}

fn bench_parsing(c: &mut Criterion) {
    use clean_language_compiler::parser::CleanParser;

    let mut group = c.benchmark_group("parsing");

    let programs = [
        ("simple", SIMPLE_PROGRAM),
        ("functions", FUNCTION_PROGRAM),
        ("class", CLASS_PROGRAM),
        ("complex", COMPLEX_PROGRAM),
    ];

    for (name, source) in &programs {
        group.throughput(Throughput::Bytes(source.len() as u64));
        group.bench_with_input(BenchmarkId::new("parse", name), source, |b, source| {
            b.iter(|| {
                let result = CleanParser::parse_program(source);
                black_box(result)
            });
        });
    }

    group.finish();
}

fn bench_full_compilation(c: &mut Criterion) {
    use clean_language_compiler::compile;

    let mut group = c.benchmark_group("full_compilation");
    group.sample_size(20); // Full compilation is slower, reduce samples

    let programs = [
        ("simple", SIMPLE_PROGRAM),
        ("functions", FUNCTION_PROGRAM),
        ("class", CLASS_PROGRAM),
    ];

    for (name, source) in &programs {
        group.throughput(Throughput::Bytes(source.len() as u64));
        group.bench_with_input(BenchmarkId::new("compile", name), source, |b, source| {
            b.iter(|| {
                let result = compile(source);
                black_box(result)
            });
        });
    }

    group.finish();
}

fn bench_optimization_levels(c: &mut Criterion) {
    use clean_language_compiler::compile_with_opt_level;

    let mut group = c.benchmark_group("optimization_levels");
    group.sample_size(20);

    let source = FUNCTION_PROGRAM;

    for opt_level in 0..=3 {
        group.bench_with_input(
            BenchmarkId::new("opt_level", opt_level),
            &opt_level,
            |b, &level| {
                b.iter(|| {
                    let result = compile_with_opt_level(source, "benchmark.cln", level);
                    black_box(result)
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_lexing,
    bench_parsing,
    bench_full_compilation,
    bench_optimization_levels,
);

criterion_main!(benches);

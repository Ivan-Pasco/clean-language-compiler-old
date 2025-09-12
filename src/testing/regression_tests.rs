use crate::error::CompilerError;
use crate::testing::{TestSuite, TestCaseBuilder, BaselineMetrics};

/// Regression tests for detecting performance and functionality regressions
pub struct RegressionTests;

impl RegressionTests {
    /// Create regression test suite
    pub fn create_suite() -> Result<TestSuite, CompilerError> {
        let mut suite = TestSuite::new("regression_tests", "Regression detection tests");

        let test_cases = vec![
            Self::compilation_time_regression()?,
            Self::binary_size_regression()?,
            Self::memory_usage_regression()?,
            Self::optimization_regression()?,
            Self::parsing_accuracy_regression()?,
            Self::semantic_analysis_regression()?,
        ];

        for test_case in test_cases {
            suite.add_test(test_case);
        }

        Ok(suite)
    }

    fn compilation_time_regression() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("compilation_time_regression")
            .description("Detect compilation time regressions")
            .regression_test(
                r#"
                // Standard test program for compilation time baseline
                function fibonacci(n: integer) -> integer {
                    if (n <= 1) return n;
                    return fibonacci(n - 1) + fibonacci(n - 2);
                }

                class Calculator {
                    constructor() {
                        this.history = [];
                    }

                    add(a: number, b: number) -> number {
                        let result = a + b;
                        this.history.push({operation: "add", args: [a, b], result: result});
                        return result;
                    }

                    multiply(a: number, b: number) -> number {
                        let result = a * b;
                        this.history.push({operation: "multiply", args: [a, b], result: result});
                        return result;
                    }

                    getHistory() -> Array<Object> {
                        return this.history;
                    }
                }

                function main() {
                    let calc = new Calculator();
                    let fib10 = fibonacci(10);
                    let sum = calc.add(fib10, 25);
                    let product = calc.multiply(sum, 2);
                    print("Result: " + product);
                    print("History entries: " + calc.getHistory().length);
                }
                "#,
                BaselineMetrics {
                    compilation_time_ms: 800, // Baseline: 800ms
                    execution_time_ms: 50,
                    memory_usage_mb: 32,
                    binary_size: 15000, // 15KB
                    allowed_regression_factor: 1.2, // Allow 20% regression
                }
            )
            .tag("regression")
            .tag("compilation-time")
            .build()
    }

    fn binary_size_regression() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("binary_size_regression")
            .description("Detect binary size regressions")
            .regression_test(
                r#"
                // Standard program for binary size testing
                function simpleFunction() {
                    return "Hello, World!";
                }

                class SimpleClass {
                    constructor(value: string) {
                        this.value = value;
                    }

                    getValue() -> string {
                        return this.value;
                    }
                }

                function main() {
                    let message = simpleFunction();
                    let obj = new SimpleClass(message);
                    print(obj.getValue());
                }
                "#,
                BaselineMetrics {
                    compilation_time_ms: 300,
                    execution_time_ms: 20,
                    memory_usage_mb: 16,
                    binary_size: 8000, // Baseline: 8KB
                    allowed_regression_factor: 1.15, // Allow 15% size increase
                }
            )
            .tag("regression")
            .tag("binary-size")
            .build()
    }

    fn memory_usage_regression() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("memory_usage_regression")
            .description("Detect memory usage regressions")
            .regression_test(
                r#"
                // Memory-intensive program for baseline testing
                class DataContainer {
                    constructor() {
                        this.data = [];
                    }

                    addData(count: integer) {
                        for (let i = 0; i < count; i++) {
                            this.data.push({
                                id: i,
                                value: "item_" + i,
                                timestamp: Date.now(),
                                metadata: {
                                    type: "test",
                                    category: "regression",
                                    priority: i % 3
                                }
                            });
                        }
                    }

                    processData() -> integer {
                        let sum = 0;
                        for (let item of this.data) {
                            sum += item.id * item.metadata.priority;
                        }
                        return sum;
                    }

                    cleanup() {
                        this.data = [];
                    }
                }

                function main() {
                    let container = new DataContainer();
                    container.addData(1000);
                    let result = container.processData();
                    container.cleanup();
                    print("Processed result: " + result);
                }
                "#,
                BaselineMetrics {
                    compilation_time_ms: 1200,
                    execution_time_ms: 100,
                    memory_usage_mb: 64, // Baseline: 64MB peak
                    binary_size: 25000,
                    allowed_regression_factor: 1.25, // Allow 25% memory increase
                }
            )
            .tag("regression")
            .tag("memory")
            .build()
    }

    fn optimization_regression() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("optimization_regression")
            .description("Detect optimization effectiveness regressions")
            .regression_test(
                r#"
                // Program designed to test optimization effectiveness
                function constantFoldingTest() -> integer {
                    let a = 2 + 3; // Should be folded to 5
                    let b = a * 4; // Should be folded to 20
                    let c = b / 2; // Should be folded to 10
                    return c;
                }

                function deadCodeTest() -> integer {
                    let result = 42;
                    
                    if (false) {
                        // This should be eliminated
                        let unused = "never executed";
                        result = unused.length;
                    }
                    
                    return result;
                    
                    // This should also be eliminated
                    let afterReturn = "unreachable";
                    return afterReturn.length;
                }

                function loopOptimizationTest() -> integer {
                    let sum = 0;
                    let constant = 5; // Should be hoisted out of loop
                    
                    for (let i = 0; i < 100; i++) {
                        sum += i * constant; // constant should be loop-invariant
                    }
                    
                    return sum;
                }

                function inliningTest() -> integer {
                    function smallFunction(x: integer) -> integer {
                        return x * 2; // Should be inlined
                    }
                    
                    return smallFunction(21) + smallFunction(21);
                }

                function main() {
                    let result1 = constantFoldingTest();
                    let result2 = deadCodeTest();
                    let result3 = loopOptimizationTest();
                    let result4 = inliningTest();
                    
                    print("Results: " + result1 + ", " + result2 + ", " + result3 + ", " + result4);
                }
                "#,
                BaselineMetrics {
                    compilation_time_ms: 900,
                    execution_time_ms: 30,
                    memory_usage_mb: 24,
                    binary_size: 12000, // Should be smaller due to optimizations
                    allowed_regression_factor: 1.1, // Stricter for optimization tests
                }
            )
            .tag("regression")
            .tag("optimization")
            .build()
    }

    fn parsing_accuracy_regression() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("parsing_accuracy_regression")
            .description("Detect parsing accuracy regressions")
            .regression_test(
                r#"
                // Complex syntax program to test parser robustness
                class GenericContainer<T, U extends Comparable<T>> {
                    private items: Map<T, Array<U>>;

                    constructor() {
                        this.items = new Map<T, Array<U>>();
                    }

                    add(key: T, value: U) {
                        if (!this.items.has(key)) {
                            this.items.set(key, []);
                        }
                        this.items.get(key).push(value);
                    }

                    process<V>(processor: (item: U) -> V) -> Map<T, Array<V>> {
                        let result = new Map<T, Array<V>>();
                        
                        for (let [key, values] of this.items) {
                            let processed = values.map((value: U) => processor(value));
                            result.set(key, processed);
                        }
                        
                        return result;
                    }

                    async processAsync<V>(processor: (item: U) -> Promise<V>) -> Promise<Map<T, Array<V>>> {
                        let result = new Map<T, Array<V>>();
                        
                        for (let [key, values] of this.items) {
                            let processed = await Promise.all(
                                values.map(async (value: U) => await processor(value))
                            );
                            result.set(key, processed);
                        }
                        
                        return result;
                    }
                }

                interface Comparable<T> {
                    compareTo(other: T) -> number;
                }

                class StringWrapper implements Comparable<StringWrapper> {
                    constructor(private value: string) {}

                    compareTo(other: StringWrapper) -> number {
                        return this.value.localeCompare(other.value);
                    }

                    toString() -> string {
                        return this.value;
                    }
                }

                async function main() {
                    let container = new GenericContainer<string, StringWrapper>();
                    
                    container.add("group1", new StringWrapper("item1"));
                    container.add("group1", new StringWrapper("item2"));
                    container.add("group2", new StringWrapper("item3"));
                    
                    let processed = container.process<string>(item => item.toString().toUpperCase());
                    
                    for (let [key, values] of processed) {
                        print("Group " + key + ": " + values.join(", "));
                    }
                    
                    let asyncProcessed = await container.processAsync<string>(
                        async (item) => {
                            // Simulate async operation
                            await new Promise(resolve => setTimeout(resolve, 1));
                            return item.toString().toLowerCase();
                        }
                    );
                    
                    print("Async processing completed");
                }
                "#,
                BaselineMetrics {
                    compilation_time_ms: 1500, // Complex parsing takes longer
                    execution_time_ms: 150,
                    memory_usage_mb: 48,
                    binary_size: 35000,
                    allowed_regression_factor: 1.15, // Allow some variance for complex parsing
                }
            )
            .tag("regression")
            .tag("parsing")
            .tag("complex-syntax")
            .build()
    }

    fn semantic_analysis_regression() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("semantic_analysis_regression")
            .description("Detect semantic analysis regressions")
            .regression_test(
                r#"
                // Program with complex type relationships for semantic analysis
                abstract class Shape {
                    protected abstract calculateArea() -> number;
                    
                    public getAreaDescription() -> string {
                        return "Area: " + this.calculateArea();
                    }
                }

                class Rectangle extends Shape {
                    constructor(private width: number, private height: number) {
                        base();
                    }

                    protected calculateArea() -> number {
                        return this.width * this.height;
                    }

                    public getPerimeter() -> number {
                        return 2 * (this.width + this.height);
                    }
                }

                class Circle extends Shape {
                    constructor(private radius: number) {
                        base();
                    }

                    protected calculateArea() -> number {
                        return Math.PI * this.radius * this.radius;
                    }

                    public getCircumference() -> number {
                        return 2 * Math.PI * this.radius;
                    }
                }

                class ShapeProcessor<T extends Shape> {
                    private shapes: Array<T>;

                    constructor() {
                        this.shapes = [];
                    }

                    addShape(shape: T) {
                        this.shapes.push(shape);
                    }

                    getTotalArea() -> number {
                        return this.shapes.reduce(
                            (total, shape) => total + shape.calculateArea(), 
                            0
                        );
                    }

                    processShapes<U>(processor: (shape: T) -> U) -> Array<U> {
                        return this.shapes.map(processor);
                    }
                }

                type ShapeUnion = Rectangle | Circle;
                type ShapeArray = Array<Shape>;
                type ProcessorFunction<T> = (shape: T) -> string;

                function processShapeCollection(shapes: ShapeArray) -> Array<string> {
                    let processor = new ShapeProcessor<Shape>();
                    
                    for (let shape of shapes) {
                        processor.addShape(shape);
                    }
                    
                    return processor.processShapes<string>((shape: Shape) => {
                        if (shape instanceof Rectangle) {
                            return "Rectangle: " + shape.getAreaDescription() + 
                                   " (Perimeter: " + shape.getPerimeter() + ")";
                        } else if (shape instanceof Circle) {
                            return "Circle: " + shape.getAreaDescription() + 
                                   " (Circumference: " + shape.getCircumference() + ")";
                        } else {
                            return "Unknown shape: " + shape.getAreaDescription();
                        }
                    });
                }

                function main() {
                    let shapes: ShapeArray = [
                        new Rectangle(5, 3),
                        new Circle(2.5),
                        new Rectangle(4, 4),
                        new Circle(1.5)
                    ];
                    
                    let descriptions = processShapeCollection(shapes);
                    
                    for (let description of descriptions) {
                        print(description);
                    }
                }
                "#,
                BaselineMetrics {
                    compilation_time_ms: 2000, // Complex type analysis
                    execution_time_ms: 80,
                    memory_usage_mb: 40,
                    binary_size: 40000,
                    allowed_regression_factor: 1.2, // Allow some variance for complex analysis
                }
            )
            .tag("regression")
            .tag("semantic-analysis")
            .tag("type-checking")
            .build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_regression_suite_creation() {
        let suite = RegressionTests::create_suite().unwrap();
        assert_eq!(suite.name, "regression_tests");
        assert!(suite.test_count() > 0);
    }

    #[test]
    fn test_all_regression_tests_have_baselines() {
        let suite = RegressionTests::create_suite().unwrap();
        
        for test in &suite.test_cases {
            match &test.test_type {
                crate::testing::TestType::Regression { baseline_metrics, .. } => {
                    assert!(baseline_metrics.compilation_time_ms > 0);
                    assert!(baseline_metrics.binary_size > 0);
                    assert!(baseline_metrics.allowed_regression_factor >= 1.0);
                }
                _ => panic!("All regression tests should have regression test type"),
            }
        }
    }

    #[test]
    fn test_regression_factor_ranges() {
        let suite = RegressionTests::create_suite().unwrap();
        
        for test in &suite.test_cases {
            if let crate::testing::TestType::Regression { baseline_metrics, .. } = &test.test_type {
                // Regression factors should be reasonable (between 1.0 and 2.0)
                assert!(baseline_metrics.allowed_regression_factor >= 1.0);
                assert!(baseline_metrics.allowed_regression_factor <= 2.0);
            }
        }
    }
}
use crate::error::CompilerError;
use crate::testing::{TestSuite, TestCaseBuilder, PerformanceThresholds};
use std::time::Duration;

/// Performance and benchmark tests for the compiler
pub struct PerformanceTests;

impl PerformanceTests {
    /// Create performance test suite
    pub fn create_suite() -> Result<TestSuite, CompilerError> {
        let mut suite = TestSuite::new("performance_tests", "Performance and benchmark tests");
        suite.parallel_safe = false; // Performance tests should run sequentially

        let test_cases = vec![
            Self::small_program_compilation_time()?,
            Self::large_program_compilation_time()?,
            Self::memory_usage_test()?,
            Self::binary_size_test()?,
            Self::optimization_effectiveness()?,
            Self::parser_performance()?,
            Self::semantic_analysis_performance()?,
            Self::codegen_performance()?,
        ];

        for test_case in test_cases {
            suite.add_test(test_case);
        }

        Ok(suite)
    }

    fn small_program_compilation_time() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("small_program_compilation_time")
            .description("Measure compilation time for small programs")
            .performance_test(
                r#"
                function factorial(n: integer) -> integer {
                    if (n <= 1) return 1;
                    return n * factorial(n - 1);
                }

                function main() {
                    print(factorial(10));
                }
                "#,
                PerformanceThresholds {
                    max_compilation_time: Some(Duration::from_millis(500)),
                    max_execution_time: Some(Duration::from_millis(100)),
                    max_memory_mb: Some(50),
                    max_binary_size: Some(10 * 1024), // 10KB
                    min_throughput_ops_per_sec: None,
                }
            )
            .tag("performance")
            .tag("compilation-time")
            .tag("small-program")
            .build()
    }

    fn large_program_compilation_time() -> Result<crate::testing::TestCase, CompilerError> {
        let large_program = Self::generate_large_program(500); // 500 functions
        
        TestCaseBuilder::new("large_program_compilation_time")
            .description("Measure compilation time for large programs")
            .performance_test(
                &large_program,
                PerformanceThresholds {
                    max_compilation_time: Some(Duration::from_secs(10)),
                    max_execution_time: Some(Duration::from_secs(1)),
                    max_memory_mb: Some(200),
                    max_binary_size: Some(1024 * 1024), // 1MB
                    min_throughput_ops_per_sec: None,
                }
            )
            .tag("performance")
            .tag("compilation-time")
            .tag("large-program")
            .build()
    }

    fn memory_usage_test() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("memory_usage_test")
            .description("Test memory usage during compilation")
            .performance_test(
                r#"
                class DataStructure {
                    constructor() {
                        this.data = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
                        this.nested = {
                            a: "test",
                            b: 42,
                            c: [1, 2, 3]
                        };
                    }
                }

                function main() {
                    let objects = [];
                    for (let i = 0; i < 100; i++) {
                        objects.push(new DataStructure());
                    }
                    print("Created " + objects.length + " objects");
                }
                "#,
                PerformanceThresholds {
                    max_compilation_time: Some(Duration::from_secs(2)),
                    max_execution_time: Some(Duration::from_millis(500)),
                    max_memory_mb: Some(100),
                    max_binary_size: Some(50 * 1024), // 50KB
                    min_throughput_ops_per_sec: None,
                }
            )
            .tag("performance")
            .tag("memory")
            .build()
    }

    fn binary_size_test() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("binary_size_test")
            .description("Test generated binary size optimization")
            .performance_test(
                r#"
                function main() {
                    // Simple program should produce small binary
                    print("Hello, World!");
                }
                "#,
                PerformanceThresholds {
                    max_compilation_time: Some(Duration::from_millis(300)),
                    max_execution_time: Some(Duration::from_millis(50)),
                    max_memory_mb: Some(25),
                    max_binary_size: Some(5 * 1024), // 5KB for simple hello world
                    min_throughput_ops_per_sec: None,
                }
            )
            .tag("performance")
            .tag("binary-size")
            .build()
    }

    fn optimization_effectiveness() -> Result<crate::testing::TestCase, CompilerError> {
        TestCaseBuilder::new("optimization_effectiveness")
            .description("Test optimization effectiveness on performance")
            .performance_test(
                r#"
                function deadCode() {
                    let unused = 42;
                    return 0; // Dead code after this
                    let moreUnused = "never executed";
                }

                function constantFolding() {
                    let a = 2 + 3; // Should be folded to 5
                    let b = a * 4; // Should be folded to 20
                    return b;
                }

                function loopOptimization() {
                    let sum = 0;
                    for (let i = 0; i < 10; i++) {
                        sum += i;
                    }
                    return sum;
                }

                function main() {
                    print(constantFolding());
                    print(loopOptimization());
                }
                "#,
                PerformanceThresholds {
                    max_compilation_time: Some(Duration::from_millis(800)),
                    max_execution_time: Some(Duration::from_millis(100)),
                    max_memory_mb: Some(50),
                    max_binary_size: Some(15 * 1024), // Should be optimized
                    min_throughput_ops_per_sec: None,
                }
            )
            .tag("performance")
            .tag("optimization")
            .build()
    }

    fn parser_performance() -> Result<crate::testing::TestCase, CompilerError> {
        let complex_syntax = Self::generate_complex_syntax_program();
        
        TestCaseBuilder::new("parser_performance")
            .description("Test parser performance on complex syntax")
            .performance_test(
                &complex_syntax,
                PerformanceThresholds {
                    max_compilation_time: Some(Duration::from_secs(3)),
                    max_execution_time: None, // Focus on parsing only
                    max_memory_mb: Some(100),
                    max_binary_size: None,
                    min_throughput_ops_per_sec: None,
                }
            )
            .tag("performance")
            .tag("parser")
            .build()
    }

    fn semantic_analysis_performance() -> Result<crate::testing::TestCase, CompilerError> {
        let type_heavy_program = Self::generate_type_heavy_program();
        
        TestCaseBuilder::new("semantic_analysis_performance")
            .description("Test semantic analysis performance")
            .performance_test(
                &type_heavy_program,
                PerformanceThresholds {
                    max_compilation_time: Some(Duration::from_secs(5)),
                    max_execution_time: None,
                    max_memory_mb: Some(150),
                    max_binary_size: None,
                    min_throughput_ops_per_sec: None,
                }
            )
            .tag("performance")
            .tag("semantic")
            .build()
    }

    fn codegen_performance() -> Result<crate::testing::TestCase, CompilerError> {
        let computation_heavy = Self::generate_computation_heavy_program();
        
        TestCaseBuilder::new("codegen_performance")
            .description("Test code generation performance")
            .performance_test(
                &computation_heavy,
                PerformanceThresholds {
                    max_compilation_time: Some(Duration::from_secs(7)),
                    max_execution_time: Some(Duration::from_secs(2)),
                    max_memory_mb: Some(100),
                    max_binary_size: Some(100 * 1024), // 100KB
                    min_throughput_ops_per_sec: None,
                }
            )
            .tag("performance")
            .tag("codegen")
            .build()
    }

    /// Generate a large program for performance testing
    fn generate_large_program(function_count: usize) -> String {
        let mut program = String::new();
        
        // Generate many functions
        for i in 0..function_count {
            program.push_str(&format!(
                r#"
                function func_{}(x: integer) -> integer {{
                    let result = x * {} + {};
                    if (result > 100) {{
                        return result / 2;
                    }} else {{
                        return result * 3;
                    }}
                }}
                "#,
                i, i + 1, i * 2
            ));
        }

        // Main function that calls some of them
        program.push_str(
            r#"
            function main() {
                let sum = 0;
                for (let i = 0; i < 50; i++) {
                    sum += func_0(i) + func_10(i) + func_25(i);
                }
                print(sum);
            }
            "#
        );

        program
    }

    /// Generate a program with complex syntax
    fn generate_complex_syntax_program() -> String {
        r#"
        class ComplexClass<T, U> extends BaseClass<T> implements Interface<U> {
            private field1: T;
            protected field2: U;
            public field3: Array<T>;

            constructor(param1: T, param2: U, param3: Array<T>) {
                base(param1);
                this.field1 = param1;
                this.field2 = param2;
                this.field3 = param3;
            }

            public method1<V>(param: V) -> Promise<Array<T | U | V>> {
                return new Promise<Array<T | U | V>>((resolve, reject) => {
                    try {
                        let result: Array<T | U | V> = [];
                        
                        for (let item of this.field3) {
                            if (item instanceof T && param instanceof V) {
                                result.push(this.processItem(item, param));
                            }
                        }
                        
                        resolve(result);
                    } catch (error) {
                        reject(error);
                    }
                });
            }

            private processItem<V>(item: T, processor: V) -> T | U | V {
                switch (typeof item) {
                    case "string":
                        return this.field2;
                    case "number":
                        return processor;
                    default:
                        return item;
                }
            }

            protected async asyncMethod(callback: (x: T) => Promise<U>) -> U {
                try {
                    let result = await callback(this.field1);
                    return result;
                } catch (error) {
                    throw new Error("Async operation failed: " + error.message);
                }
            }
        }

        interface Interface<T> {
            method1<V>(param: V) -> Promise<Array<T | V>>;
        }

        abstract class BaseClass<T> {
            abstract process(item: T) -> T;
        }

        function main() {
            let complex = new ComplexClass<string, number>(
                "test", 
                42, 
                ["a", "b", "c"]
            );
            
            let result = complex.method1<boolean>(true);
            print(result);
        }
        "#.to_string()
    }

    /// Generate a program with heavy type checking requirements
    fn generate_type_heavy_program() -> String {
        r#"
        type NumberOrString = number | string;
        type ProcessorFunction<T> = (input: T) -> T;
        type ComplexMap<K, V> = Map<K, V | Array<V>>;

        class TypeHeavyClass<T extends NumberOrString, U> {
            private processors: Map<string, ProcessorFunction<T>>;
            private data: ComplexMap<T, U>;

            constructor() {
                this.processors = new Map<string, ProcessorFunction<T>>();
                this.data = new Map<T, U | Array<U>>();
            }

            addProcessor<V extends T>(name: string, processor: ProcessorFunction<V>) {
                this.processors.set(name, processor as ProcessorFunction<T>);
            }

            process<V extends T>(input: V, processorName: string) -> V | null {
                let processor = this.processors.get(processorName);
                if (processor) {
                    return processor(input) as V;
                }
                return null;
            }

            getData<V extends U>(key: T) -> V | Array<V> | null {
                return this.data.get(key) as V | Array<V> | null;
            }
        }

        function createTypeHeavyInstance() -> TypeHeavyClass<string, number> {
            let instance = new TypeHeavyClass<string, number>();
            
            instance.addProcessor<string>("uppercase", (s: string) => s.toUpperCase());
            instance.addProcessor<string>("lowercase", (s: string) => s.toLowerCase());
            
            return instance;
        }

        function main() {
            let instance = createTypeHeavyInstance();
            let result = instance.process("Hello", "uppercase");
            print(result);
        }
        "#.to_string()
    }

    /// Generate a computation-heavy program
    fn generate_computation_heavy_program() -> String {
        r#"
        class Matrix {
            private data: Array<Array<number>>;
            private rows: number;
            private cols: number;

            constructor(rows: number, cols: number) {
                this.rows = rows;
                this.cols = cols;
                this.data = [];
                
                for (let i = 0; i < rows; i++) {
                    let row = [];
                    for (let j = 0; j < cols; j++) {
                        row.push(Math.random());
                    }
                    this.data.push(row);
                }
            }

            multiply(other: Matrix) -> Matrix {
                if (this.cols != other.rows) {
                    throw new Error("Matrix dimensions don't match");
                }

                let result = new Matrix(this.rows, other.cols);
                
                for (let i = 0; i < this.rows; i++) {
                    for (let j = 0; j < other.cols; j++) {
                        let sum = 0;
                        for (let k = 0; k < this.cols; k++) {
                            sum += this.data[i][k] * other.data[k][j];
                        }
                        result.data[i][j] = sum;
                    }
                }
                
                return result;
            }

            transpose() -> Matrix {
                let result = new Matrix(this.cols, this.rows);
                
                for (let i = 0; i < this.rows; i++) {
                    for (let j = 0; j < this.cols; j++) {
                        result.data[j][i] = this.data[i][j];
                    }
                }
                
                return result;
            }

            determinant() -> number {
                if (this.rows != this.cols) {
                    throw new Error("Matrix must be square");
                }
                
                return this.calculateDeterminant(this.data);
            }

            private calculateDeterminant(matrix: Array<Array<number>>) -> number {
                let n = matrix.length;
                if (n == 1) return matrix[0][0];
                if (n == 2) return matrix[0][0] * matrix[1][1] - matrix[0][1] * matrix[1][0];
                
                let det = 0;
                for (let j = 0; j < n; j++) {
                    let minor = this.getMinor(matrix, 0, j);
                    let cofactor = matrix[0][j] * this.calculateDeterminant(minor);
                    if (j % 2 == 1) cofactor = -cofactor;
                    det += cofactor;
                }
                
                return det;
            }

            private getMinor(matrix: Array<Array<number>>, row: number, col: number) -> Array<Array<number>> {
                let minor = [];
                for (let i = 0; i < matrix.length; i++) {
                    if (i == row) continue;
                    let minorRow = [];
                    for (let j = 0; j < matrix[i].length; j++) {
                        if (j == col) continue;
                        minorRow.push(matrix[i][j]);
                    }
                    minor.push(minorRow);
                }
                return minor;
            }
        }

        function performMatrixOperations() {
            let a = new Matrix(10, 10);
            let b = new Matrix(10, 10);
            
            let product = a.multiply(b);
            let transpose = product.transpose();
            let determinant = transpose.determinant();
            
            print("Matrix operations completed. Determinant: " + determinant);
        }

        function main() {
            for (let i = 0; i < 5; i++) {
                performMatrixOperations();
            }
        }
        "#.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_performance_suite_creation() {
        let suite = PerformanceTests::create_suite().unwrap();
        assert_eq!(suite.name, "performance_tests");
        assert!(suite.test_count() > 0);
        assert!(!suite.parallel_safe); // Performance tests should run sequentially
    }

    #[test]
    fn test_large_program_generation() {
        let program = PerformanceTests::generate_large_program(10);
        assert!(program.contains("func_0"));
        assert!(program.contains("func_9"));
        assert!(program.contains("function main()"));
    }
}
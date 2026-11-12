//! A multi-file program compiles to the same bytes on every build (#28).
//!
//! Single-file compiles were already reproducible; `cln build` of a
//! multi-file app emitted its functions in a different order from run to
//! run. Every `HashMap` gets its own random hash keys, so compiling the same
//! project several times in one process exercises every hash-map iteration
//! that could leak into the output.

use clean_language_compiler::MemoryTier;
use std::fs;

const MAIN: &str = "\
import:
\tshapes
\tgeometry
\tcounters

start:
\tSquare s = Square(3)
\tprint(s.area())
\tprint(perimeter(s))
\tprint(total([1, 2, 3]))
\tprint(next())
";

const SHAPES: &str = "\
class Square
\tpublic:
\t\tinteger side

\tconstructor(integer sideParam)
\t\tside = sideParam

\tfunctions:
\t\tpublic:
\t\t\tinteger area()
\t\t\t\treturn side * side

class Circle
\tpublic:
\t\tnumber radius

\tconstructor(number radiusParam)
\t\tradius = radiusParam
";

const GEOMETRY: &str = "\
import:
\tshapes
\tutils

functions:
\t// Changed in 0.33.184: an imported module function outside public: (perimeter, total) is private, SEM005 per developer decision
\tpublic:
\t\tinteger perimeter(Square s)
\t\t\treturn times(s.side, 4)

\t\tinteger total(list<integer> xs)
\t\t\tinteger sum = 0
\t\t\titerate x in xs
\t\t\t\tsum = sum + x
\t\t\treturn sum
";

const UTILS: &str = "\
functions:
\t// Changed in 0.33.184: an imported module function outside public: (times, plus) is private, SEM005 per developer decision
\tpublic:
\t\tinteger times(integer a, integer b)
\t\t\treturn a * b

\t\tinteger plus(integer a, integer b)
\t\t\treturn a + b

\tstring label(integer n)
\t\treturn \"n=\" + n.toString()
";

const COUNTERS: &str = "\
import:
\tutils

start:
\tinteger seed = plus(1, 2)
\tprint(seed)

functions:
\t// Changed in 0.33.184: an imported module function outside public: (next) is private, SEM005 per developer decision
\tpublic:
\t\tinteger next()
\t\t\treturn plus(times(2, 3), 1)
";

#[test]
fn multi_file_program_compiles_to_the_same_bytes_every_time() {
    let tmp = tempfile::tempdir().expect("create temp dir");
    for (name, source) in [
        ("main.cln", MAIN),
        ("shapes.cln", SHAPES),
        ("geometry.cln", GEOMETRY),
        ("utils.cln", UTILS),
        ("counters.cln", COUNTERS),
    ] {
        fs::write(tmp.path().join(name), source).expect("write module");
    }
    let main = tmp.path().join("main.cln");

    let compile = || {
        clean_language_compiler::compile_multi_file_with_memory_tier(
            &main,
            vec![tmp.path().to_path_buf()],
            2,
            None,
            MemoryTier::Standard,
            false,
        )
        .unwrap_or_else(|errors| {
            for e in &errors {
                eprintln!("compile error: {e}");
            }
            panic!("the multi-file program failed to compile");
        })
        .0
    };

    let first = compile();
    for build in 2..=6 {
        assert!(
            compile() == first,
            "build {build} of the same multi-file program differs from the first"
        );
    }
}

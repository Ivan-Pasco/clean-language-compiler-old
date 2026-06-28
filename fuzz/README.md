# Fuzz harness

Libfuzzer targets for the Clean Language compiler. Each target is a tight loop
around one entry point — `cargo fuzz run <target>` drives them with random
input until something panics or trips a sanitiser.

## Prerequisites

```sh
cargo install cargo-fuzz
rustup install nightly      # libfuzzer requires nightly
```

## Targets

| Target | Entry point | What it catches |
|---|---|---|
| `parser_random_input` | `CleanParser::parse_program` | Parser panics, infinite loops, stack overflows on arbitrary UTF-8 |
| `json_textToData` | Parser fed `json.textToData("...")` source | Front-end crashes triggered by adversarial JSON-shaped string literals |

## Run

```sh
# From the repo root:
cd fuzz
cargo +nightly fuzz run parser_random_input
cargo +nightly fuzz run json_textToData
```

Stop with Ctrl-C. Crashing inputs land in `fuzz/artifacts/<target>/` —
reproduce them with `cargo +nightly fuzz run <target> artifacts/<target>/<file>`
and reduce them with `cargo +nightly fuzz tmin`.

## CI

These targets are not run on every push (libfuzzer requires nightly and the
job is open-ended). They are intended for the nightly scheduled job —
see `.github/workflows/fuzz.yml` (when added) or run them manually on
machines with spare time before a release.

## Adding a target

1. Create `fuzz_targets/<name>.rs` with a `fuzz_target!` body.
2. Add a `[[bin]]` entry to `fuzz/Cargo.toml`.
3. Keep the target tight — one entry point per file.
4. Document the entry point and what failure modes it catches above.

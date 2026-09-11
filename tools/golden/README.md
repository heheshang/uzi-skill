# Golden reference testing

The Rust port must behave like the Python implementation. The Python scripts talk
in JSON dicts, so parity is testable: feed the same input, compare the output tree.

## Pieces

| file | role |
|---|---|
| `make_fixture.py` | builds deterministic `raw_data*.json` fixtures covering all 22 dims plus edge cases |
| `dump_python.py` | runs the **upstream** Python modules and dumps their JSON output |
| `dump_name_matcher.py` | dumps upstream `lib.name_matcher` results for a fixed index (`expected/name_matcher/queries.json`) |
| `dump_pyrandom.py` | dumps CPython `random` streams for 8 seeds (`expected/pyrandom/streams.json`) |
| `dump_mock.py` | captures `preview_with_mock.py`'s four mock artifacts into `assets/mock/` |
| `dump_xueqiu.py` | dumps `lib/xueqiu_browser.py` parsers for fixed HTML (`expected/xueqiu/`) |
| `dump_fallback.py` | dumps `lib/playwright_fallback.py` strategies + gates (`expected/fallback/`) |
| `compare.py` | standalone tree diff (key order + exact floats) for ad-hoc checks |
| `expected/<case>/` | checked-in golden output |
| `fixtures/` | checked-in input snapshots |

## Regenerate

```bash
python3 tools/golden/make_fixture.py
python3 tools/golden/dump_python.py tools/golden/fixtures/raw_data_synthetic.json tools/golden/expected/synthetic
python3 tools/golden/dump_python.py tools/golden/fixtures/raw_data_sparse.json    tools/golden/expected/sparse
python3 tools/golden/dump_name_matcher.py
python3 tools/golden/dump_pyrandom.py
python3 tools/golden/dump_mock.py
python3 tools/golden/dump_xueqiu.py
python3 tools/golden/dump_fallback.py
```

`dump_python.py` imports from `/tmp/uzi-src/skills/deep-analysis/scripts`, so the
upstream checkout must exist there.

## Comparing from Rust

```rust
let golden = uzi_core::testkit::load_golden("synthetic", "dimensions");
uzi_core::testkit::assert_json_eq(&actual, &golden, "score_dimensions/synthetic");
```

`assert_json_eq` enforces:
* identical key **order** (renderers walk dict order),
* exact float equality (the port must reproduce Python arithmetic, including
  `round()`),
* identical array lengths and element order.

## The one intentional non-determinism

Upstream `investor_personas.get_comment` picks its flavor line with an **unseeded**
`random.choice`, so two Python runs differ in the first line of each
`panel.investors[].comment`. The Rust port picks deterministically.

* `assert_json_eq` ignores the first line of any `comment` string and compares the
  remainder (the real headline / reasoning tail).
* `tools/golden/expected/<case>/persona_pools.json` holds every candidate line per
  `(investor, signal)`. Use `testkit::assert_persona_line_known` to assert the
  Rust-produced line is one the upstream could have produced, after `{name}` /
  `{industry}` / `{pe}` / `{roe}` / `{stage}` / `{growth}` / `{price}` substitution.

Everything else — scores, verdicts, weights, distribution counters, school
scores, synthesis numbers and labels — is deterministic and must match exactly.

## The mock preview assets

`assets/mock/` holds `preview_with_mock.py`'s four mock payloads plus its prose
tables, captured by `dump_mock.py` (the upstream script run with
`write_task_output` / `assemble` stubbed). The port regenerates `panel.json` from
CPython's seeded stream via `uzi_core::pyrandom`, and
`crates/uzi-cli/src/preview.rs` asserts it reproduces the captured fixture
exactly, so the two cannot drift.

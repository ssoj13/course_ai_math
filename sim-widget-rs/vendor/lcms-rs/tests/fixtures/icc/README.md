# ICC test profiles

Real profiles the unit and integration tests open (`crate::test_profile` for unit tests,
`tests/golden.rs` and `tests/real_icc.rs`). Copied byte for byte from Little-CMS
(<https://github.com/mm2/Little-CMS>, commit `8f7f96b17c6d3cea93275ffc654cfca6d046fc63`,
lcms2 2.19, MIT licence, © Marti Maria Saguer) so that building or testing lcms-rs does not
need the C sources.

| File | Upstream path |
|---|---|
| `crayons.icc` | `testbed/crayons.icc` |
| `ibm-t61.icc` | `testbed/ibm-t61.icc` |
| `test1.icc`, `test2.icc`, `test3.icc`, `test5.icc` | `testbed/` (identical copies also sit in `plugins/test_profiles/`) |
| `test0.icc` | `plugins/test_profiles/test0.icc` |
| `alltags.icc` | `fuzzers/corpus/alltags.icc` |

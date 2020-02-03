# grass

An implementation of the SASS spec in pure Rust

To run the official test suite,

```bash
git clone https://github.com/ConnorSkees/grass
cd grass
git submodule init
git submodule update
cargo b --release
./sass-spec/sass-spec.rb -c './target/release/grass'
```

```
2020-02-03
PASSING: 242
FAILING: 4851
TOTAL: 5093
```

```
2020-01-27
PASSING: 186
FAILING: 4907
TOTAL: 5093
```

```
2020-01-20
PASSING: 143
FAILING: 4950
TOTAL: 5093
```

## Features

The focus right now is just getting the most basic tests to pass.

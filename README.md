# poseidon

## Dependencies

* [OTP 24+](https://www.erlang.org/)
* [`rebar3`](https://www.rebar3.org/)
* [Rust](https://www.rust-lang.org/)

## Development

### Building

To compile this library, the following command can be used:

```sh
rebar3 compile
```

### Running Checks

To run all the checks, it is possible to use the `check` alias:

```sh
rebar3 check
```

This will run all the tests (`eunit`, Common Tests and `proper` tests) along
with `xref`, `dialyzer`, `edoc` and code coverage.

### Running Interactively

To run the library inside an interactive Erlang shell, the following command
may be used:

```sh
rebar3 shell
```

## Licensing

This software is licensed under the BSD3 license.

# Rust Bisector

This is a tool written to find which commit introduced an error message into Rust,
by bisecting the commits of the Rust repository.

In order to use it, you will need to get a local clone of the `rust-lang/rust` repo.
Then, the recommended approach is to get Docker, and run the following command:

```
cd test
docker build -t bisector .
cd ..
cargo build --release
RUST_LOG=bisect_rust=info  target/release/bisect --preserve --test test.sh
```

For each run, copy test.example.sh into test.sh and configure it to match your test case.

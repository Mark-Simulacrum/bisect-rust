# Rust Bisector

This is a tool written to find which commit introduced an error message into Rust,
by bisecting the commits of the Rust repository.

In order to use it, you will need to get a GitHub API token (this requirement is temporary, and is
will be replaced with a checkout of the rust-lang/rust repo). Then, the recommended approach is to
get Docker, and run the following command:

```
RUST_LOG=bisect_rust=info \
    GH_API_TOKEN=TOKEN_HERE \
    cargo run -- --preserve --triple x86_64-unknown-linux-gnu --test test.sh
```

With the test.sh in this repository, this will run a docker container to isolate the compilation of
the cloned crate. You'll need to update the directory of the crate (`{{GIT_DIRECTORY}}`) and the
error message you're looking for (`{{ERROR_MESSAGE}}`).

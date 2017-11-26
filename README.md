# Rust Bisector

This is a tool written to find which commit introduced an error message into Rust,
by bisecting the commits of the Rust repository.

In order to use it, first record the range of commits which contains a regression.
Note that if a commit happened more than 90 days ago, the bisector may not be
able to download the build artifacts.
Then, the recommended approach is to get Docker, and run the following command:

```
cd test
docker build -t bisector .
cd ..
cargo build --release
RUST_LOG=rust_sysroot=info target/release/bisect \
    --preserve \
    --test test.sh \
    --start 5f44c653cff61d0f55f53e07a188f755c7acddd1 \
    --end e97ba83287a6f0f85cc9cc7a51ab309487e17038
```

For each run, copy `test.example.sh` into `test.sh` and configure it to match
your test case. The script should exit with 0 if the regression occured, and
exit with nonzero code if no regression is detected.

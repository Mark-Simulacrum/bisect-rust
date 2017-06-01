#!/bin/sh

TEST_DIRECTORY="test"
ERROR_MESSAGE="internal compiler error"

docker run --rm -v `pwd`:/dir -v `pwd`/$TEST_DIRECTORY:/source \
	-e RUSTC=/dir/$RUSTC_RELATIVE -e CARGO_RELATIVE bisector \
	bash -c 'cd /source && rm -fr target ; /dir/$CARGO_RELATIVE test' \
	2>&1 | rg -q "$ERROR_MESSAGE"

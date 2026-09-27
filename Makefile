.PHONY: run setup test build

run:
	@if [ ! -d example/node_modules ]; then npm --prefix example install; fi
	npm --prefix example run dev

setup:
	npm --prefix example run setup

test:
	npm --prefix example test

build:
	npm --prefix example run build

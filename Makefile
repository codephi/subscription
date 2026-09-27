.PHONY: run monitoring setup test build

run:
	@if [ ! -x admin-ui/node_modules/.bin/vite ]; then npm --prefix admin-ui install; fi
	@if [ ! -d example/node_modules ]; then npm --prefix example install; fi
	./example/node_modules/.bin/concurrently -k -n subscription,admin,tasklab -c cyan,green,blue \
		"APP_PORT=3000 cargo run" \
		"npm --prefix admin-ui run dev -- --host 127.0.0.1 --port 5173 --strictPort" \
		"npm --prefix example run dev"

monitoring:
	./scripts/monitor-backends.sh

setup:
	npm --prefix example run setup

test:
	npm --prefix example test

build:
	npm --prefix example run build

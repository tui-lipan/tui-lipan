.PHONY: hello hello-build search-palette search-palette-build

hello-build:
	npm install --prefix hello
	cd hello && wasm-pack build --target web

hello: hello-build
	python3 hello/serve.py

search-palette-build:
	npm install --prefix search_palette
	cd search_palette && wasm-pack build --target web

search-palette: search-palette-build
	python3 search_palette/serve.py

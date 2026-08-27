CARGO ?= cargo
LATEX_FONT_SIZE ?= 12pt
LATEX_CHAPTER_OPENING ?= right

LIT_BOOK := lit/index.lit
PACKAGE_TARGET_DIR := target/package-verify
LATEX_OUTPUT_DIR := latex
LATEX_DOCUMENT := index.tex

.PHONY: build test latex pdf install clean

build:
	@if [ ! -f "$(LIT_BOOK)" ]; then echo "Canonical book not found: $(LIT_BOOK)" >&2; exit 1; fi
	@set -eu; \
	mkdir -p target; \
	stage_dir=$$(mktemp -d target/bootstrap-update.XXXXXX); \
	html_stage=$$(mktemp -d target/litweb-html.XXXXXX); \
	cleanup() { \
		rm -rf "$$stage_dir"; \
		if [ -n "$$html_stage" ]; then rm -rf "$$html_stage"; fi; \
	}; \
	trap cleanup EXIT HUP INT TERM; \
	$(CARGO) run --locked --offline --example bootstrap -- stage "$$stage_dir"; \
	cp "$$stage_dir"/src/*.rs src/; \
	$(CARGO) build --locked --offline --bin lw; \
	target/debug/lw -w -odir "$$html_stage" "$(LIT_BOOK)"; \
	rm -rf html; \
	mv "$$html_stage" html; \
	html_stage=; \
	echo "Updated src/ and generated HTML under html/."

test:
	$(CARGO) fmt --all -- --check
	$(CARGO) test --locked --offline
	$(CARGO) clippy --locked --offline --all-targets --all-features -- -D warnings
	$(CARGO) doc --locked --offline --no-deps
	$(CARGO) build --locked --offline --release
	$(CARGO) run --locked --offline --example bootstrap -- check
	$(CARGO) package --locked --offline --allow-dirty --target-dir $(PACKAGE_TARGET_DIR)

latex:
	@if [ ! -f "$(LIT_BOOK)" ]; then echo "Canonical book not found: $(LIT_BOOK)" >&2; exit 1; fi
	@set -eu; \
	mkdir -p target; \
	latex_stage=$$(mktemp -d target/litweb-latex.XXXXXX); \
	cleanup() { if [ -n "$$latex_stage" ]; then rm -rf "$$latex_stage"; fi; }; \
	trap cleanup EXIT HUP INT TERM; \
	$(CARGO) build --locked --offline --bin lw; \
	target/debug/lw --weave --format latex --font-size "$(LATEX_FONT_SIZE)" \
		--chapter-opening "$(LATEX_CHAPTER_OPENING)" \
		--out-dir "$$latex_stage" "$(LIT_BOOK)"; \
	rm -rf "$(LATEX_OUTPUT_DIR)"; \
	mv "$$latex_stage" "$(LATEX_OUTPUT_DIR)"; \
	latex_stage=; \
	echo "Generated LaTeX under $(LATEX_OUTPUT_DIR)/."

pdf: latex
	cd "$(LATEX_OUTPUT_DIR)" && latexmk -lualatex -halt-on-error -interaction=nonstopmode \
		-e '$$lualatex = "lualatex -no-shell-escape %O %S"' "$(LATEX_DOCUMENT)"

install: build
	$(CARGO) install --locked --offline --force --path . --root "$(HOME)/.cargo"

clean:
	$(CARGO) clean
	rm -rf html "$(LATEX_OUTPUT_DIR)"

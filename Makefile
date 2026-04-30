PREFIX ?= $(HOME)/.local
BINDIR ?= $(PREFIX)/bin

.PHONY: all build install uninstall clean test

all: build

build:
	cargo build --release

install: build
	install -d $(BINDIR)
	install -m 0755 target/release/gmenu $(BINDIR)/gmenu

uninstall:
	rm -f $(BINDIR)/gmenu

test:
	cargo test

clean:
	cargo clean

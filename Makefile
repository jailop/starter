UNAME_S := $(shell uname -s)

ifeq ($(UNAME_S),Darwin)
TARGETS = x86_64-apple-darwin x86_64-unknown-linux-gnu x86_64-pc-windows-gnu
else
TARGETS = x86_64-unknown-linux-gnu x86_64-pc-windows-gnu
endif

.PHONY: all

all: $(TARGETS)

$(TARGETS):
	rustup target add $@
	cargo build --release --target $@

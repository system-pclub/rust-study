# Regression test for issue #10973

-include ../tools.mk

all:
	$(RUSTC) --crate-type=rlib --test foo.rs
	rm $(TMPDIR)/foo.bc && exit 1 || exit 0

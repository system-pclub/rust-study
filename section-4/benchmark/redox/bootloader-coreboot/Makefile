ARCH=x86
TARGET=$(ARCH)-unknown-none

export LD=ld
export OBJCOPY=objcopy
export RUST_TARGET_PATH=$(CURDIR)/targets
export XARGO_HOME=$(CURDIR)/target/xargo

all: build/bootloader

clean:
	rm -rf build

build/libstartup.o: src/nasm/startup.asm src/nasm/*
	mkdir -p build
ifeq ($(KERNEL),)
	nasm -f elf32 $< -I src/nasm/ -o $@
else
	nasm -f elf32 $< -I src/nasm/ -D KERNEL=$(KERNEL) -o $@
endif

build/libbootloader.a: Cargo.lock Cargo.toml src/* src/*/* src/*/*/* src/*/*/*/*
	mkdir -p build
	xargo rustc --lib --target $(TARGET) --release -- -C soft-float -C debuginfo=2 --emit link=$@

build/bootloader: linkers/$(ARCH).ld build/libstartup.o build/libbootloader.a
	mkdir -p build
	$(LD) -m elf_i386 --gc-sections -z max-page-size=0x1000 -T $< -o $@ build/libstartup.o build/libbootloader.a && \
	$(OBJCOPY) --only-keep-debug $@ $@.sym && \
	$(OBJCOPY) --strip-debug $@

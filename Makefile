# Builds the core with Cargo, then copies it to the name libretro frontends expect.
# Cargo always prefixes cdylibs with "lib" on Unix, so it can't produce this name itself.

CORE_NAME := ruffle_libretro

ifeq ($(platform),)
	ifeq ($(OS),Windows_NT)
		platform := win
	else ifeq ($(shell uname -s),Darwin)
		platform := osx
	else
		platform := unix
	endif
endif

ifeq ($(platform),win)
	LIB_PREFIX :=
	EXT := dll
else ifeq ($(platform),osx)
	LIB_PREFIX := lib
	EXT := dylib
else
	LIB_PREFIX := lib
	EXT := so
endif

ifeq ($(DEBUG),1)
	PROFILE := dev
	PROFILE_DIR := debug
else
	PROFILE := release
	PROFILE_DIR := release
endif

CARGO ?= cargo
CARGO_TARGET_DIR ?= target
BUILT := $(CARGO_TARGET_DIR)/$(PROFILE_DIR)/$(LIB_PREFIX)$(CORE_NAME).$(EXT)
TARGET := $(CORE_NAME).$(EXT)

.PHONY: all clean $(BUILT)

all: $(TARGET)

$(BUILT):
	$(CARGO) build --profile $(PROFILE) $(CARGO_FLAGS)

$(TARGET): $(BUILT)
	cp $< $@

clean:
	$(CARGO) clean
	rm -f $(TARGET)

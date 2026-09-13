CC ?= gcc
CFLAGS ?= -g -O0

all: build/app

build/app: main.c
	mkdir -p build
	$(CC) $(CFLAGS) -o $@ $<

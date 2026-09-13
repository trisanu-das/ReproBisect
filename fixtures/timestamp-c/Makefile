CC ?= gcc
CFLAGS ?= -O0 -Wall -Wextra

all: build/app

build/app: main.c
	mkdir -p build
	$(CC) $(CFLAGS) -o $@ $<

clean:
	rm -rf build
